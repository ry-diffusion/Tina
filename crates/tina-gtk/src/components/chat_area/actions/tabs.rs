// Tab-lifecycle handlers: open / close / select / move / auto-merge.

use relm4::ComponentSender;
use relm4::prelude::*;
use tina_db::MessageRow;

use crate::components::chat_tab::{ChatTab, ChatTabInit, ChatTabInput, ChatTabOutput};

use super::super::messages::{ChatAreaInput, ChatAreaOutput};
use super::super::model::ChatArea;

impl ChatArea {
    pub(in crate::components::chat_area) fn handle_open_in_current(
        &mut self,
        chat_id: String,
        sender: &ComponentSender<Self>,
    ) {
        let pane_idx = self
            .open_tabs
            .get(&chat_id)
            .map(|(_, _, p)| *p)
            .unwrap_or(self.focused_pane);
        if let Some((_, page, _)) = self.open_tabs.get(&chat_id) {
            self.panes[pane_idx].tab_view.set_selected_page(page);
        } else if self.pane_tab_count(self.focused_pane) == 0 {
            let _ = sender.output(ChatAreaOutput::OpenChatNew(chat_id));
        } else {
            if let Some(current) = self.panes[self.focused_pane].tab_view.selected_page() {
                self.panes[self.focused_pane].tab_view.close_page(&current);
            }
            let _ = sender.output(ChatAreaOutput::OpenChatNew(chat_id));
        }
    }

    pub(in crate::components::chat_area) fn handle_open_in_new_tab(
        &mut self,
        chat_id: String,
        sender: &ComponentSender<Self>,
    ) {
        if let Some((_, page, pane_idx)) = self.open_tabs.get(&chat_id) {
            self.panes[*pane_idx].tab_view.set_selected_page(page);
        } else {
            let _ = sender.output(ChatAreaOutput::OpenChatNew(chat_id));
        }
    }

    pub(in crate::components::chat_area) fn handle_chat_opened(
        &mut self,
        chat_id: String,
        name: String,
        kind: String,
        messages: Vec<MessageRow>,
        sender: &ComponentSender<Self>,
    ) {
        self.chat_meta
            .insert(chat_id.clone(), (name.clone(), kind.clone()));
        // Feed the inventory + auto-refresh on miss. The same
        // `WaIdentity` predicates the sidebar uses; channels open
        // with `name == chat_id` until `GetNewsletterInfo` resolves,
        // and the inventory dedupes the request so opening the same
        // channel twice in a session only round-trips once.
        self.chats
            .ingest_row(&chat_id, &kind, &name, self.avatars.get(&chat_id).as_deref());
        let id = crate::wa_id::WaIdentity::parse(&chat_id);
        if id.needs_metadata_refresh()
            && crate::wa_id::WaIdentity::looks_like_unresolved_name(&name)
        {
            self.chats.request_refresh(id.raw());
        }
        if let Some((controller, page, _)) = self.open_tabs.get(&chat_id) {
            let _ = controller.sender().send(ChatTabInput::SetMeta {
                name: name.clone(),
                kind: kind.clone(),
            });
            let _ = controller.sender().send(ChatTabInput::Reset(messages));
            page.set_title(&name);
        } else {
            self.spawn_tab(chat_id.clone(), name.clone(), kind.clone(), messages, sender);
        }
        self.refresh_pane_visibility();
        self.refresh_pane_header(0);
        self.refresh_pane_header(1);
        self.broadcast_active_tabs(sender);
        if self.avatars.get(&chat_id).is_none() && self.avatars.needs_fetch(&chat_id) {
            let _ = sender.output(ChatAreaOutput::RequestFetchAvatar(
                tina_core::WaIdentity::parse(&chat_id),
            ));
        }
    }

