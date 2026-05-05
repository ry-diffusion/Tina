// Per-input handlers for the sidebar.

use std::collections::HashSet;

use adw::prelude::*;
use relm4::ComponentSender;
use relm4::prelude::*;
use tina_db::ChatRow;

use crate::app::ConnectionStatus;
use crate::components::chat_row::ChatRowItem;
use crate::components::profile_menu::{ProfileMenuInput, ProfileMenuOutput};

use super::messages::{ChatFilter, SidebarInput, SidebarOutput};
use super::model::Sidebar;
use super::status_row::StatusAuthorItem;

impl Sidebar {
    pub(super) fn handle_set_identity(
        &mut self,
        phone: Option<String>,
        jid: Option<tina_core::WaIdentity>,
        push_name: Option<String>,
        sender: &ComponentSender<Self>,
    ) {
        self.user_jid = jid.clone();
        if let Some(j) = jid.as_ref() {
            let raw = j.raw();
            if !raw.is_empty() {
                if let Some(p) = self.avatars.get(raw) {
                    let _ = self.profile.sender().send(ProfileMenuInput::SetAvatar(p));
                } else if self.avatars.needs_fetch(raw) {
                    let _ = sender.output(SidebarOutput::RequestFetchAvatar(j.clone()));
                }
            }
        }
        let _ = self.profile.sender().send(ProfileMenuInput::SetIdentity {
            phone,
            jid: jid.as_ref().map(|j| j.raw().to_string()),
            push_name,
        });
    }

    pub(super) fn handle_chats_upserted(
        &mut self,
        mut rows: Vec<ChatRow>,
        sender: &ComponentSender<Self>,
    ) {
        for r in &mut rows {
            if r.avatar_path.is_none()
                && let Some(p) = self.avatars.get(&r.chat_id) {
                    r.avatar_path = Some(p);
                }
            if r.avatar_path.is_none() {
                if let Some(url) = &r.avatar_url {
                    if self.avatars.needs_url_fetch(&r.chat_id) {
                        // URL-based fetches bypass the queue — they go
                        // to a CDN and are typically fast.
                        let _ = sender.output(SidebarOutput::RequestFetchAvatarFromURL(
                            tina_core::WaIdentity::parse(&r.chat_id),
                            url.clone(),
                        ));
                    }
                } else if self.avatars.needs_fetch(&r.chat_id) {
                    // JID-based IPC calls are slow (~440ms each). Push
                    // to the throttle queue; drain_avatar_queue() below
                    // will emit up to CAP at a time.
                    self.pending_avatar_fetches
                        .push_back(tina_core::WaIdentity::parse(&r.chat_id));
                }
            }
        }
        self.drain_avatar_queue(sender);
        // Snapshot whether the user is parked at the top BEFORE
        // applying the batch — once items_changed fires the
        // SortListModel can reorder rows and drift the value
        // arbitrarily, so the post-apply value is no longer a
        // reliable signal of user intent.
        let was_at_top = self
            .scroll
            .as_ref()
            .map(|s| s.vadjustment().value() < 4.0)
            .unwrap_or(true);
        self.apply_chats_upserted(rows);
        if was_at_top
            && let Some(scroll) = self.scroll.clone() {
                // Defer to idle so the layout has settled before we
                // set the value — calling set_value(0) before the
                // upper has been recomputed is a no-op.
                gtk::glib::idle_add_local_once(move || {
                    scroll.vadjustment().set_value(0.0);
                });
            }
    }

    pub(super) fn handle_search_changed(&mut self, text: String) {
        *self.search_query.borrow_mut() = text.to_lowercase();
        self.list.notify_filter_changed(0);
    }

    pub(super) fn handle_set_connection(&mut self, c: ConnectionStatus) {
        self.connection = c;
    }

    pub(super) fn handle_history_sync_progress(&mut self, sync_type: String, progress: u32) {
        self.history_sync_type = sync_type;
        self.history_sync_progress = Some(progress.min(100));
    }

    pub(super) fn handle_history_sync_ended(&mut self) {
        self.history_sync_progress = None;
        self.history_sync_type.clear();
    }

