// Init / Input / Output for the `ChatArea` component.

use tina_core::WaIdentity;
use tina_db::MessageRow;

use crate::inventory::{AvatarInventory, ChatInventory, MediaInventory, MessageInventory};

pub struct ChatAreaInit {
    pub avatars: AvatarInventory,
    pub media: MediaInventory,
    pub chats: ChatInventory,
    pub messages: MessageInventory,
}

#[derive(Debug)]
pub enum ChatAreaInput {
    /// User picked a chat from the sidebar — reuse the focused pane's
    /// selected tab if one's open, else open a fresh tab in that pane.
    OpenInCurrent(String),
    /// User explicitly asked for a new tab (right-click menu).
    OpenInNewTab(String),
    ChatOpened {
        chat_id: String,
        name: String,
        kind: String,
        messages: Vec<MessageRow>,
    },
    MessagesAppended {
        chat_id: String,
        messages: Vec<MessageRow>,
    },
    OlderMessagesLoaded {
        chat_id: String,
        messages: Vec<MessageRow>,
        reached_top: bool,
    },
    MediaReady {
        message_ids: Vec<String>,
        path: String,
        mimetype: Option<String>,
    },
    MediaFailed {
        message_id: String,
    },
    AvatarReady {
        jid: WaIdentity,
        path: String,
    },
    /// A pane's selected tab changed; route StickToBottom + update headerbar.
    PaneTabSelected {
        pane: usize,
        chat_id: Option<String>,
    },
    /// AdwTabView signalled close-page; finalize teardown.
    TabClosed {
        pane: usize,
        chat_id: String,
    },
    /// User pressed the "move to other split" button on pane `from`.
    /// Transfers the pane's currently-selected tab to the opposite pane,
    /// creating the split if it wasn't visible.
    MoveTabToOtherPane(usize),
    /// User clicked into a pane — make it the routing target for new
    /// chats. Selecting a tab inside a pane already does this via
    /// PaneTabSelected, but a click in an empty pane needs its own path.
    PaneFocused(usize),
    /// Adaptive: window narrowed below the split threshold. Move every
    /// pane 1 tab back into pane 0 so they don't end up stranded in a
    /// hidden pane the user can't reach without widening again.
    AutoMergePane1,
    /// Forwarded from a ChatTab.
    SendFromTab {
        chat_id: String,
        text: String,
    },
    /// Forwarded from a ChatTab — user confirmed a media-attach
    /// preview.
    SendMediaFromTab {
        chat_id: String,
        kind: tina_core::MediaKind,
        path: String,
        caption: Option<String>,
        mimetype: Option<String>,
        filename: Option<String>,
    },
    /// Forwarded from a ChatTab.
    RequestMediaDownload(String),
    /// Forwarded from a ChatTab.
    RequestLoadOlder {
        chat_id: String,
        before_ts: i64,
    },
    /// Forwarded from a ChatTab — sender-avatar fetch.
    RequestFetchAvatar(WaIdentity),
    /// Forwarded from a ChatTab — sticker picker requested its
    /// catalog.
    RequestStickers { chat_id: String },
    /// Forwarded from a ChatTab — read receipts for incoming rows.
    RequestMarkRead {
        chat_id: String,
        sender_jid: String,
        message_ids: Vec<String>,
    },
    /// Worker pushed sticker catalog. Routed to the matching tab.
    StickersLoaded {
        chat_id: String,
        items: Vec<(String, String)>,
    },
    /// Receipt update — fanned out to every open tab.
    ReceiptUpdate {
        message_ids: Vec<String>,
        status: String,
    },
    /// Identity arrived (or changed). Stored for new tabs + forwarded
    /// to existing ones so from_me rows pick up the user avatar.
    SetUserJid(Option<WaIdentity>),
}

#[derive(Debug)]
pub enum ChatAreaOutput {
    ToggleSidebar(bool),
    /// Ask the worker to fetch metadata + first page for `chat_id`. Comes
    /// back as `ChatOpened` via the parent.
    OpenChatNew(String),
    SendText {
        chat_id: String,
        text: String,
    },
    SendMedia {
        chat_id: String,
        kind: tina_core::MediaKind,
        path: String,
        caption: Option<String>,
        mimetype: Option<String>,
        filename: Option<String>,
    },
    /// A chat was closed in the UI — parent must tell the worker so it
    /// stops emitting `MessagesAppended` for it.
    CloseChat(String),
    RequestMediaDownload(String),
    RequestLoadOlder {
        chat_id: String,
        before_ts: i64,
    },
    RequestFetchAvatar(WaIdentity),
    /// Forwarded sticker-picker request.
    RequestStickers { chat_id: String },
    /// Forwarded mark-read request — child of `ChatAreaOutput`. The
    /// `ChatAreaInput` carries the same variant for the controller-
    /// to-area hop.
    RequestMarkRead {
        chat_id: String,
        sender_jid: String,
        message_ids: Vec<String>,
    },
    /// The set of chat_ids currently open in tabs (across both panes).
    /// Emitted whenever a tab opens or closes so the sidebar can
    /// highlight + sort-to-top the active chats.
    ActiveTabsChanged(Vec<String>),
}
