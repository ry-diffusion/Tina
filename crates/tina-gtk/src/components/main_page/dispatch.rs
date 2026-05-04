// `MainInput` dispatcher: forward to children + bubble user intents
// to the parent. Separated from `component.rs` so the relm4 macro
// stays tight.

use relm4::ComponentSender;
use relm4::prelude::*;

use crate::components::chat_area::{ChatAreaInput, ChatAreaOutput};
use crate::components::sidebar::{SidebarInput, SidebarOutput};

use super::messages::{MainInput, MainOutput};
use super::model::MainPage;

impl MainPage {
    pub(super) fn dispatch(&mut self, msg: MainInput, sender: ComponentSender<Self>) {
        match msg {
            MainInput::SetIdentity {
                phone,
                jid,
                push_name,
                ..
            } => self.handle_set_identity(phone, jid, push_name),
            MainInput::ChatsUpserted(rows) => {
                let _ = self
                    .sidebar
                    .sender()
                    .send(SidebarInput::ChatsUpserted(rows));
            }
            MainInput::ChatOpened {
                chat_id: Some(chat_id),
                name,
                kind,
                messages,
            } => {
                let _ = self.chat_area.sender().send(ChatAreaInput::ChatOpened {
                    chat_id,
                    name,
                    kind,
                    messages,
                });
            }
            MainInput::ChatOpened { chat_id: None, .. } => {
                // Service told us "no chat open" — leave tabs as-is.
            }
            MainInput::MessagesAppended { chat_id, messages } => {
                let _ = self
                    .chat_area
                    .sender()
                    .send(ChatAreaInput::MessagesAppended { chat_id, messages });
            }
            MainInput::SetRepairing(r) => {
                let _ = self.sidebar.sender().send(SidebarInput::SetRepairing(r));
            }
            MainInput::SetConnection(c) => {
                let _ = self.sidebar.sender().send(SidebarInput::SetConnection(c));
            }
            MainInput::HistorySyncProgress {
                sync_type,
                progress,
            } => {
                let _ = self
                    .sidebar
                    .sender()
                    .send(SidebarInput::HistorySyncProgress {
                        sync_type,
                        progress,
                    });
            }
            MainInput::HistorySyncEnded => {
                let _ = self
                    .sidebar
                    .sender()
                    .send(SidebarInput::HistorySyncEnded);
            }
            MainInput::RepairProgress {
                stage,
                current,
                total,
                indeterminate,
            } => {
                let _ = self.sidebar.sender().send(SidebarInput::RepairProgress {
                    stage,
                    current,
                    total,
                    indeterminate,
                });
            }
            MainInput::MediaReady {
                message_ids,
                path,
                mimetype,
            } => {
                let _ = self.chat_area.sender().send(ChatAreaInput::MediaReady {
                    message_ids,
                    path,
                    mimetype,
                });
            }
            MainInput::MediaFailed { message_id } => {
                let _ = self
                    .chat_area
                    .sender()
                    .send(ChatAreaInput::MediaFailed { message_id });
            }
            MainInput::OlderMessagesLoaded {
                chat_id,
                messages,
                reached_top,
            } => {
                let _ = self
                    .chat_area
                    .sender()
                    .send(ChatAreaInput::OlderMessagesLoaded {
                        chat_id,
                        messages,
                        reached_top,
                    });
            }
            MainInput::AvatarReady { jid, path } => {
                // Both children may care: sidebar (chat list rows + own
                // user avatar in the profile popover), chat_area (header
                // for the focused chat).
                let _ = self.sidebar.sender().send(SidebarInput::AvatarReady {
                    jid: jid.clone(),
                    path: path.clone(),
                });
                let _ = self
                    .chat_area
                    .sender()
                    .send(ChatAreaInput::AvatarReady { jid, path });
            }
            MainInput::FromSidebar(out) => self.handle_sidebar_output(out, &sender),
            MainInput::FromChatArea(out) => self.handle_chat_area_output(out, &sender),
        }
    }

    fn handle_set_identity(
        &mut self,
        phone: Option<String>,
        jid: Option<String>,
        push_name: Option<String>,
    ) {
        let _ = self.sidebar.sender().send(SidebarInput::SetIdentity {
            phone: phone.clone(),
            jid: jid.clone(),
            push_name: push_name.clone(),
        });
        let _ = self
            .chat_area
            .sender()
            .send(ChatAreaInput::SetUserJid(jid));
    }

    fn handle_sidebar_output(&mut self, out: SidebarOutput, sender: &ComponentSender<Self>) {
        match out {
            SidebarOutput::OpenInCurrent(id) => {
                let _ = self
                    .chat_area
                    .sender()
                    .send(ChatAreaInput::OpenInCurrent(id));
                if self.split_view.is_collapsed() {
                    self.split_view.set_show_sidebar(false);
                }
            }
            SidebarOutput::OpenInNewTab(id) => {
                let _ = self.chat_area.sender().send(ChatAreaInput::OpenInNewTab(id));
                if self.split_view.is_collapsed() {
                    self.split_view.set_show_sidebar(false);
                }
            }
            SidebarOutput::RequestPreferences => {
                let _ = sender.output(MainOutput::RequestPreferences);
            }
            SidebarOutput::RequestLogout => {
                let _ = sender.output(MainOutput::RequestLogout);
            }
            SidebarOutput::RequestFetchAvatar(jid) => {
                let _ = sender.output(MainOutput::RequestFetchAvatar(jid));
            }
            SidebarOutput::SetChatPinned { chat_id, pinned } => {
                let _ = sender.output(MainOutput::SetChatPinned { chat_id, pinned });
            }
        }
    }

    fn handle_chat_area_output(&mut self, out: ChatAreaOutput, sender: &ComponentSender<Self>) {
        match out {
            ChatAreaOutput::ToggleSidebar(show) => {
                self.split_view.set_show_sidebar(show);
            }
            ChatAreaOutput::OpenChatNew(id) => {
                let _ = sender.output(MainOutput::OpenChatNew(id));
            }
            ChatAreaOutput::SendText { chat_id, text } => {
                let _ = sender.output(MainOutput::SendText { chat_id, text });
            }
            ChatAreaOutput::CloseChat(id) => {
                let _ = sender.output(MainOutput::CloseChat(id));
            }
            ChatAreaOutput::RequestMediaDownload(id) => {
                let _ = sender.output(MainOutput::RequestMediaDownload(id));
            }
            ChatAreaOutput::RequestLoadOlder { chat_id, before_ts } => {
                let _ = sender.output(MainOutput::RequestLoadOlder { chat_id, before_ts });
            }
            ChatAreaOutput::RequestFetchAvatar(jid) => {
                let _ = sender.output(MainOutput::RequestFetchAvatar(jid));
            }
            ChatAreaOutput::ActiveTabsChanged(ids) => {
                // Sidebar uses this to pin active chats to the top
                // and paint the `tina-tab-open` highlight.
                let _ = self
                    .sidebar
                    .sender()
                    .send(SidebarInput::SetActiveChats(ids));
            }
        }
    }
}