    pub(super) fn handle_set_chat_filter(
        &mut self,
        f: ChatFilter,
        sender: &ComponentSender<Self>,
    ) {
        self.chat_filter.set(f);
        // The Cell mutation is invisible to the SortListModel until
        // we explicitly re-run the filter pass. Otherwise the tab
        // click would only take effect after the next ChatsUpserted
        // reordering.
        self.list.notify_filter_changed(0);
        // The Status tab pulls a separate aggregate (one row per
        // contact who's posted) — refresh it lazily when the user
        // first switches in. The chats list is fed by the regular
        // ChatsUpserted stream so it doesn't need a refresh here.
        if matches!(f, ChatFilter::Status) {
            let _ = sender.output(SidebarOutput::RequestLoadStatuses);
        }
    }

    pub(super) fn handle_status_activated(
        &mut self,
        pos: u32,
        sender: &ComponentSender<Self>,
    ) {
        // `get_visible` returns the filtered+sorted item; no filter
        // is registered on the status list so it's equivalent to
        // `get(pos)`, but staying consistent with the chat-row
        // pattern keeps the surprise low.
        let Some(item) = self.status_list.get_visible(pos) else {
            tracing::warn!(pos, "status row activated but no item at position");
            return;
        };
        let item = item.borrow();
        tracing::info!(
            sender_jid = %item.sender_jid,
            name = %item.name,
            post_count = item.post_count,
            "status author activated",
        );
        let _ = sender.output(SidebarOutput::OpenStatusAuthor {
            sender_jid: tina_core::WaIdentity::parse(&item.sender_jid),
            name: item.name.clone(),
        });
    }

    pub(super) fn handle_status_authors_upserted(
        &mut self,
        rows: Vec<tina_db::StatusAuthorRow>,
    ) {
        // Replace the whole list — these are aggregated and we
        // re-fetch on every Status-tab click, so an in-place reapply
        // would just be more code for the same outcome.
        let total = self.status_list.len();
        for _ in 0..total {
            self.status_list.remove(0);
        }
        for row in &rows {
            self.status_list
                .append(StatusAuthorItem::from_row(row, self.avatars.clone()));
        }
    }

    pub(super) fn handle_set_repairing(&mut self, r: bool) {
        self.repairing = r;
        if !r {
            self.repair_current = 0;
            self.repair_total = 0;
            self.repair_stage.clear();
        }
        let _ = self.profile.sender().send(ProfileMenuInput::SetRepairing(r));
    }

    pub(super) fn handle_repair_progress(
        &mut self,
        stage: String,
        current: i64,
        total: i64,
        indeterminate: bool,
    ) {
        self.repair_stage = stage;
        self.repair_current = current;
        self.repair_total = total;
        self.repair_indeterminate = indeterminate;
    }

    pub(super) fn handle_avatar_ready(
        &mut self,
        jid: tina_core::WaIdentity,
        path: String,
        sender: &ComponentSender<Self>,
    ) {
        self.in_flight_avatar_count = self.in_flight_avatar_count.saturating_sub(1);
        let raw = jid.raw().to_string();
        self.avatars.put(raw.clone(), path.clone());
        if let Some(pos) = self.find_chat_position(&raw) {
            let prev = self.list.get(pos).map(|i| i.borrow().clone());
            if let Some(mut prev) = prev {
                prev.avatar_path = Some(path.clone());
                self.replace_at(pos, prev);
            }
        }
        if self.user_jid.as_ref().map(|x| x.raw()) == Some(raw.as_str()) {
            let _ = self.profile.sender().send(ProfileMenuInput::SetAvatar(path));
        }
        self.drain_avatar_queue(sender);
    }

    pub(super) fn handle_avatar_failed(
        &mut self,
        jid: tina_core::WaIdentity,
        sender: &ComponentSender<Self>,
    ) {
        self.avatars.mark_failed(jid.raw());
        self.in_flight_avatar_count = self.in_flight_avatar_count.saturating_sub(1);
        self.drain_avatar_queue(sender);
    }

