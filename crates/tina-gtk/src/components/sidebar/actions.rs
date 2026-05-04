// Per-input handlers for the sidebar.

use std::collections::HashSet;

use adw::prelude::*;
use relm4::ComponentSender;
use relm4::prelude::*;
use tina_db::ChatRow;

use crate::app::ConnectionStatus;
use crate::components::chat_row::ChatRowItem;
use crate::components::profile_menu::{ProfileMenuInput, ProfileMenuOutput};

use super::messages::{SidebarInput, SidebarOutput};
use super::model::Sidebar;

impl Sidebar {
    pub(super) fn handle_set_identity(
        &mut self,
        phone: Option<String>,
        jid: Option<String>,
        push_name: Option<String>,
        sender: &ComponentSender<Self>,
    ) {
        self.user_jid = jid.clone();
        if let Some(j) = jid.as_deref()
            && !j.is_empty() {
                if let Some(p) = self.avatars.get(j) {
                    let _ = self.profile.sender().send(ProfileMenuInput::SetAvatar(p));
                } else if self.avatars.needs_fetch(j) {
                    let _ = sender.output(SidebarOutput::RequestFetchAvatar(j.to_string()));
                }
            }
        let _ = self.profile.sender().send(ProfileMenuInput::SetIdentity {
            phone,
            jid,
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
            if r.avatar_path.is_none() && self.avatars.needs_fetch(&r.chat_id) {
                let _ = sender.output(SidebarOutput::RequestFetchAvatar(r.chat_id.clone()));
            }
        }
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

    pub(super) fn handle_avatar_ready(&mut self, jid: String, path: String) {
        self.avatars.put(jid.clone(), path.clone());
        if let Some(pos) = self.find_chat_position(&jid) {
            let prev = self.list.get(pos).map(|i| i.borrow().clone());
            if let Some(mut prev) = prev {
                prev.avatar_path = Some(path.clone());
                self.replace_at(pos, prev);
            }
        }
        if self.user_jid.as_deref() == Some(jid.as_str()) {
            let _ = self.profile.sender().send(ProfileMenuInput::SetAvatar(path));
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
            ProfileMenuOutput::Repair => {
                let _ = sender.output(SidebarOutput::RequestRepair);
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
            SidebarInput::RepairProgress {
                stage,
                current,
                total,
                indeterminate,
            } => self.handle_repair_progress(stage, current, total, indeterminate),
            SidebarInput::AvatarReady { jid, path } => self.handle_avatar_ready(jid, path),
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
