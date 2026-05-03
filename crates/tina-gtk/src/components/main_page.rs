// In-app page: thin shell wiring the `Sidebar` (chat list + profile +
// search + repair bar) and the `ChatArea` (multi-tab chat surface) onto
// an `AdwOverlaySplitView` for HIG-canonical responsive collapse.
//
// All real state lives in the children. This component only routes worker
// events to whichever child cares (often both, e.g. AvatarReady) and
// forwards user intents back up to `app.rs`.

use relm4::Controller;
use relm4::prelude::*;
use tina_db::{ChatRow, MessageRow};

use crate::components::chat_area::{ChatArea, ChatAreaInput, ChatAreaOutput};
use crate::components::sidebar::{Sidebar, SidebarInput, SidebarOutput};
use crate::service::ServiceHandle;

pub struct MainInit {
    pub service: ServiceHandle,
}

#[derive(Debug)]
pub enum MainInput {
    SetIdentity {
        account_id: String,
        phone: Option<String>,
        jid: Option<String>,
        push_name: Option<String>,
    },
    ChatsUpserted(Vec<ChatRow>),
    ChatOpened {
        chat_id: Option<String>,
        name: String,
        kind: String,
        messages: Vec<MessageRow>,
    },
    MessagesAppended {
        chat_id: String,
        messages: Vec<MessageRow>,
    },
    SetRepairing(bool),
    RepairProgress {
        stage: String,
        current: i64,
        total: i64,
        indeterminate: bool,
    },
    MediaReady {
        message_ids: Vec<String>,
        path: String,
        mimetype: Option<String>,
    },
    MediaFailed {
        message_id: String,
    },
    OlderMessagesLoaded {
        chat_id: String,
        messages: Vec<MessageRow>,
        reached_top: bool,
    },
    AvatarReady {
        jid: String,
        path: String,
    },
    /// Forwarded from children.
    FromSidebar(SidebarOutput),
    FromChatArea(ChatAreaOutput),
}

#[derive(Debug)]
pub enum MainOutput {
    OpenChatNew(String),
    CloseChat(String),
    SendText { chat_id: String, text: String },
    RequestRepair,
    RequestLogout,
    RequestMediaDownload(String),
    RequestLoadOlder { chat_id: String, before_ts: i64 },
    RequestFetchAvatar(String),
}

pub struct MainPage {
    #[allow(dead_code)]
    service: ServiceHandle,
    sidebar: Controller<Sidebar>,
    chat_area: Controller<ChatArea>,
    /// Stashed clone of the `AdwOverlaySplitView` root so we can toggle
    /// `show-sidebar` from within `update` (the toggle lives inside the
    /// chat area headerbar and bubbles up as an output).
    split_view: adw::OverlaySplitView,
}

#[relm4::component(pub)]
impl SimpleComponent for MainPage {
    type Init = MainInit;
    type Input = MainInput;
    type Output = MainOutput;

    view! {
        #[root]
        adw::OverlaySplitView {
            set_min_sidebar_width: 280.0,
            set_max_sidebar_width: 380.0,
            set_sidebar_width_fraction: 0.27,

            #[wrap(Some)]
            set_sidebar = model.sidebar.widget(),

            #[wrap(Some)]
            set_content = model.chat_area.widget(),
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let sidebar = Sidebar::builder()
            .launch(())
            .forward(sender.input_sender(), MainInput::FromSidebar);

        let chat_area = ChatArea::builder()
            .launch(())
            .forward(sender.input_sender(), MainInput::FromChatArea);

        let model = MainPage {
            service: init.service,
            sidebar,
            chat_area,
            split_view: root.clone(),
        };

        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: MainInput, sender: ComponentSender<Self>) {
        match msg {
            MainInput::SetIdentity {
                phone,
                jid,
                push_name,
                ..
            } => {
                let _ = self.sidebar.sender().send(SidebarInput::SetIdentity {
                    phone,
                    jid,
                    push_name,
                });
            }
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
            MainInput::FromSidebar(out) => match out {
                SidebarOutput::OpenInCurrent(id) => {
                    let _ = self
                        .chat_area
                        .sender()
                        .send(ChatAreaInput::OpenInCurrent(id));
                }
                SidebarOutput::OpenInNewTab(id) => {
                    let _ = self.chat_area.sender().send(ChatAreaInput::OpenInNewTab(id));
                }
                SidebarOutput::RequestRepair => {
                    let _ = sender.output(MainOutput::RequestRepair);
                }
                SidebarOutput::RequestLogout => {
                    let _ = sender.output(MainOutput::RequestLogout);
                }
                SidebarOutput::RequestFetchAvatar(jid) => {
                    let _ = sender.output(MainOutput::RequestFetchAvatar(jid));
                }
            },
            MainInput::FromChatArea(out) => match out {
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
            },
        }
    }
}
