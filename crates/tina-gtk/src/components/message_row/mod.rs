// Virtualised chat-timeline row, mirroring `chat_row` for the sidebar.
//
// Why a new module: the existing `message_bubble` is a relm4
// `FactoryComponent` that lives inside a `gtk::ListBox` — every row
// in the deque is a real GTK widget tree, so even with a soft cap
// the realised cost grows linearly. The Fractal-pattern port in this
// module wraps the same data (`MessageItem`) into a `RelmListItem`
// that the typed `gtk::ListView` factory recycles: only widgets for
// rows currently in the viewport are kept around. Off-screen rows
// drop back to "data only" until they scroll into view again.

use relm4::typed_view::list::RelmListItem;

mod bind;
mod item;
mod setup;
mod widgets;

pub use item::{MessageRowItem, RowUiInventory};

impl RelmListItem for MessageRowItem {
    type Root = gtk::Box;
    type Widgets = widgets::MessageRowWidgets;

    fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
        setup::build_root_and_widgets()
    }

    fn bind(&mut self, widgets: &mut Self::Widgets, root: &mut Self::Root) {
        bind::bind(self, widgets, root);
    }

    fn unbind(&mut self, widgets: &mut Self::Widgets, _root: &mut Self::Root) {
        bind::unbind(widgets);
    }
}
