// Message bubble: one row in the chat thread, with media handling
// (lightbox + image decoding) and a Dissent-style cozy/collapsed layout.

mod css;
mod factory;
mod format;
mod image;
mod item;
mod lightbox;

pub use css::MESSAGE_ROW_CSS;
pub use factory::{MessageBubble, MessageBubbleInput, MessageBubbleOut};
pub use item::MessageItem;
