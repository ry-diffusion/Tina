// Reset / Append / Send: the three handlers that grow the message
// factory. `Append` carries the optimistic-echo confirmation logic;
// `Send` synthesises an optimistic local row and hands the trimmed text
// to the parent for the worker round-trip.

use std::collections::{HashMap, HashSet};

use adw::prelude::*;
use relm4::ComponentSender;
use tina_db::MessageRow;

use crate::components::message_bubble::MessageItem;

use super::super::build::{build_item, collapse_against, day_flips};
use super::super::messages::{ChatTabOutput, COLLAPSE_WINDOW_SECS};
use super::super::model::ChatTab;
use super::echo::match_pending_echoes;

impl ChatTab {
    pub(in crate::components::chat_tab) fn handle_reset(
        &mut self,
        rows: Vec<MessageRow>,
        sender: &ComponentSender<Self>,
    ) {
        self.oldest_ts = rows.iter().map(|r| r.timestamp).min();
        self.newest_ts = rows.iter().map(|r| r.timestamp).max();
        self.reached_top = rows.len() < 50;
        self.loading_older = false;
        // Reset always pulls the newest 50 from the worker — the
        // factory tail is the actual DB tail at this moment. Live
        // pushes via `MessagesAppended` keep it that way; trim paths
        // (TrimBottom, Append cumulative cap) clear this.
        self.loading_newer = false;
        self.reached_bottom = true;
        // Force sticky-bottom on every chat open. The connect_changed
        // handler we registered will catch each upper-grew tick as the
        // factory lays out the rows and re-scroll to the bottom; no
        // manual timeouts needed.
        self.bottomed.set(true);
        let mut avatar_fetches: Vec<String> = Vec::new();
        {
            let chat_ctx = self.chat_context();
            self.list.clear();
            self.seen_message_ids.clear();
            let mut cursor_sender: Option<String> = None;
            let mut cursor_ts: Option<i64> = None;
            let mut cursor_day: Option<String> = None;
            for row in &rows {
                let collapsed =
                    collapse_against(row, &mut cursor_sender, &mut cursor_ts);
                let day_flip = day_flips(row, &mut cursor_day);
                self.seen_message_ids.insert(row.message_id.clone());
                let mut item = build_item(
                    row,
                    collapsed,
                    &self.avatars,
                    &self.media,
                    &self.mentions,
                    self.user_jid.as_ref().map(|x| x.raw()),
                    &chat_ctx,
                    &mut |jid| avatar_fetches.push(jid),
                );
                if day_flip {
                    item.is_first_of_day = true;
                    item.day_label = crate::time::format_day_divider(row.timestamp);
                    item.is_collapsed = false;
                }
                self.list.append(self.wrap_row(item));
            }
        }
        for jid in avatar_fetches {
            let _ = sender.output(ChatTabOutput::RequestFetchAvatar(
                tina_core::WaIdentity::parse(&jid),
            ));
        }
        self.auto_queue_downloads(&rows, sender);
        self.maybe_mark_read(&rows, sender);
    }

    /// Apply the active DownloadMethod to a fresh batch of rows.
    /// `OnDemand` / `Eager` fire a download for every visual-media
    /// row that's still in `media_status = "none"` and lacks a local
    /// path; `Manual` skips. Per-id dedup via `MediaInventory` so
    /// reopening a tab doesn't re-queue the same fetches.
    /// Send Read receipts for the from_other rows in `rows` when the
    /// user is currently parked at the bottom of the thread (i.e. the
    /// rows are visible). Groups by sender JID — whatsmeow's MarkRead
    /// expects all ids in one call to share a sender. Newsletters
    /// don't generate inbound message receipts so we skip them.
    pub(super) fn maybe_mark_read(
        &mut self,
        rows: &[tina_db::MessageRow],
        sender: &relm4::ComponentSender<Self>,
    ) {
        if !self.bottomed.get() {
            return;
        }
        if matches!(self.kind.as_str(), "newsletter" | "status" | "broadcast") {
            return;
        }
        let mut by_sender: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for r in rows {
            if r.is_from_me {
                continue;
            }
            // For DMs, the sender JID is just the chat JID; for
            // groups we need the per-row participant.
            let sender_jid = r
                .sender_jid
                .clone()
                .unwrap_or_else(|| self.chat_id.clone());
            by_sender
                .entry(sender_jid)
                .or_default()
                .push(r.message_id.clone());
        }
        for (sender_jid, message_ids) in by_sender {
            if message_ids.is_empty() {
                continue;
            }
            let _ = sender.output(super::super::messages::ChatTabOutput::RequestMarkRead {
                chat_id: self.chat_id.clone(),
                sender_jid,
                message_ids,
            });
        }
    }

