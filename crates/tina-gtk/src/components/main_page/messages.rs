// Init / Input / Output for the in-app page.

use tina_core::WaIdentity;
use tina_db::{ChatRow, MentionCandidate, MessageRow, StatusAuthorRow};

use crate::app::ConnectionStatus;
use crate::components::chat_area::ChatAreaOutput;
use crate::components::sidebar::SidebarOutput;
use crate::inventory::{
    AvatarInventory, ChatInventory, MediaInventory, MentionInventory, MessageInventory,
};
use crate::service::ServiceHandle;

pub struct MainInit {
    pub service: ServiceHandle,
    pub avatars: AvatarInventory,
    pub media: MediaInventory,
    pub chats: ChatInventory,
    pub messages: MessageInventory,
    pub mentions: MentionInventory,
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
    /// Worker delivered the recent-stickers catalog. Forwarded to
    /// the matching tab's picker.
    StickersLoaded {
        chat_id: String,
        items: Vec<(String, String)>,
    },
    /// Receipt update broadcast — fan out to every open tab so the
    /// matching from_me row updates its status icon.
    ReceiptUpdate {
        message_ids: Vec<String>,
        status: String,
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
    NewerMessagesLoaded {
        chat_id: String,
        messages: Vec<MessageRow>,
        reached_bottom: bool,
    },
    AvatarReady {
        jid: WaIdentity,
        path: String,
    },
    AvatarFailed(WaIdentity),
    AvatarTextureReady(String),
    /// Worker resolved the `@`-mention picker for `chat_id`. Mirrored
    /// into the inventory + forwarded down to the matching tab.
    MentionCandidatesLoaded {
        chat_id: String,
        candidates: Vec<MentionCandidate>,
    },
    /// Forwarded from children.
    FromSidebar(SidebarOutput),
    FromChatArea(ChatAreaOutput),
}

#[derive(Debug)]
pub enum MainOutput {
    OpenChatNew(String),
    CloseChat(String),
    SendText {
        chat_id: String,
        text: String,
        mentioned_jids: Vec<String>,
    },
    SendMedia {
        chat_id: String,
        kind: tina_core::MediaKind,
        path: String,
        caption: Option<String>,
        mimetype: Option<String>,
        filename: Option<String>,
        local_id: Option<String>,
    },
    RequestPreferences,
    RequestLogout,
    RequestLoadStatuses,
    OpenStatusAuthor {
        sender_jid: WaIdentity,
        name: String,
    },
    RequestMediaDownload(String),
    RequestLoadOlder { chat_id: String, before_ts: i64 },
    RequestLoadNewer { chat_id: String, after_ts: i64 },
    RequestFetchAvatar(WaIdentity),
    RequestFetchAvatarFromURL(WaIdentity, String),
    SetChatPinned { chat_id: String, pinned: bool },
    /// Sticker-picker popover wants the recent-stickers catalog.
    RequestStickers { chat_id: String },
    /// Tab observed unread incoming rows while bottomed.
    RequestMarkRead {
        chat_id: String,
        sender_jid: String,
        message_ids: Vec<String>,
    },
}
