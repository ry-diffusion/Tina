// Init / Input / Output messages for the `Sidebar` component.

use tina_core::WaIdentity;
use tina_db::{ChatRow, StatusAuthorRow};

use crate::app::ConnectionStatus;
use crate::components::profile_menu::ProfileMenuOutput;
use crate::inventory::{AvatarInventory, ChatInventory};

/// Active sidebar filter — mirrors WhatsApp's tab strip (All / Groups /
/// Channels / Status). Each variant scopes the visible chat list to a
/// disjoint subset; Newsletter and Status used to share the "All" view
/// with regular chats and crowded the list, so they live in their own
/// tabs now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatFilter {
    /// DMs + groups + newsletters (everything that can host a real
    /// conversation). Hides `status@broadcast` because it's a
    /// pseudo-chat aggregating other people's status posts, not a
    /// thread the user reads top-to-bottom.
    All,
    /// `kind == "group"`.
    Groups,
    /// `kind == "newsletter"` — WhatsApp Channels in the official app.
    Channels,
    /// `kind == "status"` — the `status@broadcast` row.
    Status,
}

impl ChatFilter {
    /// Test for "should this row be visible under the current tab?".
    /// Search-text filtering layers on top in the closure that owns
    /// the search query.
    pub fn matches(self, kind: &str) -> bool {
        match self {
            ChatFilter::All => kind != "status" && kind != "broadcast",
            ChatFilter::Groups => kind == "group",
            ChatFilter::Channels => kind == "newsletter",
            ChatFilter::Status => kind == "status",
        }
    }
}

pub struct SidebarInit {
    pub avatars: AvatarInventory,
    pub chats: ChatInventory,
}

#[derive(Debug)]
pub enum SidebarInput {
    SetIdentity {
        phone: Option<String>,
        jid: Option<WaIdentity>,
        push_name: Option<String>,
    },
    ChatsUpserted(Vec<ChatRow>),
    StatusAuthorsUpserted(Vec<StatusAuthorRow>),
    SearchChanged(String),
    SetRepairing(bool),
    /// Worker reported a connection-state transition; drives the
    /// `Tina` headerbar subtitle ("", "Connecting…", "Offline").
    SetConnection(ConnectionStatus),
    /// Live history-sync progress from `WorkerEvent::HistorySyncProgress`.
    /// Sent on every chunk so the headerbar subtitle reflects the
    /// active stream when the user is already in-app.
    HistorySyncProgress { sync_type: String, progress: u32 },
    /// Sync stream wrapped up (or got pre-empted by `HistorySyncDone`).
    /// Clears the headerbar progress affordance.
    HistorySyncEnded,
    /// User clicked one of the All / Groups / Channels / Status tabs.
    SetChatFilter(ChatFilter),
    RepairProgress {
        stage: String,
        current: i64,
        total: i64,
        indeterminate: bool,
    },
    AvatarReady {
        jid: WaIdentity,
        path: String,
    },
    /// Avatar fetch failed (network error, no avatar set, etc.).
    /// Decrements the in-flight counter so the fetch queue can
    /// drain the next pending request.
    AvatarFailed(WaIdentity),
    /// Local glycin decode of an avatar file landed. Refresh any
    /// row whose `avatar_path` matches `path` so the texture takes
    /// effect on screen.
    AvatarTextureReady(String),
    /// ListView's `activate` signal fired with the row position (in the
    /// post-filter, post-sort visible model).
    RowActivated(u32),
    /// Status-list activation — opens the stories viewer for the
    /// author at this row.
    StatusAuthorActivated(u32),
    /// Right-click context menu picked "Open".
    OpenChatRequested(String),
    /// Right-click context menu picked "Open in new tab".
    OpenInNewTabRequested(String),
    /// Right-click context menu picked "Pin" or "Unpin".
    PinChatRequested {
        chat_id: String,
        pinned: bool,
    },
    /// The set of chat_ids currently open as tabs in the chat area.
    /// Drives the "active" highlight + sort-to-top behaviour.
    SetActiveChats(Vec<String>),
    /// Forwarded from the profile menu child.
    FromProfile(ProfileMenuOutput),
}

#[derive(Debug)]
pub enum SidebarOutput {
    OpenInCurrent(String),
    OpenInNewTab(String),
    RequestPreferences,
    RequestLogout,
    /// User opened the Status tab — ask the worker to recompute the
    /// status authors list.
    RequestLoadStatuses,
    /// User activated a status author — open the stories viewer for
    /// that contact's posts.
    OpenStatusAuthor {
        sender_jid: WaIdentity,
        name: String,
    },
    RequestFetchAvatar(WaIdentity),
    RequestFetchAvatarFromURL(WaIdentity, String),
    SetChatPinned {
        chat_id: String,
        pinned: bool,
    },
}
