// Init / Input / Output for the in-app page.

use tina_core::WaIdentity;
use tina_db::{ChatRow, MessageRow, StatusAuthorRow};

use crate::app::ConnectionStatus;
use crate::components::chat_area::ChatAreaOutput;
use crate::components::sidebar::SidebarOutput;
use crate::inventory::{AvatarInventory, ChatInventory, MediaInventory, MessageInventory};
use crate::service::ServiceHandle;

pub struct MainInit {
    pub service: ServiceHandle,
    pub avatars: AvatarInventory,
    pub media: MediaInventory,
    pub chats: ChatInventory,
    pub messages: MessageInventory,
}

#[derive(Debug)]
pub enum MainInput {
    SetIdentity {
        account_id: String,
        phone: Option<String>,
        jid: Option<WaIdentity>,
        push_name: Option<String>,
    },
    ChatsUpserted(Vec<ChatRow>),
    StatusAuthorsUpserted(Vec<StatusAuthorRow>),
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
    HistorySyncProgress { sync_type: String, progress: u32 },
    HistorySyncEnded,
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
        jid: WaIdentity,
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
    RequestPreferences,
    RequestLogout,
    RequestLoadStatuses,
    OpenStatusAuthor {
        sender_jid: WaIdentity,
        name: String,
    },
    RequestMediaDownload(String),
    RequestLoadOlder { chat_id: String, before_ts: i64 },
    RequestFetchAvatar(WaIdentity),
    SetChatPinned { chat_id: String, pinned: bool },
}
