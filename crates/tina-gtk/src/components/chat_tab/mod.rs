// One open chat — header strip with the contact's name, scrollable
// thread of message bubbles, and a single-line composer.

mod actions;
mod build;
mod clipboard_paste;
mod component;
mod messages;
mod model;
mod preview;
mod record;
mod scroll;

pub use messages::{ChatTabInit, ChatTabInput, ChatTabOutput};
pub use model::ChatTab;
