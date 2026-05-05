// Message-row data + helpers shared with the virtualised
// `message_row` factory. The legacy relm4 `FactoryComponent`
// `MessageBubble` is gone — its data type (`MessageItem`), pango
// markup pipeline, lightbox, image loading, and delivery-status
// helpers are still exported here as the canonical home for that
// logic.

mod css;
mod factory;
mod format;
mod item;
pub mod lightbox;

pub use css::MESSAGE_ROW_CSS;
pub use factory::{delivery_css_class, delivery_icon_name};
pub use item::MessageItem;
