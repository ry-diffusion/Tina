// State carried by the `ChatTab` component. Fields are split across two
// sibling files (`actions.rs` for the message-handler bodies, `view.rs`
// for the relm4 view + update dispatcher) — this file owns the struct
// definition + the small read-only helpers everyone reuses.

use std::cell::Cell;
use crate::fl;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use adw::prelude::*;
use relm4::typed_view::list::TypedListView;

use crate::components::message_row::{MessageRowItem, RowUiInventory};
use crate::inventory::{AvatarInventory, MediaInventory, MentionInventory};

pub struct ChatTab {
    pub(super) chat_id: String,
    pub(super) name: String,
    pub(super) kind: String,
    /// Virtualised chat timeline. Backed by a `gtk::ListView` factory
    /// — only the rows currently in the viewport carry a realised
    /// widget tree. Off-screen rows live as `glib::Object`-boxed
    /// `MessageRowItem` values inside the `gio::ListStore` and
    /// re-realise on scroll-in.
    pub(super) list: TypedListView<MessageRowItem, gtk::NoSelection>,
    pub(super) composer_buffer: gtk::EntryBuffer,
    pub(super) avatars: AvatarInventory,
    pub(super) media: MediaInventory,
    pub(super) mentions: MentionInventory,
    /// Per-row UI state (currently `media_expanded`) keyed by
    /// `message_id`. Survives recycling because the inventory is
    /// owned by the `ChatTab`, not the row widgets.
    pub(super) ui_state: RowUiInventory,
    /// JIDs the user picked from the `@`-popover for the current
    /// composer draft. Drained on `handle_send`. Stored here rather
    /// than recomputed at send time because the typed text may not
    /// retain enough information (the user could replace the chip
    /// label or the `@<digits>` after picking).
    pub(super) pending_mentions: HashSet<String>,
    /// Live `@`-mention popover. Anchored to the composer entry,
    /// rebuilt on each open. `None` until the view is constructed.
    pub(super) mention_popover: Option<crate::components::mention_popover::MentionPopover>,
    pub(super) user_jid: Option<tina_core::WaIdentity>,
    pub(super) scroll: Option<gtk::ScrolledWindow>,
    /// Cloned input sender stashed at init, used as the per-row
    /// dispatch handle threaded through every `MessageRowItem`. Keeps
    /// the row's click handlers self-contained (no factory output
    /// wiring needed because RelmListItem doesn't expose an output
    /// channel).
    pub(super) sender_handle: relm4::Sender<super::messages::ChatTabInput>,
    pub(super) seen_message_ids: HashSet<String>,
    pub(super) last_send: Option<(String, std::time::Instant)>,
    pub(super) oldest_ts: Option<i64>,
    pub(super) loading_older: bool,
    pub(super) reached_top: bool,
    /// Mirror of `oldest_ts` for the descending pagination path.
    /// `Some(ts)` when the list holds at least one row; the worker
    /// pages forward from this timestamp on `NearBottomFetch`.
    pub(super) newest_ts: Option<i64>,
    pub(super) loading_newer: bool,
    /// `true` when we know the list tail is the actual DB tail —
    /// either because the chat just opened (initial 50 always
    /// includes the newest) or because a `LoadNewer` returned fewer
    /// rows than requested. Cleared whenever `TrimBottom` or
    /// `Append`'s autoscroll-cap drops the newest rows from the list.
    pub(super) reached_bottom: bool,
    pub(super) pending_echoes: HashMap<String, VecDeque<String>>,
    /// Pending media echoes keyed by lower-hex SHA-256 of the source
    /// file. The Go side echoes the row back with the same hash, so
    /// matching here is exact and immune to caption mismatches that
    /// broke the body-text path. Multiple sends of the same file
    /// (e.g. forwarding the same sticker twice in a row) queue under
    /// the same key, FIFO.
    pub(super) pending_media_echoes: HashMap<String, VecDeque<String>>,
    /// Sticky-bottom state, ported from dissent's autoscroll.Window. When
    /// `true`, every `vadj.changed` (new content added → upper grew)
    /// re-scrolls to `upper - page_size`. Cleared when the user scrolls
    /// away from the bottom; re-set when they scroll back.
    pub(super) bottomed: Rc<Cell<bool>>,
    /// Edge-detection flag matching dissent's `updatedValue`. The
    /// `changed` signal sets it; the deferred `value-changed` resolution
    /// reads it to distinguish "GTK relayout finished" from "user
    /// dragged the scrollbar".
    pub(super) updated_value: Rc<Cell<bool>>,
    /// Stronger lock for programmatic scroll-position writes. While
    /// set, the `wire_value_changed` listener completely ignores the
    /// vadjustment (no NearTop / NearBottom / bottomed update). The
    /// flag is held across an entire `update_item_at` (which fires
    /// multiple value-changed events in sequence — remove, insert,
    /// then our restore set_value) and only released on the next
    /// idle, after GTK has fully settled.
    ///
    /// `updated_value` alone wasn't enough: it's a one-shot flag,
    /// consumed by the first value-changed; the second and third
    /// events from the same operation slipped through and got
    /// interpreted as the user scrolling.
    pub(super) scroll_lock: Rc<Cell<bool>>,
    /// Active voice-record handle. `Some` while a `gst-launch-1.0`
    /// pipeline is capturing; toggled off by `ToggleRecord` (which
    /// SIGINTs the child and waits for the writer to flush). The
    /// state is signalled to the view via `recording_active`.
    pub(super) recorder: Option<super::record::RecordingHandle>,
    pub(super) recording_active: Rc<Cell<bool>>,
    /// Live state of the sticker-picker popover. The popover widget
    /// is shared here so Open/StickersLoaded can repaint it without
    /// wiring a separate sub-component, and the FlowBox below holds
    /// the tile widgets we recreate on each refresh.
    pub(super) sticker_popover: Option<gtk::Popover>,
    pub(super) sticker_grid: Option<gtk::FlowBox>,
}

