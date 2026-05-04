// Init / Input / Output messages for the `Sidebar` component.

use tina_db::ChatRow;

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
    RequestRepair,
    RequestLogout,
    RequestFetchAvatar(String),
    SetChatPinned {
        chat_id: String,
        pinned: bool,
    },
}
