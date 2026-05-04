// Init / Input / Output messages for the `Sidebar` component.

use tina_db::ChatRow;

use crate::app::ConnectionStatus;
use crate::components::profile_menu::ProfileMenuOutput;
use crate::inventory::AvatarInventory;

pub struct SidebarInit {
    pub avatars: AvatarInventory,
}

#[derive(Debug)]
pub enum SidebarInput {
    SetIdentity {
        phone: Option<String>,
        jid: Option<String>,
        push_name: Option<String>,
    },
    ChatsUpserted(Vec<ChatRow>),
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
    RepairProgress {
        stage: String,
        current: i64,
        total: i64,
        indeterminate: bool,
    },
    AvatarReady {
        jid: String,
        path: String,
    },
    /// ListView's `activate` signal fired with the row position (in the
    /// post-filter, post-sort visible model).
    RowActivated(u32),
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
    RequestFetchAvatar(String),
    SetChatPinned {
        chat_id: String,
        pinned: bool,
    },
}
