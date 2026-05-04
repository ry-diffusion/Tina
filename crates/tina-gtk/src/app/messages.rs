// Init / Scene / AppMsg for the root component.

use std::path::PathBuf;

use tina_db::{ChatRow, MessageRow};

pub struct AppInit {
    pub nanachi_dir: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    Init,
    QrLogin,
    Syncing,
    InApp,
    Error,
}

#[derive(Debug)]
pub enum AppMsg {
    // From the service worker:
    ShowQrLogin,
    ShowInApp,
    QrCode(String),
    Connected {
        account_id: String,
        phone_number: Option<String>,
        jid: Option<String>,
        push_name: Option<String>,
    },
    Disconnected(String),
    LoggedOut,
    ChatsUpserted(Vec<ChatRow>),
    MessagesAppended {
        chat_id: String,
        messages: Vec<MessageRow>,
    },
    ChatOpened {
        chat_id: Option<String>,
        name: String,
        kind: String,
        messages: Vec<MessageRow>,
    },
    HistorySyncDone,
    RepairStarted,
    RepairProgress {
        stage: String,
        current: i64,
        total: i64,
        indeterminate: bool,
    },
    RepairEnded,
    FatalError(String),
    Toast(String),

    MediaDownloadProgress {
        message_id: String,
        current: i64,
        total: i64,
    },
    MediaReady {
        message_ids: Vec<String>,
        path: String,
        mimetype: Option<String>,
    },
    MediaDownloadFailed {
        message_id: String,
        error: String,
    },
    RequestMediaDownload(String),
    OlderMessagesLoaded {
        chat_id: String,
        messages: Vec<MessageRow>,
        reached_top: bool,
    },
    RequestLoadOlder {
        chat_id: String,
        before_ts: i64,
    },
    AvatarReady {
        jid: String,
        path: String,
    },
    RequestFetchAvatar(String),

    // From the UI:
    OpenChatNew(String),
    CloseChat(String),
    SendText {
        chat_id: String,
        text: String,
    },
    RequestRepair,
    RequestLogout,
    SetChatPinned { chat_id: String, pinned: bool },
}
