// One open chat — header strip with the contact's name, scrollable
// thread of message bubbles, and a single-line composer.

mod actions;
mod build;
mod component;
mod messages;
mod model;
mod scroll;

pub use messages::{ChatTabInit, ChatTabInput, ChatTabOutput};
pub use model::ChatTab;
