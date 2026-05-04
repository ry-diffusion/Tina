// State + read/write helpers for the sidebar.

use std::cell::RefCell;
use std::rc::Rc;

use relm4::Controller;
use relm4::typed_view::list::TypedListView;
use tina_db::ChatRow;

use crate::app::ConnectionStatus;
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
    /// Worker-reported link state. Defaults to `Connecting` so a
    /// freshly launched UI shows that state before the first
    /// `Connected`/`Disconnected` event lands.
    pub(super) connection: ConnectionStatus,
    pub(super) user_jid: Option<String>,
    pub(super) avatars: AvatarInventory,
}

impl Sidebar {
    /// Subtitle for the headerbar `WindowTitle`. Empty when we're
    /// online and idle so the title sits alone.
    pub(super) fn status_subtitle(&self) -> String {
        match self.connection {
            ConnectionStatus::Offline => return "Offline".to_string(),
            ConnectionStatus::Connecting => return "Connecting…".to_string(),
            ConnectionStatus::Connected => {}
        }
        if self.repairing {
            if self.repair_indeterminate || self.repair_total <= 0 {
                return "Syncing…".to_string();
            }
            let pct = ((self.repair_current as f64) / (self.repair_total as f64) * 100.0)
                .clamp(0.0, 100.0)
                .round() as i64;
            return format!("Syncing ({pct}%)");
        }
        String::new()
    }

    /// Whether the indeterminate top progress bar should pulse (no
    /// known fraction). True for connecting + indeterminate repair.
    pub(super) fn status_bar_pulsing(&self) -> bool {
        matches!(self.connection, ConnectionStatus::Connecting)
            || (self.repairing && (self.repair_indeterminate || self.repair_total <= 0))
    }

    /// Whether the thin top progress bar should be shown. Visible
    /// while we're actively reaching the network (connecting) or
    /// reconciling — not for `Offline`, which the subtitle alone
    /// communicates.
    pub(super) fn status_bar_visible(&self) -> bool {
        self.repairing || matches!(self.connection, ConnectionStatus::Connecting)
    }

    /// Determinate fraction for the top progress bar; falls back to
    /// pulsing when we don't have a known total.
    pub(super) fn status_bar_fraction(&self) -> Option<f64> {
        if self.repairing && !self.repair_indeterminate && self.repair_total > 0 {
            Some((self.repair_current as f64) / (self.repair_total as f64))
        } else {
            None
        }
    }

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
            let mut item = ChatRowItem::from_row(row, self.avatars.clone());
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
