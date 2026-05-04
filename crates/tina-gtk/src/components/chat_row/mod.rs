// One row in the sidebar's chat list. `ChatRowItem` is the data; it
// implements `relm4::typed_view::RelmListItem` so a
// `TypedListView<ChatRowItem, _>` can host it directly — the relm4
// abstraction owns the GObject boxing, factory plumbing and bind
// lifecycle, so this module no longer needs `unsafe { qdata }` plumbing
// or a hand-rolled `glib::Object` subclass.

mod context_menu;
mod css;
mod item;
mod widgets;

pub use context_menu::install_context_menu_sender;
pub use css::CHAT_ROW_CSS;
pub use item::ChatRowItem;
