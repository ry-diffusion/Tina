// State + read/write helpers for the sidebar.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use relm4::Controller;
use relm4::typed_view::list::TypedListView;
use tina_db::ChatRow;

use crate::app::ConnectionStatus;
use crate::components::chat_row::ChatRowItem;
use crate::components::profile_menu::ProfileMenu;
use crate::inventory::{AvatarInventory, ChatInventory};

use super::messages::ChatFilter;
use super::status_row::StatusAuthorItem;

pub struct Sidebar {
    /// Typed wrapper around `gtk::ListView` + `gio::ListStore` +
    /// sort/filter models. We talk to it through the wrapper's typed
    /// API; the unsafe boxing/unboxing of the row data happens inside
    /// the relm4 abstraction.
    pub(super) list: TypedListView<ChatRowItem, gtk::SingleSelection>,
    /// Second list view, only visible when the Status tab is active.
    /// One row per contact who's posted to `status@broadcast`. Built
    /// from `WorkerEvent::StatusAuthorsUpserted` and refreshed on tab
    /// activation — not part of the regular chat-row stream.
    pub(super) status_list: TypedListView<StatusAuthorItem, gtk::SingleSelection>,
    /// Search query backing the search filter we register on `list`.
    /// Mutated on `SearchChanged`; the filter closure reads through it.
    pub(super) search_query: Rc<RefCell<String>>,
    /// Active tab. Shared with the per-row filter closure via `Rc`
    /// so flipping the tab from a button click only mutates a Cell;
    /// `notify_filter_changed` then re-runs the closure on every row.
    pub(super) chat_filter: Rc<Cell<ChatFilter>>,
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
    /// 0..100 from the latest `WorkerEvent::HistorySyncProgress`.
    /// `None` once the sync stream finishes (or when no sync is
    /// active). Drives the headerbar subtitle when whatsmeow is
    /// streaming a HistorySync while the user is already in-app —
    /// e.g. on auto-reconnect after a network drop.
    pub(super) history_sync_progress: Option<u32>,
    /// Last `HistorySync.SyncType` enum string ("INITIAL_BOOTSTRAP",
    /// "RECENT", …) — used so the subtitle can spell out the stage
    /// instead of an opaque percentage.
    pub(super) history_sync_type: String,
    pub(super) user_jid: Option<String>,
    pub(super) avatars: AvatarInventory,
    pub(super) chats: ChatInventory,
}

impl Sidebar {
    /// Subtitle for the headerbar `WindowTitle`. Empty when we're
    /// online and idle so the title sits alone. Priority order:
    /// connection state > repair > history sync — connection trumps
    /// everything because if we're not online there's no point
    /// reporting download progress on a dead pipe.
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
        if let Some(progress) = self.history_sync_progress {
            // Tag the percentage with the sync type when whatsmeow
            // gave us one. INITIAL_BOOTSTRAP stays generic ("Syncing")
            // because it's the most common path and the verbose label
            // adds noise.
            let label = match self.history_sync_type.as_str() {
                "RECENT" => "Catching up",
                "FULL" => "Pulling history",
                "ON_DEMAND" => "Pulling history",
                _ => "Syncing",
            };
            return if progress == 0 {
                format!("{label}…")
            } else {
                format!("{label} ({progress}%)")
            };
        }
        String::new()
    }

    /// Whether the indeterminate top progress bar should pulse (no
    /// known fraction). True for connecting, indeterminate repair, or
    /// a HistorySync stream still at 0%.
    pub(super) fn status_bar_pulsing(&self) -> bool {
        matches!(self.connection, ConnectionStatus::Connecting)
            || (self.repairing && (self.repair_indeterminate || self.repair_total <= 0))
            || self.history_sync_progress == Some(0)
    }

    /// Whether the thin top progress bar should be shown. Visible
    /// while we're actively reaching the network (connecting),
    /// reconciling, or pulling a HistorySync stream — not for
    /// `Offline`, which the subtitle alone communicates.
    pub(super) fn status_bar_visible(&self) -> bool {
        self.repairing
            || matches!(self.connection, ConnectionStatus::Connecting)
            || self.history_sync_progress.is_some()
    }

    /// Determinate fraction for the top progress bar; falls back to
    /// pulsing when we don't have a known total.
    pub(super) fn status_bar_fraction(&self) -> Option<f64> {
        if self.repairing && !self.repair_indeterminate && self.repair_total > 0 {
            return Some((self.repair_current as f64) / (self.repair_total as f64));
        }
        if let Some(p) = self.history_sync_progress
            && p > 0 {
                return Some((p as f64 / 100.0).clamp(0.0, 1.0));
            }
        None
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
            // Feed every upserted row into the chat inventory so
            // any later widget (chat tab header, message bubble in
            // a newsletter, status row resolver) can pick up the
            // resolved name + avatar synchronously.
            self.chats.ingest_row(
                &row.chat_id,
                &row.kind,
                &row.name,
                row.avatar_path.as_deref(),
            );
            // Auto-fetch trigger: rows whose name is still the raw
            // chat_id (no `display_name` yet) come from the
            // chat_row_select_clause's `COALESCE(... c.chat_id)`
            // fallback. Channels follow this path until
            // `GetNewsletterInfo` resolves.
            if matches!(row.kind.as_str(), "newsletter" | "group")
                && (row.name.is_empty() || row.name == row.chat_id)
            {
                self.chats.request_refresh(&row.chat_id);
            }
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
