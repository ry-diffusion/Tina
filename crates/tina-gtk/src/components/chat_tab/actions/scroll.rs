// Scroll-position handlers: stick-to-bottom on tab focus, lazy-load on
// near-top, and the soft-cap prune triggered when the user is parked
// at the bottom and the factory grew past `MAX_KEEP`.

use adw::prelude::*;
use gtk::glib;
use relm4::ComponentSender;
use tina_db::MessageRow;

use super::super::build::{build_item, collapse_against, day_flips};
use super::super::messages::{ChatTabInput, ChatTabOutput};
use super::super::model::ChatTab;
use super::super::scroll::force_to_bottom;

impl ChatTab {
    pub(in crate::components::chat_tab) fn handle_stick_to_bottom(&mut self) {
        self.bottomed.set(true);
        if let Some(scroll) = self.scroll.clone() {
            force_to_bottom(&scroll);
        }
    }

    pub(in crate::components::chat_tab) fn handle_near_bottom(&mut self) {
        // Soft cap: when the user is parked at the bottom and the
        // factory holds more than `MAX_KEEP` rows, drop the oldest down
        // to `TARGET`. Re-opens the scroll-up path (clears
        // `reached_top` because there's now older history we don't
        // have in memory).
        const MAX_KEEP: usize = 150;
        const TARGET: usize = 100;
        let count = self.list.len() as usize;
        if count <= MAX_KEEP {
            return;
        }
        let to_drop = count - TARGET;
        let mut dropped_ids: Vec<String> = Vec::with_capacity(to_drop);
        for i in 0..to_drop {
            if let Some(h) = self.list.get(i as u32) {
                dropped_ids.push(h.borrow().item.id.clone());
            }
        }
        for _ in 0..to_drop {
            self.list.remove(0);
        }
        for id in &dropped_ids {
            self.seen_message_ids.remove(id);
        }
        self.ui_state.forget(&dropped_ids);
        // New oldest = first remaining item's timestamp.
        self.oldest_ts = self.list_front().map(|r| r.item.timestamp_unix);
        // We dropped real history — older pages are once again
        // legitimately available to fetch.
        self.reached_top = false;
        tracing::info!(
            chat = %self.chat_id,
            dropped = to_drop,
            remaining = self.list.len(),
            "ChatTab: pruned top after near-bottom"
        );
    }

    /// Symmetric counterpart to `handle_near_bottom`: posted by
    /// `handle_prepend_older` after a fast scroll-up has pushed the
    /// factory past the soft cap. Drops the newest rows so paging back
    /// through history doesn't grow the GTK widget tree without bound
    /// (a group chat with media stacks dozens of pages × 50 rows × the
    /// per-bubble widgets into hundreds of MB very quickly).
    ///
    /// The user's viewport is in the upper portion of the factory at
    /// the moment this fires (NearTop only triggers below ~2*page), so
    /// dropping from the back doesn't shift their view — only `upper`
    /// shrinks; `value` stays put.
    ///
    /// Recovering the trimmed tail requires reopening the chat (the
    /// existing `OpenChat` path repaints from the latest 50). Live
    /// pushes via `MessagesAppended` still land — `seen_message_ids`
    /// gets the dropped IDs cleared so a re-emit of the same row isn't
    /// dedup'd away.
    pub(in crate::components::chat_tab) fn handle_trim_bottom(&mut self) {
        const MAX_KEEP: usize = 150;
        const TARGET: usize = 100;
        if self.bottomed.get() {
            return;
        }
        let count = self.list.len() as usize;
        if count <= MAX_KEEP {
            return;
        }
        let to_drop = count - TARGET;
        let mut dropped_ids: Vec<String> = Vec::with_capacity(to_drop);
        let start = count - to_drop;
        for i in start..count {
            if let Some(h) = self.list.get(i as u32) {
                dropped_ids.push(h.borrow().item.id.clone());
            }
        }
        // Each `remove(last_index)` shrinks the list; the last index
        // moves with it. Easier: remove the same final position N
        // times.
        for _ in 0..to_drop {
            let last = self.list.len() - 1;
            self.list.remove(last);
        }
        for id in &dropped_ids {
            self.seen_message_ids.remove(id);
        }
        self.ui_state.forget(&dropped_ids);
        // Newest = first remaining trailing item's ts, OR clear if
        // the list ended up empty. Either way, the rows we just
        // dropped DID exist in the DB, so the tail is no longer the
        // DB tail — clear `reached_bottom` so a future near-bottom
        // scroll re-fetches them.
        self.newest_ts = self.list_back().map(|r| r.item.timestamp_unix);
        self.reached_bottom = false;
        tracing::info!(
            chat = %self.chat_id,
            dropped = to_drop,
            remaining = self.list.len(),
            "ChatTab: pruned bottom after prepend"
        );
    }