    pub(super) fn auto_queue_downloads(
        &mut self,
        rows: &[MessageRow],
        sender: &ComponentSender<Self>,
    ) {
        use crate::components::settings::DownloadMethod;
        if matches!(self.media.download_method(), DownloadMethod::Manual) {
            return;
        }
        let mut ids: Vec<String> = Vec::new();
        for row in rows {
            if row.media_status != "none" {
                continue;
            }
            if row
                .media_path
                .as_deref()
                .map(|p| !p.is_empty())
                .unwrap_or(false)
            {
                continue;
            }
            if !matches!(
                row.message_type.as_str(),
                "image" | "video" | "audio" | "sticker" | "document"
            ) {
                continue;
            }
            if !self.media.try_mark_auto_queued(&row.message_id) {
                continue;
            }
            ids.push(row.message_id.clone());
        }
        for id in ids {
            // Reuse the click-path handler so the bubble flips to
            // its spinner state via the same code that powers a
            // manual click.
            self.handle_request_media_download(id, sender);
        }
    }

    pub(in crate::components::chat_tab) fn handle_append(
        &mut self,
        rows: Vec<MessageRow>,
        sender: &ComponentSender<Self>,
    ) {
        tracing::info!(
            chat = %self.chat_id,
            count = rows.len(),
            "ChatTab::Append"
        );

        let (avatar_fetches_ack, confirmed_server_ids) =
            self.confirm_pending_echoes(&rows);

        let mut new_rows: Vec<_> = rows
            .into_iter()
            .filter(|r| !confirmed_server_ids.contains(&r.message_id))
            .filter(|r| self.seen_message_ids.insert(r.message_id.clone()))
            .collect();

        // Cap the batch when the user is parked at the bottom (sticky
        // autoscroll). History-sync flushes can deliver thousands of
        // rows in a single MessagesAppended; rendering them all just
        // to prune on the next NearBottom freezes the UI for seconds.
        // The tail is what the autoscroll lands on anyway, so keep
        // only that and discard the older portion of this batch — the
        // user can page back via near-top to retrieve them from disk.
        // Tightened from 200 → 100 to keep big sync flushes from
        // realising 200 widget subtrees in a single update tick. The
        // newly-wired `LoadNewer` path will refill the dropped tail
        // on demand if the user scrolls there.
        const APPEND_BATCH_CAP: usize = 100;
        if self.bottomed.get() && new_rows.len() > APPEND_BATCH_CAP {
            let drop = new_rows.len() - APPEND_BATCH_CAP;
            let dropped: Vec<_> = new_rows.drain(..drop).collect();
            for r in &dropped {
                // Re-allow these IDs so a future older-page fetch can
                // surface them — they live in the DB, just not in the
                // factory.
                self.seen_message_ids.remove(&r.message_id);
            }
            tracing::warn!(
                chat = %self.chat_id,
                dropped = drop,
                kept = new_rows.len(),
                "ChatTab::Append: capped during autoscroll-bottomed sync"
            );
            // We're dropping rows older than the newest in this
            // batch; the gap means older history is once again
            // legitimately available to fetch.
            self.reached_top = false;
        }

        let mut avatar_fetches = avatar_fetches_ack;
        if new_rows.is_empty() {
            for jid in avatar_fetches {
                let _ = sender.output(ChatTabOutput::RequestFetchAvatar(
                    tina_core::WaIdentity::parse(&jid),
                ));
            }
            return;
        }
        // Always seed the collapse cursor from the factory's current
        // trailing item — never from a stashed field. Any other path
        // (echo drop, pane move, tab switch, etc.) is allowed to mutate
        // the factory between Append calls, and we'd silently misgroup
        // messages if we trusted a stale `last_sender`. The factory IS
        // the source of truth.
        let (mut cursor_sender, mut cursor_ts) = self.factory_tail_cursor();
        let mut cursor_day = self.factory_tail_day();
        let chat_ctx = self.chat_context();
        for row in &new_rows {
            let collapsed = collapse_against(row, &mut cursor_sender, &mut cursor_ts);
            let day_flip = day_flips(row, &mut cursor_day);
            let mut item = build_item(
                row,
                collapsed,
                &self.avatars,
                &self.media,
                &self.mentions,
                self.user_jid.as_ref().map(|x| x.raw()),
                &chat_ctx,
                &mut |jid| avatar_fetches.push(jid),
            );
            if day_flip {
                item.is_first_of_day = true;
                item.day_label = crate::time::format_day_divider(row.timestamp);
                item.is_collapsed = false;
            }
            let wrapped = self.wrap_row(item);
            self.list.append(wrapped);
        }
        for jid in avatar_fetches {
            let _ = sender.output(ChatTabOutput::RequestFetchAvatar(
                tina_core::WaIdentity::parse(&jid),
            ));
        }

        // Live pushes always extend the tail — update newest_ts so
        // a subsequent NearBottomFetch (e.g. after a TrimBottom)
        // pages forward from the right anchor.
        if let Some(t) = new_rows.iter().map(|r| r.timestamp).max() {
            self.newest_ts = Some(match self.newest_ts {
                Some(prev) => prev.max(t),
                None => t,
            });
        }

        // Cumulative cap: even with each batch capped, repeated
        // appends during a long sync can stack up. Trim from the
        // front when bottomed and over the soft cap so the factory
        // stays bounded. Tightened from 250/200 → 200/150 — the
        // memory budget for a media-heavy chat is dominated by these
        // live widgets, not by the lighter-weight items still in
        // memory but offscreen.
        const FACTORY_SOFT_CAP: usize = 200;
        const FACTORY_TARGET: usize = 150;
        if self.bottomed.get() && (self.list.len() as usize) > FACTORY_SOFT_CAP {
            let to_drop = self.list.len() as usize - FACTORY_TARGET;
            let mut dropped_ids: Vec<String> = Vec::with_capacity(to_drop);
            for i in 0..to_drop {
                if let Some(h) = self.list.get(i as u32) {
                    dropped_ids.push(h.borrow().item.id.clone());
                }
            }
            // Remove from the front. Each `remove(0)` shifts the
            // remaining indices down — pop the same offset N times.
            for _ in 0..to_drop {
                self.list.remove(0);
            }
            for id in &dropped_ids {
                self.seen_message_ids.remove(id);
            }
            self.ui_state.forget(&dropped_ids);
            self.oldest_ts = self.list_front().map(|r| r.item.timestamp_unix);
            self.reached_top = false;
            tracing::info!(
                chat = %self.chat_id,
                dropped = to_drop,
                remaining = self.list.len(),
                "ChatTab::Append: pruned cumulative overflow"
            );
        }
        self.auto_queue_downloads(&new_rows, sender);
        self.maybe_mark_read(&new_rows, sender);
        // The connect_changed handler will autoscroll if `bottomed` —
        // meaning we only follow new messages when the user was already
        // at (or near) the bottom. If they scrolled up to read history,
        // they stay where they are.
    }

