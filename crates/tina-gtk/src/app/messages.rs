// Init / Scene / AppMsg for the root component.

use std::path::PathBuf;

use tina_core::WaIdentity;
use tina_db::{ChatRow, MentionCandidate, MessageRow, StatusAuthorRow};

pub struct AppInit {
    pub nanachi_dir: PathBuf,
    pub data_dir: PathBuf,
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
        jid: Option<WaIdentity>,
        push_name: Option<String>,
    },
    Disconnected(String),
    LoggedOut,
    ChatsUpserted { rows: Vec<ChatRow>, messages_written: usize },
    StatusAuthorsUpserted(Vec<StatusAuthorRow>),
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
        messages_count: usize,
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
    /// User pressed "Skip" on the reconnect-sync page. Drops the UI
    /// straight to InApp without waiting for HistorySyncDone.
    SkipSync,

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
    NewerMessagesLoaded {
        chat_id: String,
        messages: Vec<MessageRow>,
        reached_bottom: bool,
    },
    RequestLoadOlder {
        chat_id: String,
        before_ts: i64,
    },
    RequestLoadNewer {
        chat_id: String,
        after_ts: i64,
    },
    AvatarReady {
        jid: WaIdentity,
        path: String,
    },
    AvatarFailed(WaIdentity),
    /// glycin finished decoding an avatar file locally. Broadcast
    /// to UI components so they rebind rows whose `avatar_path`
    /// matches `path`.
    AvatarTextureReady(String),
    RequestFetchAvatar(WaIdentity),
    RequestFetchAvatarFromURL(WaIdentity, String),
    /// Worker propagated a receipt update — flip the matching
    /// outgoing rows' `delivery_status` icon. Multiple ids per
    /// status because whatsmeow batches them.
    ReceiptUpdate {
        message_ids: Vec<String>,
        status: String,
    },

    // From the UI:
    OpenChatNew(String),
    CloseChat(String),
    SendText {
        chat_id: String,
        text: String,
        /// JIDs `@`-picked from the composer popover. Routes through
        /// `Cmd::SendText` → `IpcCommand::SendMessage` so the peer's
        /// client sees the proper `contextInfo.MentionedJID`.
        mentioned_jids: Vec<String>,
    },
    /// User confirmed a media-attach preview. Routes to
    /// `Cmd::SendMedia` and the worker forwards it to nanachi.
    SendMedia {
        chat_id: String,
        kind: tina_core::MediaKind,
        path: String,
        caption: Option<String>,
        mimetype: Option<String>,
        filename: Option<String>,
        local_id: Option<String>,
    },
    RequestRepair,
    RequestPreferences,
    RequestLogout,
    RequestLoadStatuses,
    /// Triggered by `ChatInventory` when it sees a chat without a
    /// resolved display name. Routed to `Cmd::RefreshChat`.
    RequestRefreshChat(WaIdentity),
    OpenStatusAuthor {
        sender_jid: WaIdentity,
        name: String,
    },
    /// Worker pushed back the timeline of one author's status posts;
    /// the dispatcher presents the stories viewer dialog with them.
    ShowStoriesViewer {
        name: String,
        posts: Vec<MessageRow>,
    },
    SetChatPinned { chat_id: String, pinned: bool },

    /// Settings dialog finished applying the user's choice.
    SetDownloadMethod(crate::components::settings::DownloadMethod),
    /// Worker pushed the persisted preferences back to us so the
    /// dialog's combo + memory rows can render real values.
    PreferencesLoaded {
        method: crate::components::settings::DownloadMethod,
        pid: Option<u32>,
    },
    /// Settings dialog asked us to drop the on-disk media cache.
    ClearMediaCache,
    /// Sticker picker requested its catalog. Carries the active
    /// chat id so the result can be routed back to the right
    /// `ChatTab`.
    RequestStickers { chat_id: String },
    /// ChatTab observed unread incoming messages while the user
    /// is at the bottom of the thread. Routed to `Cmd::MarkChatRead`
    /// → IPC → `whatsmeow.Client.MarkRead`.
    MarkChatRead {
        chat_id: String,
        sender_jid: String,
        message_ids: Vec<String>,
    },
    /// Worker pushed the recently-received stickers up. Forwarded
    /// straight to the matching `ChatTab` so its picker can repaint.
    StickersLoaded {
        chat_id: String,
        items: Vec<(String, String)>,
    },
    /// Settings dialog asked us to drop the on-disk avatar cache.
    ClearAvatarCache,
    /// User picked a language in Preferences. Locale key ("en-US",
    /// "pt-BR", or "" for system). Written to disk; restart required.
    SetLanguage(String),
    /// Worker resolved the mention-picker candidates for `chat_id`.
    /// Forwarded down to the matching `ChatTab` so its `@`-popover
    /// has the live list to filter against.
    MentionCandidatesLoaded {
        chat_id: String,
        candidates: Vec<MentionCandidate>,
    },
}
