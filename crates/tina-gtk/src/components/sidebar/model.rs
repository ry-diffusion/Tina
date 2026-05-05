// State + read/write helpers for the sidebar.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use crate::fl;
use std::rc::Rc;

use relm4::Controller;
use relm4::typed_view::list::TypedListView;
use tina_db::ChatRow;

use crate::app::ConnectionStatus;
use crate::components::chat_row::ChatRowItem;
use crate::components::profile_menu::ProfileMenu;
use crate::inventory::{AvatarInventory, ChatInventory};

use super::messages::{ChatFilter, SidebarOutput};
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
    pub(super) user_jid: Option<tina_core::WaIdentity>,
    pub(super) avatars: AvatarInventory,
    pub(super) chats: ChatInventory,
    /// JIDs waiting to be fetched once an in-flight slot opens up.
    pub(super) pending_avatar_fetches: VecDeque<tina_core::WaIdentity>,
    /// How many `RequestFetchAvatar` outputs are currently in flight.
    /// Decremented on both `AvatarReady` and `AvatarFailed` so the
    /// queue never stalls after a failure.
    pub(super) in_flight_avatar_count: usize,
}

impl Sidebar {
    /// Subtitle for the headerbar `WindowTitle`. Empty when we're
    /// online and idle so the title sits alone. Priority order:
    /// connection state > repair > history sync — connection trumps
    /// everything because if we're not online there's no point
    /// reporting download progress on a dead pipe.
    pub(super) fn status_subtitle(&self) -> String {
        match self.connection {
            ConnectionStatus::Offline => return fl!("sidebar-offline"),
            ConnectionStatus::Connecting => return fl!("sidebar-connecting"),
            ConnectionStatus::Connected => {}
        }
        if self.repairing {
            if self.repair_indeterminate || self.repair_total <= 0 {
                return fl!("sidebar-syncing") + "…";
            }
            let pct = ((self.repair_current as f64) / (self.repair_total as f64) * 100.0)
                .clamp(0.0, 100.0)
                .round() as i64;
            return format!("{} ({pct}%)", fl!("sidebar-syncing"));
        }
        if let Some(progress) = self.history_sync_progress {
            // Tag the percentage with the sync type when whatsmeow
            // gave us one. INITIAL_BOOTSTRAP stays generic ("Syncing")
            // because it's the most common path and the verbose label
            // adds noise.
            let label = match self.history_sync_type.as_str() {
                "RECENT" => fl!("sidebar-catching-up"),
                "FULL" | "ON_DEMAND" => fl!("sidebar-pulling-history"),
                _ => fl!("sidebar-syncing"),
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

    /// Emit up to `CAP - in_flight_avatar_count` queued fetch requests.
    /// Called after every `ChatsUpserted` batch and after each
    /// `AvatarReady` / `AvatarFailed` so the pipeline stays full
    /// without flooding nanachi with hundreds of simultaneous IPC calls.
    pub(super) fn drain_avatar_queue(
        &mut self,
        sender: &relm4::ComponentSender<Self>,
    ) {
        const CAP: usize = 6;
        while self.in_flight_avatar_count < CAP {
            let Some(jid) = self.pending_avatar_fetches.pop_front() else {
                break;
            };
            self.in_flight_avatar_count += 1;
            let _ = sender.output(SidebarOutput::RequestFetchAvatar(jid));
        }
    }

    pub(super) fn apply_chats_upserted(&mut self, rows: Vec<ChatRow>) {
        // Build a chat_id → position index in one O(n) pass so the loop
        // below is O(n) instead of O(n²). Without this, every row called
        // find_chat_position() which iterated the entire list, causing
        // noticeable freezes when the initial batch of 200+ chats landed.
        let mut pos_index: std::collections::HashMap<String, u32> =
            std::collections::HashMap::with_capacity(self.list.len() as usize);
        for pos in 0..self.list.len() {
            if let Some(item) = self.list.get(pos) {
                pos_index.insert(item.borrow().chat_id.clone(), pos);
            }
        }

        for row in &rows {
            self.chats.ingest_row(
                &row.chat_id,
                &row.kind,
                &row.name,
                row.avatar_path.as_deref(),
            );
            let id = crate::wa_id::WaIdentity::parse(&row.chat_id);
            if id.needs_metadata_refresh()
                && crate::wa_id::WaIdentity::looks_like_unresolved_name(&row.name)
            {
                self.chats.request_refresh(id.raw());
            }
            let mut item = ChatRowItem::from_row(row, self.avatars.clone());
            if let Some(&pos) = pos_index.get(&item.chat_id) {
                if let Some(prev) = self.list.get(pos) {
                    let prev = prev.borrow();
                    // Carry over `is_active` — a fresh from_row() always
                    // returns false, which would strip the highlight from
                    // an open chat on every incoming message.
                    item.is_active = prev.is_active;
                    // Skip the replace (and its two items_changed signals)
                    // if nothing that affects sort order or display changed.
                    // On reconnect, most rows are identical — this cuts the
                    // number of SortListModel re-evaluations dramatically.
                    if !item.differs_from(&prev) {
                        continue;
                    }
                }
                self.replace_at(pos, item);
            } else {
                self.list.append(item);
            }
        }
    }
}