    /// For each from_me row that confirms a pending optimistic echo,
    /// REPLACE the local placeholder in-place (preserving its
    /// `is_collapsed` flag) rather than remove+append. The
    /// remove+append path silently broke the collapse seam when the
    /// dropped local was the head of a from_me run: the next local in
    /// the run stayed flagged collapsed, ended up first-visible, and
    /// rendered without an avatar — visually attaching to whoever
    /// spoke before.
    fn confirm_pending_echoes(
        &mut self,
        rows: &[MessageRow],
    ) -> (Vec<String>, HashSet<String>) {
        // Fast path: nothing pending → skip the O(n) walk over the
        // entire factory. History sync emits MessagesAppended for every
        // open tab; we'd otherwise pay the walk for every tab with no
        // optimistic echoes outstanding.
        if self.pending_echoes.is_empty() && self.pending_media_echoes.is_empty() {
            return (Vec::new(), HashSet::new());
        }

        let local_idx_state: HashMap<String, (usize, bool)> = self
            .list_snapshot()
            .into_iter()
            .map(|(i, r)| (r.item.id.clone(), (i as usize, r.item.is_collapsed)))
            .collect();
        let mut m = match_pending_echoes(rows, &mut self.pending_echoes, &local_idx_state);

        // Media path: match by `media_sha256`. Drains the matched
        // entry, appends to the same `replacements` vec so the
        // remove/insert tail-first ordering still works.
        if !self.pending_media_echoes.is_empty() {
            for r in rows {
                if !r.is_from_me {
                    continue;
                }
                let Some(sha) = r.media_sha256.clone() else {
                    continue;
                };
                let Some(queue) = self.pending_media_echoes.get_mut(&sha) else {
                    continue;
                };
                let Some(local_id) = queue.pop_front() else {
                    continue;
                };
                if queue.is_empty() {
                    self.pending_media_echoes.remove(&sha);
                }
                if let Some((idx, was_collapsed)) = local_idx_state.get(&local_id).copied() {
                    m.replacements.push((idx, r.clone(), was_collapsed));
                    m.confirmed_server_ids.insert(r.message_id.clone());
                    m.confirmed_local_ids.push(local_id);
                }
            }
            // Re-sort tail-first for the safe remove/insert pattern.
            m.replacements
                .sort_by_key(|(idx, _, _)| std::cmp::Reverse(*idx));
        }

        let mut avatar_fetches: Vec<String> = Vec::new();
        let chat_ctx = self.chat_context();
        for (idx, row, was_collapsed) in m.replacements {
            let item = build_item(
                &row,
                was_collapsed,
                &self.avatars,
                &self.media,
                &self.mentions,
                self.user_jid.as_ref().map(|x| x.raw()),
                &chat_ctx,
                &mut |jid| avatar_fetches.push(jid),
            );
            let wrapped = self.wrap_row(item);
            self.list.remove(idx as u32);
            self.list.insert(idx as u32, wrapped);
        }
        for id in &m.confirmed_local_ids {
            self.seen_message_ids.remove(id);
        }
        for id in &m.confirmed_server_ids {
            self.seen_message_ids.insert(id.clone());
        }
        (avatar_fetches, m.confirmed_server_ids)
    }

