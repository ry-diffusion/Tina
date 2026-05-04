// Sidebar + ChatArea on an AdwOverlaySplitView. All real state lives
// in the children; this module is a thin router.

mod component;
mod dispatch;
mod messages;
mod model;

pub use messages::{MainInit, MainInput, MainOutput};
pub use model::MainPage;
