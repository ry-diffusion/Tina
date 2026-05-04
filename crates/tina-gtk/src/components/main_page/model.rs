// In-app page state: thin shell wiring the `Sidebar` (chat list +
// profile + search + repair bar) and the `ChatArea` (multi-tab chat
// surface) onto an `AdwOverlaySplitView` for HIG-canonical responsive
// collapse.

use relm4::Controller;

use crate::components::chat_area::ChatArea;
use crate::components::sidebar::Sidebar;
use crate::service::ServiceHandle;

pub struct MainPage {
    #[allow(dead_code)]
    pub(super) service: ServiceHandle,
    pub(super) sidebar: Controller<Sidebar>,
    pub(super) chat_area: Controller<ChatArea>,
    /// `AdwOverlaySplitView` running the sidebar+content split. We
    /// stash a clone so the toggle button (in the chat area's headerbar)
    /// can flip `show-sidebar`. The split also has a `collapsed`
    /// property driven by an `AdwBreakpoint` for narrow-width adaptive
    /// behaviour (sidebar overlays content instead of pushing it).
    pub(super) split_view: adw::OverlaySplitView,
}
