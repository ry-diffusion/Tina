// Init / Input / Output for the in-app page.

use tina_db::{ChatRow, MessageRow};

use crate::app::ConnectionStatus;
use crate::components::chat_area::ChatAreaOutput;
use crate::components::sidebar::SidebarOutput;
use crate::inventory::{AvatarInventory, MediaInventory};
use crate::service::ServiceHandle;

pub struct MainInit {
    pub service: ServiceHandle,
    pub avatars: AvatarInventory,
    pub media: MediaInventory,
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
    SetConnection(ConnectionStatus),
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
    SetChatPinned { chat_id: String, pinned: bool },
}
