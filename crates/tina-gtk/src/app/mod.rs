// Root component: scenes (Init / QR / Syncing / InApp / Error) and the
// ServiceWorker + child controllers that back them.

mod component;
mod dispatch;
mod messages;
mod model;
mod pages;

pub use messages::{AppInit, AppMsg, ConnectionStatus};
pub use model::AppModel;
