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
    /// Full-screen "Repairing…" overlay shown while a Reconcile is in
    /// progress. Tracks the previous scene so we can return to it on
    /// `RepairEnded` (always `InApp` in practice but kept generic).
    Repairing,
    Error,
}

/// Connection state for the sidebar headerbar subtitle. `Connecting`
/// is the boot/reconnect state — distinct from `Offline`, which is
/// reserved for an explicit "we've given up" signal (no current event
/// path sets it; left in the enum so future logout/network monitoring
/// can use it without rewiring callers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    Connecting,
    Connected,
    #[allow(dead_code)]
    Offline,
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
    HistorySyncProgress {
        sync_type: String,
        progress: u32,
    },
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