    pub(in crate::components::chat_tab) fn handle_send(&mut self, sender: &ComponentSender<Self>) {
        let text = self.composer_buffer.text().to_string();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        if let Some((prev, when)) = &self.last_send
            && prev == trimmed && when.elapsed() < std::time::Duration::from_secs(1) {
                tracing::warn!(
                    chat = %self.chat_id,
                    "Send debounced (duplicate within 1s)"
                );
                self.composer_buffer.set_text("");
                return;
            }
        self.last_send = Some((trimmed.to_string(), std::time::Instant::now()));

        let local_item = self.build_optimistic_echo(trimmed);
        let local_id = local_item.id.clone();
        self.seen_message_ids.insert(local_id.clone());
        self.pending_echoes
            .entry(trimmed.to_string())
            .or_default()
            .push_back(local_id);
        // Force sticky on send — even if the user had scrolled up to
        // read history, sending a message is a strong intent signal
        // that they want to see what they just typed.
        // Render the markup BEFORE pushing to the list — the new
        // virtualised bind reads `cached_markup` directly, so an
        // empty cache made the optimistic echo appear blank until
        // the server echo replaced it.
        let mut local_item = local_item;
        local_item.recompute_markup();
        self.bottomed.set(true);
        let wrapped = self.wrap_row(local_item);
        self.list.append(wrapped);

        // Mentions live in `pending_mentions` rather than being
        // re-derived from `trimmed` because the popover may have
        // inserted a chip whose digits the user later tweaked. We
        // filter against the final text so a mention picked and
        // then deleted doesn't leak into `contextInfo.MentionedJID`.
        let mut mentioned_jids: Vec<String> = self
            .pending_mentions
            .iter()
            .filter(|jid| {
                let digits = jid.split('@').next().unwrap_or("");
                !digits.is_empty() && trimmed.contains(&format!("@{digits}"))
            })
            .cloned()
            .collect();
        mentioned_jids.sort();
        mentioned_jids.dedup();
        self.pending_mentions.clear();
        let _ = sender.output(ChatTabOutput::Send {
            chat_id: self.chat_id.clone(),
            text: trimmed.to_string(),
            mentioned_jids,
        });
        self.composer_buffer.set_text("");
    }