    /// User clicked a reply quote-header. Locate the cited row in
    /// the factory window, scroll the viewport so it sits in the
    /// upper third, and add a transient highlight class so the
    /// target is easy to spot. No-op when the message has been
    /// pruned out of the loaded window — paging back to it is a
    /// follow-up.
    pub(in crate::components::chat_tab) fn handle_jump_to_message(
        &mut self,
        target_id: String,
    ) {
        let Some(idx) = self.find_index(&target_id) else {
            tracing::info!(
                target_id,
                "JumpToMessage: target outside loaded window"
            );
            return;
        };
        // ListView's scroll_to handles the realisation + scroll in
        // one call — equivalent of the old listbox.row_at_index +
        // compute_bounds + vadj.set_value dance, but works correctly
        // for offscreen virtualised rows (which haven't been
        // realised yet so they have no bounds to read).
        let view = self.list.view.clone();
        glib::idle_add_local_once(move || {
            view.scroll_to(idx, gtk::ListScrollFlags::FOCUS, None);
        });
        // The transient highlight class lived on the gtk::ListBoxRow.
        // With virtualised ListView rows we don't have a stable
        // widget handle to flash; the FOCUS scroll flag draws the
        // attention-ring on the bound row instead, which serves the
        // same UX purpose without per-bind CSS bookkeeping.
    }

    /// Refresh the row matching `message_id` so the bind pass picks
    /// up the latest `ui_state` (used after the user expands a
    /// video / audio inline). No-op when the row isn't currently in
    /// the loaded window.
    pub(in crate::components::chat_tab) fn handle_rebind_row(&mut self, message_id: &str) {
        if let Some(idx) = self.find_index(message_id) {
            // No item-level mutation needed — the bind pass reads
            // `ui_state` via the inventory cell, which the click
            // handler already updated. We just need to nudge the
            // typed view into re-binding the position. Re-inserting
            // the same value is the cheap way to fire items_changed.
            if let Some(h) = self.list.get(idx) {
                let row = h.borrow().clone();
                self.list.remove(idx);
                self.list.insert(idx, row);
            }
        }
    }

    /// Symmetric of `handle_near_top` but for the descending path.
    /// Fired when the user scrolls into the lower fetch zone and the
    /// factory's newest row isn't the actual DB tail.
    pub(in crate::components::chat_tab) fn handle_near_bottom_fetch(
        &mut self,
        sender: &ComponentSender<Self>,
    ) {
        if self.loading_newer || self.reached_bottom {
            return;
        }
        let Some(after_ts) = self.newest_ts else {
            return;
        };
        self.loading_newer = true;
        tracing::info!(
            chat = %self.chat_id,
            after_ts,
            "ChatTab: requesting newer page",
        );
        let _ = sender.output(ChatTabOutput::RequestLoadNewer {
            chat_id: self.chat_id.clone(),
            after_ts,
        });
    }

