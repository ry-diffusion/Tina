// Multi-tab chat surface with optional vertical split. Owns the
// AdwTabViews, the per-pane headerbar machinery, and the routing from
// sidebar clicks → tab open / select / move.

mod actions;
mod component;
mod messages;
mod model;
mod pane;

pub use messages::{ChatAreaInit, ChatAreaInput, ChatAreaOutput};
pub use model::ChatArea;
