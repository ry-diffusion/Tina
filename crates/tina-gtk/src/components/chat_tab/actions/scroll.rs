// Scroll-position handlers: stick-to-bottom on tab focus, lazy-load on
// near-top, and the soft-cap prune triggered when the user is parked
// at the bottom and the factory grew past `MAX_KEEP`.

use adw::prelude::*;
use gtk::glib;
use relm4::ComponentSender;
use tina_db::MessageRow;

use super::super::build::{build_item, collapse_against};
use super::super::messages::ChatTabOutput;
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
        let count = self.messages.len();
        if count <= MAX_KEEP {
            return;
        }
        let to_drop = count - TARGET;
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
        // New oldest = first remaining item's timestamp.
        self.oldest_ts = self
            .messages
            .guard()
            .iter()
            .next()
            .map(|f| f.item.timestamp_unix);
        // We dropped real history — older pages are once again
        // legitimately available to fetch.
        self.reached_top = false;
        tracing::info!(
            chat = %self.chat_id,
            dropped = to_drop,
            remaining = self.messages.len(),
            "ChatTab: pruned top after near-bottom"
        );
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
        {
            let mut guard = self.messages.guard();
            let mut sender_cursor: Option<String> = None;
            let mut ts_cursor: Option<i64> = None;
            let collapsed_flags: Vec<bool> = messages
                .iter()
                .map(|r| collapse_against(r, &mut sender_cursor, &mut ts_cursor))
                .collect();
            let mut avatar_fetches: Vec<String> = Vec::new();
            for (row, collapsed) in messages.iter().zip(&collapsed_flags).rev() {
                if !self.seen_message_ids.insert(row.message_id.clone()) {
                    continue;
                }
                let item = build_item(
                    row,
                    *collapsed,
                    &self.avatars,
                    &self.media,
                    self.user_jid.as_deref(),
                    &chat_ctx,
                    &mut |jid| avatar_fetches.push(jid),
                );
                guard.push_front(item);
            }
            drop(guard);
            for jid in avatar_fetches {
                let _ = sender.output(ChatTabOutput::RequestFetchAvatar(jid));
            }
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
    }
}
