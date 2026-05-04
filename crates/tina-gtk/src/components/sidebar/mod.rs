// Chat-list sidebar with profile menu, search, virtualised list, and
// repair progress bar.

mod actions;
mod component;
mod messages;
mod model;

pub use messages::{SidebarInit, SidebarInput, SidebarOutput};
pub use model::Sidebar;