    /// Apply a `LoadNewer` response — append the rows at the back of
    /// the factory in chronological order, update `newest_ts`, and
    /// flip `reached_bottom` when the worker returned a short batch.
    /// Mirrors `handle_prepend_older`'s structure but without the
    /// scroll-position lock (appending at the back doesn't shift the
    /// user's view: rows materialise below the visible area, the
    /// `vadj.upper` grows but `value` stays put unless `bottomed`
    /// is set, in which case the connect_changed handler scrolls us
    /// down anyway — which is exactly what the user asked for by
    /// scrolling to the bottom).
    pub(in crate::components::chat_tab) fn handle_append_newer(
        &mut self,
        messages: Vec<MessageRow>,
        reached_bottom: bool,
        sender: &ComponentSender<Self>,
    ) {
        self.loading_newer = false;
        if reached_bottom || messages.len() < 50 {
            self.reached_bottom = true;
        }
        if messages.is_empty() {
            return;
        }
        // Filter out anything we already have (live `MessagesAppended`
        // may have raced ahead of the LoadNewer round trip).
        let new_rows: Vec<_> = messages
            .into_iter()
            .filter(|r| self.seen_message_ids.insert(r.message_id.clone()))
            .collect();
        if new_rows.is_empty() {
            return;
        }
        // Update tail tracking — pick the max ts so a partially-out-
        // of-order batch can't regress the cursor.
        if let Some(t) = new_rows.iter().map(|r| r.timestamp).max() {
            self.newest_ts = Some(match self.newest_ts {
                Some(prev) => prev.max(t),
                None => t,
            });
        }
        let (mut cursor_sender, mut cursor_ts) = self.factory_tail_cursor();
        let mut cursor_day = self.factory_tail_day();
        let chat_ctx = self.chat_context();
        let mut avatar_fetches: Vec<String> = Vec::new();
        for row in &new_rows {
            let collapsed =
                super::super::build::collapse_against(row, &mut cursor_sender, &mut cursor_ts);
            let day_flip = super::super::build::day_flips(row, &mut cursor_day);
            let mut item = super::super::build::build_item(
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
        self.auto_queue_downloads(&new_rows, sender);
        self.maybe_mark_read(&new_rows, sender);
        // The newly-appended rows just grew `vadj.upper`; if the user
        // is `bottomed`, `wire_changed` will scroll-snap to the new
        // tail. Otherwise they hold position.
    }

    pub(in crate::components::chat_tab) fn handle_near_top(
        &mut self,
        sender: &ComponentSender<Self>,
    ) {
        if self.loading_older || self.reached_top {
            return;
        }
        let Some(before_ts) = self.oldest_ts else {
            return;
        };
        self.loading_older = true;
        tracing::info!(
            chat = %self.chat_id,
            before_ts,
            "ChatTab: requesting older page",
        );
        let _ = sender.output(ChatTabOutput::RequestLoadOlder {
            chat_id: self.chat_id.clone(),
            before_ts,
        });
    }

    pub(in crate::components::chat_tab) fn handle_prepend_older(
        &mut self,
        messages: Vec<MessageRow>,
        reached_top: bool,
        sender: &ComponentSender<Self>,
    ) {
        self.loading_older = false;
        if reached_top || messages.len() < 50 {
            self.reached_top = true;
        }
        if messages.is_empty() {
            return;
        }
        // LockScroll / UnlockScroll pattern (gotkit autoscroll):
        // capture (upper, value) before prepend, then after the layout
        // settles set value = old_value + (new_upper - old_upper). User
        // stays on the same content while history grows above. We also
        // turn `bottomed` off explicitly so the connect_changed handler
        // doesn't pull us back to the bottom on the upper notification.
        let saved = self
            .scroll
            .as_ref()
            .map(|s| (s.vadjustment().upper(), s.vadjustment().value()));
        let prev_bottomed = self.bottomed.replace(false);

        let new_oldest = messages.iter().map(|r| r.timestamp).min();
        if let Some(t) = new_oldest {
            self.oldest_ts = Some(match self.oldest_ts {
                Some(prev) => prev.min(t),
                None => t,
            });
        }

        let chat_ctx = self.chat_context();
        // Day-flip flags computed in forward order. The boundary
        // condition that matters here: the LAST row of the prepended
        // batch must compare its day against the first row currently
        // in the factory — if they share a day, the existing head
        // doesn't need its pill any more (we don't mutate it post
        // hoc; this is just to not double-up). In practice the
        // existing head keeps its pill, which is fine — the divider
        // just sits exactly at the day boundary.
        let head_day_after = self.factory_head_day();
        let mut sender_cursor: Option<String> = None;
        let mut ts_cursor: Option<i64> = None;
        let mut day_cursor: Option<String> = None;
        let collapsed_flags: Vec<bool> = messages
            .iter()
            .map(|r| collapse_against(r, &mut sender_cursor, &mut ts_cursor))
            .collect();
        let day_flags: Vec<bool> = messages
            .iter()
            .map(|r| day_flips(r, &mut day_cursor))
            .collect();
        let mut avatar_fetches: Vec<String> = Vec::new();
        for (idx, (row, collapsed)) in messages
            .iter()
            .zip(&collapsed_flags)
            .enumerate()
            .rev()
        {
            if !self.seen_message_ids.insert(row.message_id.clone()) {
                continue;
            }
            let mut item = build_item(
                row,
                *collapsed,
                &self.avatars,
                &self.media,
                &self.mentions,
                self.user_jid.as_ref().map(|x| x.raw()),
                &chat_ctx,
                &mut |jid| avatar_fetches.push(jid),
            );
            // Apply day flip from the forward-order pass — last
            // entry in the batch suppresses its pill if the
            // pre-prepend list head already sat in the same local
            // day (the existing head's pill covers it).
            let mut is_first_of_day = day_flags[idx];
            if idx + 1 == messages.len()
                && let Some(h) = head_day_after.as_deref()
                && crate::time::local_day_key(row.timestamp) == h
            {
                is_first_of_day = false;
            }
            if is_first_of_day {
                item.is_first_of_day = true;
                item.day_label = crate::time::format_day_divider(row.timestamp);
                item.is_collapsed = false;
            }
            let wrapped = self.wrap_row(item);
            self.list.insert(0, wrapped);
        }
        for jid in avatar_fetches {
            let _ = sender.output(ChatTabOutput::RequestFetchAvatar(
                tina_core::WaIdentity::parse(&jid),
            ));
        }

        if let (Some(scroll), Some((old_upper, old_value))) = (self.scroll.clone(), saved) {
            let bottomed_flag = self.bottomed.clone();
            glib::idle_add_local_once(move || {
                let adj = scroll.vadjustment();
                let new_upper = adj.upper();
                let delta = new_upper - old_upper;
                adj.set_value(old_value + delta);
                bottomed_flag.set(prev_bottomed);
            });
        }

        // Soft-cap mirror of NearBottom. Without this, every NearTop
        // → PrependOlder cycle adds 50 rows and never gives any back —
        // the factory grows past whatever the user has patience to
        // scroll through, and group chats run the process out of RAM.
        // The pop is deferred to a follow-up idle so the value-restore
        // above lands first; otherwise the upper-shrink from the trim
        // would fold into the (new_upper - old_upper) delta and drag
        // the user's view up by the trimmed pixels.
        if self.list.len() > 150 {
            let input = sender.input_sender().clone();
            glib::idle_add_local_once(move || {
                let _ = input.send(ChatTabInput::TrimBottom);
            });
        }

        // Older history just scrolled into view — same on-demand
        // policy as Reset/Append.
        self.auto_queue_downloads(&messages, sender);
    }
}