    fn spawn_tab(
        &mut self,
        chat_id: String,
        name: String,
        kind: String,
        messages: Vec<MessageRow>,
        sender: &ComponentSender<Self>,
    ) {
        let target_pane = self.focused_pane;
        let controller = ChatTab::builder()
            .launch(ChatTabInit {
                chat_id: chat_id.clone(),
                name: name.clone(),
                kind: kind.clone(),
                initial: messages,
                avatars: self.avatars.clone(),
                media: self.media.clone(),
                user_jid: self.user_jid.clone(),
            })
            .forward(sender.input_sender(), |o| match o {
                ChatTabOutput::Send { chat_id, text } => {
                    ChatAreaInput::SendFromTab { chat_id, text }
                }
                ChatTabOutput::SendMedia {
                    chat_id,
                    kind,
                    path,
                    caption,
                    mimetype,
                    filename,
                } => ChatAreaInput::SendMediaFromTab {
                    chat_id,
                    kind,
                    path,
                    caption,
                    mimetype,
                    filename,
                },
                ChatTabOutput::Close { chat_id } => {
                    ChatAreaInput::TabClosed { pane: 0, chat_id }
                }
                ChatTabOutput::RequestMediaDownload(id) => {
                    ChatAreaInput::RequestMediaDownload(id)
                }
                ChatTabOutput::RequestLoadOlder { chat_id, before_ts } => {
                    ChatAreaInput::RequestLoadOlder { chat_id, before_ts }
                }
                ChatTabOutput::RequestFetchAvatar(jid) => {
                    ChatAreaInput::RequestFetchAvatar(jid)
                }
                ChatTabOutput::RequestStickers { chat_id } => {
                    ChatAreaInput::RequestStickers { chat_id }
                }
                ChatTabOutput::RequestMarkRead {
                    chat_id,
                    sender_jid,
                    message_ids,
                } => ChatAreaInput::RequestMarkRead {
                    chat_id,
                    sender_jid,
                    message_ids,
                },
            });
        let widget = controller.widget().clone();
        let page = self.panes[target_pane].tab_view.append(&widget);
        page.set_title(&name);
        page.set_keyword(&chat_id);
        self.panes[target_pane].tab_view.set_selected_page(&page);
        self.open_tabs
            .insert(chat_id.clone(), (controller, page, target_pane));
        // Populate the pane's single-mode header state directly. The
        // selected_page_notify callback fires on append() before we set
        // keyword(), so its callback can't recover the chat_id — we
        // set it here instead.
        let pane = &mut self.panes[target_pane];
        pane.current_chat_id = Some(chat_id.clone());
        pane.current_chat_name = name;
        pane.current_chat_kind = kind;
        pane.current_chat_avatar = self.avatars.get(&chat_id);
        self.focused_pane = target_pane;
    }

    pub(in crate::components::chat_area) fn handle_pane_tab_selected(
        &mut self,
        pane: usize,
        chat_id: Option<String>,
    ) {
        self.focused_pane = pane;
        if let Some(id) = &chat_id {
            if let Some((name, kind)) = self.chat_meta.get(id) {
                self.panes[pane].current_chat_name = name.clone();
                self.panes[pane].current_chat_kind = kind.clone();
            }
            self.panes[pane].current_chat_id = Some(id.clone());
            self.panes[pane].current_chat_avatar = self.avatars.get(id);
            if let Some((controller, _, _)) = self.open_tabs.get(id) {
                let _ = controller.sender().send(ChatTabInput::StickToBottom);
            }
        } else if self.pane_tab_count(pane) == 0 {
            self.panes[pane].current_chat_id = None;
            self.panes[pane].current_chat_name.clear();
            self.panes[pane].current_chat_kind.clear();
            self.panes[pane].current_chat_avatar = None;
        }
        self.refresh_pane_header(pane);
    }

    pub(in crate::components::chat_area) fn handle_tab_closed(
        &mut self,
        chat_id: String,
        sender: &ComponentSender<Self>,
    ) {
        if let Some((controller, page, pane_idx)) = self.open_tabs.remove(&chat_id) {
            self.panes[pane_idx].tab_view.close_page_finish(&page, true);
            drop(controller);
        }
        self.chat_meta.remove(&chat_id);
        self.refresh_pane_visibility();
        self.refresh_pane_header(0);
        self.refresh_pane_header(1);
        self.broadcast_active_tabs(sender);
        let _ = sender.output(ChatAreaOutput::CloseChat(chat_id));
    }

    pub(in crate::components::chat_area) fn handle_auto_merge_pane1(&mut self) {
        // Drain pane 1 → pane 0 by transferring every page in the
        // natural order. After this, pane 1 is empty and
        // refresh_pane_visibility will collapse the revealer.
        while self.panes[1].tab_view.n_pages() > 0 {
            let page = self.panes[1].tab_view.nth_page(0);
            let chat_id = page.keyword().map(|s| s.to_string()).unwrap_or_default();
            let dest = self.panes[0].tab_view.n_pages();
            self.panes[1]
                .tab_view
                .transfer_page(&page, &self.panes[0].tab_view, dest);
            if !chat_id.is_empty()
                && let Some(entry) = self.open_tabs.get_mut(&chat_id) {
                    entry.2 = 0;
                }
        }
        self.focused_pane = 0;
        self.refresh_pane_visibility();
        self.refresh_pane_header(0);
        self.refresh_pane_header(1);
    }

    pub(in crate::components::chat_area) fn handle_move_tab_to_other_pane(&mut self, from: usize) {
        let to = 1 - from;
        let Some(page) = self.panes[from].tab_view.selected_page() else {
            return;
        };
        let Some(chat_id) = page.keyword().map(|s| s.to_string()) else {
            return;
        };
        let pos = self.panes[to].tab_view.n_pages();
        self.panes[from]
            .tab_view
            .transfer_page(&page, &self.panes[to].tab_view, pos);
        if let Some(entry) = self.open_tabs.get_mut(&chat_id) {
            entry.2 = to;
        }
        self.focused_pane = to;
        self.refresh_pane_visibility();
        self.refresh_pane_header(0);
        self.refresh_pane_header(1);
    }
}
