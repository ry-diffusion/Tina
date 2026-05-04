// State + read/write helpers for the sidebar.

use std::cell::RefCell;
use std::rc::Rc;

use relm4::Controller;
use relm4::typed_view::list::TypedListView;
use tina_db::ChatRow;

use crate::components::chat_row::ChatRowItem;
use crate::components::profile_menu::ProfileMenu;
use crate::inventory::AvatarInventory;

pub struct Sidebar {
    /// Typed wrapper around `gtk::ListView` + `gio::ListStore` +
    /// sort/filter models. We talk to it through the wrapper's typed
    /// API; the unsafe boxing/unboxing of the row data happens inside
    /// the relm4 abstraction.
    pub(super) list: TypedListView<ChatRowItem, gtk::SingleSelection>,
    /// Search query backing the (only) filter we register on `list`.
    /// Mutated on `SearchChanged`; the filter closure reads through it.
    pub(super) search_query: Rc<RefCell<String>>,
    /// Stashed for the scroll-pinning snap. Captured from the view!
    /// macro at init time. While the user is parked at the top, every
    /// `ChatsUpserted` batch nudges the viewport back to 0 so the
    /// SortListModel's reorders don't drift the list to the bottom
    /// over the course of a sync. Once they scroll away, we leave
    /// them where they are.
    pub(super) scroll: Option<gtk::ScrolledWindow>,
    pub(super) profile: Controller<ProfileMenu>,
    pub(super) repairing: bool,
    pub(super) repair_stage: String,
    pub(super) repair_current: i64,
    pub(super) repair_total: i64,
    pub(super) repair_indeterminate: bool,
    pub(super) user_jid: Option<String>,
    pub(super) avatars: AvatarInventory,
}

impl Sidebar {
    /// Linear search through the base store for a chat by id. The list
    /// is small enough (a few hundred chats max in practice) that the
    /// O(n) cost is invisible — and we already iterate the store on
    /// every `ChatsUpserted`, so adding a separate `chat_id → pos`
    /// index would just be more state to keep coherent.
    pub(super) fn find_chat_position(&self, chat_id: &str) -> Option<u32> {
        let total = self.list.len();
        for pos in 0..total {
            if let Some(item) = self.list.get(pos)
                && item.borrow().chat_id == chat_id {
                    return Some(pos);
                }
        }
        None
    }

    /// Replace the item at `pos` with `item`. Triggers `items_changed`
    /// internally → the SortListModel re-evaluates the row's position
    /// and the bound widget rebinds, picking up the new fields.
    pub(super) fn replace_at(&mut self, pos: u32, item: ChatRowItem) {
        self.list.remove(pos);
        self.list.insert(pos, item);
    }

    pub(super) fn apply_chats_upserted(&mut self, rows: Vec<ChatRow>) {
        for row in &rows {
            let mut item = ChatRowItem::from_row(row);
            if let Some(pos) = self.find_chat_position(&item.chat_id) {
                if let Some(prev) = self.list.get(pos) {
                    // `is_active` is owned by the chat area (via
                    // `SetActiveChats`); a fresh `from_row` always
                    // reports `false`, so without this carry-over a
                    // new message landing for an open chat would
                    // silently strip its highlight.
                    item.is_active = prev.borrow().is_active;
                }
                self.replace_at(pos, item);
            } else {
                self.list.append(item);
            }
        }
    }
}