impl ChatTab {
    pub fn chat_id(&self) -> &str {
        &self.chat_id
    }

    /// True for chat kinds the protocol won't let us send to:
    /// newsletters / channels (read-only by spec — only the channel
    /// owner publishes), the `status@broadcast` pseudo-chat (status
    /// posts go through a different API), and broadcast lists.
    pub(super) fn is_read_only(&self) -> bool {
        matches!(self.kind.as_str(), "newsletter" | "status" | "broadcast")
    }

    pub(super) fn read_only_label(&self) -> String {
        match self.kind.as_str() {
            "newsletter" => fl!("readonly-newsletter"),
            "status" => fl!("readonly-status"),
            "broadcast" => fl!("readonly-broadcast"),
            _ => fl!("readonly-default"),
        }
    }

    /// Bundle the chat's kind + display name + avatar path for the
    /// per-row builder. The avatar path is pulled from the inventory
    /// because the chat's own row already lives in the sidebar list
    /// and we don't otherwise track it on `ChatTab`.
    pub(super) fn chat_context(&self) -> super::build::ChatContext {
        super::build::ChatContext {
            kind: self.kind.clone(),
            display_name: if self.name.is_empty() {
                None
            } else {
                Some(self.name.clone())
            },
            avatar_path: self.avatars.get(&self.chat_id),
        }
    }

    /// Clone of the dispatch sender for embedding in a fresh
    /// `MessageRowItem`. Cheap (mpsc-backed handle).
    pub(super) fn row_sender(&self) -> relm4::Sender<super::messages::ChatTabInput> {
        self.sender_handle.clone()
    }

    /// Wrap a `MessageItem` into a `MessageRowItem` ready to push
    /// into the typed list view.
    pub(super) fn wrap_row(
        &self,
        item: crate::components::message_bubble::MessageItem,
    ) -> MessageRowItem {
        MessageRowItem::new(
            item,
            self.avatars.clone(),
            self.media.clone(),
            self.ui_state.clone(),
            self.row_sender(),
        )
    }

    pub(super) fn list_len(&self) -> u32 {
        self.list.len()
    }

    /// Last item in the timeline by index (the chronological tail).
    pub(super) fn list_back(&self) -> Option<MessageRowItem> {
        let n = self.list.len();
        if n == 0 {
            return None;
        }
        self.list.get(n - 1).map(|h| h.borrow().clone())
    }

    /// First item in the timeline (the chronological head).
    pub(super) fn list_front(&self) -> Option<MessageRowItem> {
        if self.list.len() == 0 {
            return None;
        }
        self.list.get(0).map(|h| h.borrow().clone())
    }

    /// Snapshot of the current items as `(index, MessageRowItem)`
    /// pairs. Used by echo-confirmation and `RebindRow` so the caller
    /// can locate a row by `message_id` without holding the typed
    /// view's borrow across multiple operations. The clone is cheap:
    /// `MessageRowItem` is `Rc`-heavy.
    pub(super) fn list_snapshot(&self) -> Vec<(u32, MessageRowItem)> {
        let n = self.list.len();
        let mut out = Vec::with_capacity(n as usize);
        for i in 0..n {
            if let Some(h) = self.list.get(i) {
                out.push((i, h.borrow().clone()));
            }
        }
        out
    }

    /// `(sender_key, timestamp)` for the trailing item in the
    /// timeline. Used to seed collapse decisions for incoming Append
    /// batches and optimistic Send echoes — the list IS the source of
    /// truth for "what was just rendered", so this avoids the drift
    /// that creeps in when a separate `last_sender` field is kept in
    /// sync across many code paths.
    pub(super) fn factory_tail_cursor(&self) -> (Option<String>, Option<i64>) {
        let Some(last) = self.list_back() else {
            return (None, None);
        };
        let key = if last.item.from_me {
            "\0me".to_string()
        } else {
            last.item.sender_name.clone()
        };
        (Some(key), Some(last.item.timestamp_unix))
    }

