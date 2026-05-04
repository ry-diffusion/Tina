// Reset / Append / Send: the three handlers that grow the message
// factory. `Append` carries the optimistic-echo confirmation logic;
// `Send` synthesises an optimistic local row and hands the trimmed text
// to the parent for the worker round-trip.

use std::collections::{HashMap, HashSet};

use adw::prelude::*;
use relm4::ComponentSender;
use tina_db::MessageRow;

use crate::components::message_bubble::MessageItem;

use super::super::build::{build_item, collapse_against};
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
        self.reached_top = rows.len() < 50;
        self.loading_older = false;
        // Force sticky-bottom on every chat open. The connect_changed
        // handler we registered will catch each upper-grew tick as the
        // factory lays out the rows and re-scroll to the bottom; no
        // manual timeouts needed.
        self.bottomed.set(true);
        let mut avatar_fetches: Vec<String> = Vec::new();
        {
            let chat_ctx = self.chat_context();
            let mut guard = self.messages.guard();
            guard.clear();
            self.seen_message_ids.clear();
            let mut cursor_sender: Option<String> = None;
            let mut cursor_ts: Option<i64> = None;
            for row in &rows {
                let collapsed =
                    collapse_against(row, &mut cursor_sender, &mut cursor_ts);
                self.seen_message_ids.insert(row.message_id.clone());
                let item = build_item(
                    row,
                    collapsed,
                    &self.avatars,
                    &self.media,
                    self.user_jid.as_ref().map(|x| x.raw()),
                    &chat_ctx,
                    &mut |jid| avatar_fetches.push(jid),
                );
                guard.push_back(item);
            }
        }
        for jid in avatar_fetches {
            let _ = sender.output(ChatTabOutput::RequestFetchAvatar(
                tina_core::WaIdentity::parse(&jid),
            ));
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
        const APPEND_BATCH_CAP: usize = 200;
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
        let chat_ctx = self.chat_context();
        {
            let mut guard = self.messages.guard();
            for row in &new_rows {
                let collapsed = collapse_against(row, &mut cursor_sender, &mut cursor_ts);
                let item = build_item(
                    row,
                    collapsed,
                    &self.avatars,
                    &self.media,
                    self.user_jid.as_ref().map(|x| x.raw()),
                    &chat_ctx,
                    &mut |jid| avatar_fetches.push(jid),
                );
                guard.push_back(item);
            }
        }
        for jid in avatar_fetches {
            let _ = sender.output(ChatTabOutput::RequestFetchAvatar(
                tina_core::WaIdentity::parse(&jid),
            ));
        }

        // Cumulative cap: even with each batch capped, repeated
        // appends during a long sync can stack up. Trim from the
        // front when bottomed and over the soft cap so the factory
        // never holds more than ~200 widgets during sync.
        const FACTORY_SOFT_CAP: usize = 250;
        const FACTORY_TARGET: usize = 200;
        if self.bottomed.get() && self.messages.len() > FACTORY_SOFT_CAP {
            let to_drop = self.messages.len() - FACTORY_TARGET;
            let mut dropped_ids: Vec<String> = Vec::with_capacity(to_drop);
            {
                let guard = self.messages.guard();
                for fac in guard.iter().take(to_drop) {
                    dropped_ids.push(fac.item.id.clone());
                }
            }
            {
                let mut guard = self.messages.guard();
                for _ in 0..to_drop {
                    guard.pop_front();
                }
            }
            for id in &dropped_ids {
                self.seen_message_ids.remove(id);
            }
            self.oldest_ts = self
                .messages
                .guard()
                .iter()
                .next()
                .map(|f| f.item.timestamp_unix);
            self.reached_top = false;
            tracing::info!(
                chat = %self.chat_id,
                dropped = to_drop,
                remaining = self.messages.len(),
                "ChatTab::Append: pruned cumulative overflow"
            );
        }
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
        if self.pending_echoes.is_empty() {
            return (Vec::new(), HashSet::new());
        }

        let local_idx_state: HashMap<String, (usize, bool)> = self
            .messages
            .guard()
            .iter()
            .enumerate()
            .map(|(i, f)| (f.item.id.clone(), (i, f.item.is_collapsed)))
            .collect();
        let m = match_pending_echoes(rows, &mut self.pending_echoes, &local_idx_state);

        let mut avatar_fetches: Vec<String> = Vec::new();
        let chat_ctx = self.chat_context();
        for (idx, row, was_collapsed) in m.replacements {
            let item = build_item(
                &row,
                was_collapsed,
                &self.avatars,
                &self.media,
                self.user_jid.as_ref().map(|x| x.raw()),
                &chat_ctx,
                &mut |jid| avatar_fetches.push(jid),
            );
            let mut guard = self.messages.guard();
            guard.remove(idx);
            guard.insert(idx, item);
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
        self.bottomed.set(true);
        {
            let mut guard = self.messages.guard();
            guard.push_back(local_item);
        }

        let _ = sender.output(ChatTabOutput::Send {
            chat_id: self.chat_id.clone(),
            text: trimmed.to_string(),
        });
        self.composer_buffer.set_text("");
    }

    /// Synthesise a bubble with a sentinel id for the optimistic echo.
    /// When the worker echoes the real row back, the matching local
    /// entry is dropped so the real one slots in at the same position.
    fn build_optimistic_echo(&self, trimmed: &str) -> MessageItem {
        let local_id = format!(
            "local-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        );
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or_default();
        // Collapse against the trailing message just like a real row
        // would. Read the cursor from the factory's tail so a stale
        // `last_sender` can't misgroup this echo under the recipient.
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
            // MessageItem still keys on raw JID strings (the DB
            // schema does too); converting that down to typed
            // identifiers is a separate refactor.
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
            content: trimmed.to_string(),
            message_type: "text".to_string(),
            timestamp: crate::time::format_message_time(now_unix),
            timestamp_unix: now_unix,
            media_summary: String::new(),
            media_mimetype: None,
            media_size_bytes: None,
            media_duration_secs: None,
            media_path: None,
            media_status: "none".to_string(),
            media_filename: None,
            thumbnail: None,
            quoted_message_id: None,
            quoted_sender_id: None,
            quoted_sender_name: None,
            quoted_preview: None,
            mentions: Vec::new(),
        }
    }
}