    /// Synthesise a text bubble with a sentinel id for the optimistic echo.
    /// When the worker echoes the real row back, the matching local
    /// entry is dropped so the real one slots in at the same position.
    fn build_optimistic_echo(&self, trimmed: &str) -> MessageItem {
        let local_id = format!("local-{}", uuid::Uuid::now_v7());
        let now_unix = optimistic_secs();
        let mut item = self.build_optimistic_base(local_id, now_unix);
        item.content = trimmed.to_string();
        item.message_type = "text".to_string();
        item
    }

    /// Common fields shared by all optimistic echoes (text and media).
    /// Callers fill in the type-specific fields (`content`, `message_type`,
    /// media path / status, etc.) after calling this.
    pub(in crate::components::chat_tab) fn build_optimistic_base(
        &self,
        local_id: String,
        now_unix: i64,
    ) -> MessageItem {
        let (cursor_sender, cursor_ts) = self.factory_tail_cursor();
        let local_collapsed = match (cursor_sender.as_deref(), cursor_ts) {
            (Some("\0me"), Some(prev_ts)) => {
                now_unix.saturating_sub(prev_ts) <= COLLAPSE_WINDOW_SECS
            }
            _ => false,
        };
        let local_avatar = self
            .user_jid
            .as_ref()
            .and_then(|j| self.avatars.get(j.raw()));
        MessageItem {
            id: local_id,
            from_me: true,
            sender_name: String::new(),
            sender_jid: self.user_jid.as_ref().map(|x| x.raw().to_string()),
            sender_avatar_path: local_avatar,
            chat_kind: self.kind.clone(),
            chat_display_name: if self.name.is_empty() {
                None
            } else {
                Some(self.name.clone())
            },
            chat_avatar_path: self.avatars.get(&self.chat_id),
            is_collapsed: local_collapsed,
            is_first_of_day: false,
            day_label: String::new(),
            content: String::new(),
            message_type: String::new(),
            timestamp: crate::time::format_message_time(now_unix),
            short_time: crate::time::format_short_time(now_unix),
            timestamp_unix: now_unix,
            media_summary: String::new(),
            media_mimetype: None,
            media_size_bytes: None,
            media_width: None,
            media_height: None,
            media_duration_secs: None,
            media_path: None,
            media_status: "none".to_string(),
            media_filename: None,
            media_sha256: None,
            delivery_status: "pending".to_string(),
            thumbnail: None,
            quoted_message_id: None,
            quoted_sender_id: None,
            quoted_sender_name: None,
            quoted_preview: None,
            mentions: Vec::new(),
            cached_markup: String::new(),
        }
    }
}

pub(in crate::components::chat_tab) fn optimistic_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}