    /// Local-day key of the trailing list item — counterpart to
    /// `factory_tail_cursor` for the day-divider grouping pass.
    pub(super) fn factory_tail_day(&self) -> Option<String> {
        let last = self.list_back()?;
        Some(crate::time::local_day_key(last.item.timestamp_unix))
    }

    /// Local-day key of the leading (oldest) list item. Used by
    /// `handle_prepend_older` so when a fresh older batch lands, the
    /// row immediately following the prepended block can decide
    /// whether it still needs its day pill.
    pub(super) fn factory_head_day(&self) -> Option<String> {
        let first = self.list_front()?;
        Some(crate::time::local_day_key(first.item.timestamp_unix))
    }

    /// Locate a row by `message_id`. Linear scan (the typed view's
    /// model doesn't carry an index map). Cheap given the soft cap
    /// keeps the list bounded in the low hundreds.
    pub(super) fn find_index(&self, message_id: &str) -> Option<u32> {
        let n = self.list.len();
        for i in 0..n {
            if let Some(h) = self.list.get(i)
                && h.borrow().item.id == message_id
            {
                return Some(i);
            }
        }
        None
    }

    /// Apply `f` to the `MessageItem` at `idx`, then push the
    /// updated row back into the list (replace via remove+insert
    /// which is what triggers a rebind on the typed view). The
    /// closure is called with `&mut MessageItem`, so callers can
    /// mutate fields in place. Returns `true` if `idx` was valid.
    ///
    /// Wraps the remove+insert in a `scroll_lock` window: the
    /// `wire_value_changed` listener bails out completely while the
    /// lock is held, and the lock is only released on the next idle
    /// — after GTK has fired every value-changed cascading from the
    /// items_changed events, AND from our own value-restore. Without
    /// the lock, secondary value-changed events slipped through the
    /// one-shot `updated_value` flag and the user would visibly
    /// drift up by a fraction each click.
    pub(super) fn update_item_at(
        &mut self,
        idx: u32,
        f: impl FnOnce(&mut crate::components::message_bubble::MessageItem),
    ) -> bool {
        let Some(handle) = self.list.get(idx) else {
            return false;
        };
        let mut row = handle.borrow().clone();
        f(&mut row.item);
        let saved_value = self
            .scroll
            .as_ref()
            .map(|s| s.vadjustment().value());
        // Lock — must be held across remove + insert + restore. The
        // listener early-returns the entire time, so no NearTop /
        // NearBottom / bottomed-flip leaks through.
        self.scroll_lock.set(true);
        self.list.remove(idx);
        self.list.insert(idx, row);
        if let (Some(scroll), Some(val)) = (self.scroll.as_ref(), saved_value) {
            let adj = scroll.vadjustment();
            // Clamp to the new range — `upper` may have shifted by
            // a fraction if the row's measured size changed.
            let target = val.min(adj.upper() - adj.page_size()).max(0.0);
            if (adj.value() - target).abs() > 0.5 {
                adj.set_value(target);
            }
        }
        // Release the lock on idle. Any value-changed GTK still has
        // queued from this operation runs first (with lock held),
        // then the lock drops so genuine user scrolls are honoured.
        let lock = self.scroll_lock.clone();
        gtk::glib::idle_add_local_once(move || {
            lock.set(false);
        });
        true
    }

    /// `update_item_at` indexed by message_id. Returns `true` iff the
    /// row was found in the loaded window.
    pub(super) fn update_item_by_id(
        &mut self,
        message_id: &str,
        f: impl FnOnce(&mut crate::components::message_bubble::MessageItem),
    ) -> bool {
        let Some(idx) = self.find_index(message_id) else {
            return false;
        };
        self.update_item_at(idx, f)
    }

    /// Apply `f` to every item whose predicate returns true. Used
    /// for broadcast updates like avatar resolution / SetUserJid.
    pub(super) fn update_items_where(
        &mut self,
        predicate: impl Fn(&crate::components::message_bubble::MessageItem) -> bool,
        f: impl Fn(&mut crate::components::message_bubble::MessageItem),
    ) {
        let n = self.list.len();
        // Collect target indices first; mutating the list during
        // iteration would invalidate the indices.
        let mut targets: Vec<u32> = Vec::new();
        for i in 0..n {
            if let Some(h) = self.list.get(i)
                && predicate(&h.borrow().item)
            {
                targets.push(i);
            }
        }
        for idx in targets {
            self.update_item_at(idx, &f);
        }
    }
}