    /// glycin's async decode of a local avatar file landed in the
    /// `AvatarInventory` cache. Walk the list, find rows whose
    /// `avatar_path` matches, force a rebind so the cached texture
    /// gets pulled from the inventory.
    pub(super) fn handle_avatar_texture_ready(&mut self, path: &str) {
        let total = self.list.len();
        let mut matches: Vec<u32> = Vec::new();
        for pos in 0..total {
            if let Some(item) = self.list.get(pos)
                && item.borrow().avatar_path.as_deref() == Some(path)
            {
                matches.push(pos);
            }
        }
        for pos in matches {
            if let Some(item) = self.list.get(pos) {
                // Re-insert at the same index; the typed view fires
                // an items_changed for that row, which retriggers
                // bind. The bind path then hits the now-populated
                // texture cache.
                let row = item.borrow().clone();
                self.replace_at(pos, row);
            }
        }
    }

    pub(super) fn handle_row_activated(&mut self, pos: u32, sender: &ComponentSender<Self>) {
        if let Some(item) = self.list.get_visible(pos) {
            let id = item.borrow().chat_id.clone();
            let _ = sender.output(SidebarOutput::OpenInCurrent(id));
        }
    }

    pub(super) fn handle_set_active_chats(&mut self, ids: Vec<String>) {
        let new_active: HashSet<String> = ids.into_iter().collect();
        let total = self.list.len();
        for pos in 0..total {
            let updated = self.list.get(pos).and_then(|item| {
                let cur = item.borrow().clone();
                let now_active = new_active.contains(&cur.chat_id);
                if cur.is_active != now_active {
                    let mut next: ChatRowItem = cur.clone();
                    next.is_active = now_active;
                    Some(next)
                } else {
                    None
                }
            });
            if let Some(next) = updated {
                self.replace_at(pos, next);
            }
        }
    }

    pub(super) fn handle_from_profile(
        &mut self,
        out: ProfileMenuOutput,
        sender: &ComponentSender<Self>,
    ) {
        match out {
            ProfileMenuOutput::Preferences => {
                let _ = sender.output(SidebarOutput::RequestPreferences);
            }
            ProfileMenuOutput::Logout => {
                let _ = sender.output(SidebarOutput::RequestLogout);
            }
        }
    }

    pub(super) fn dispatch(&mut self, msg: SidebarInput, sender: ComponentSender<Self>) {
        match msg {
            SidebarInput::SetIdentity {
                phone,
                jid,
                push_name,
            } => self.handle_set_identity(phone, jid, push_name, &sender),
            SidebarInput::ChatsUpserted(rows) => self.handle_chats_upserted(rows, &sender),
            SidebarInput::SearchChanged(text) => self.handle_search_changed(text),
            SidebarInput::SetRepairing(r) => self.handle_set_repairing(r),
            SidebarInput::SetConnection(c) => self.handle_set_connection(c),
            SidebarInput::HistorySyncProgress {
                sync_type,
                progress,
            } => self.handle_history_sync_progress(sync_type, progress),
            SidebarInput::HistorySyncEnded => self.handle_history_sync_ended(),
            SidebarInput::SetChatFilter(f) => self.handle_set_chat_filter(f, &sender),
            SidebarInput::StatusAuthorsUpserted(rows) => {
                self.handle_status_authors_upserted(rows);
            }
            SidebarInput::StatusAuthorActivated(pos) => {
                self.handle_status_activated(pos, &sender);
            }
            SidebarInput::RepairProgress {
                stage,
                current,
                total,
                indeterminate,
            } => self.handle_repair_progress(stage, current, total, indeterminate),
            SidebarInput::AvatarReady { jid, path } => {
                self.handle_avatar_ready(jid, path, &sender)
            }
            SidebarInput::AvatarFailed(jid) => self.handle_avatar_failed(jid, &sender),
            SidebarInput::AvatarTextureReady(path) => {
                self.handle_avatar_texture_ready(&path)
            }
            SidebarInput::RowActivated(pos) => self.handle_row_activated(pos, &sender),
            SidebarInput::OpenChatRequested(id) => {
                let _ = sender.output(SidebarOutput::OpenInCurrent(id));
            }
            SidebarInput::OpenInNewTabRequested(id) => {
                let _ = sender.output(SidebarOutput::OpenInNewTab(id));
            }
            SidebarInput::PinChatRequested { chat_id, pinned } => {
                let _ = sender.output(SidebarOutput::SetChatPinned { chat_id, pinned });
            }
            SidebarInput::SetActiveChats(ids) => self.handle_set_active_chats(ids),
            SidebarInput::FromProfile(out) => self.handle_from_profile(out, &sender),
        }
    }
}
