// Per-message-input handlers, factored into themed submodules.
//
// Each submodule extends `ChatTab` with `pub(in crate::components::chat_tab)`
// methods. The dispatcher in `dispatch.rs` is what `component.rs::update`
// calls.

mod dispatch;
mod echo;
mod history;
mod identity;
mod media;
mod scroll;
