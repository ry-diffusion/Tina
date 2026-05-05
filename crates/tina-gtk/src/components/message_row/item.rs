// `MessageRowItem` — the value type passed to the chat timeline's
// `TypedListView`. Carries the `MessageItem` data plus the shared
// inventories and the parent's input sender so per-row click handlers
// installed in `setup` can dispatch back to the chat tab without
// going through a factory output channel (which `RelmListItem`
// doesn't have).
//
// Per-row UI state that must persist across recycling lives behind
// the `ui_state` inventory keyed by `message_id` — when the user
// clicks "play" on a video, the row scrolls offscreen, then back, the
// expanded state must come back too. Storing it on the item itself
// would lose the bit on rebind because the typed view re-projects
// from the gio::ListStore each time.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::components::message_bubble::MessageItem;
use crate::inventory::{AvatarInventory, MediaInventory};

use super::super::chat_tab::messages::ChatTabInput;

/// Per-row UI state inventory. Keyed by `message_id`. Currently only
/// tracks `media_expanded` (the lazy-instantiate-MediaFile flag), but
/// the shape leaves room for additional row-local toggles down the
/// line (e.g. "show original markdown", "expand long quote").
#[derive(Clone, Default)]
pub struct RowUiInventory {
    inner: Rc<RefCell<HashMap<String, RowUiState>>>,
}

#[derive(Clone, Default)]
pub struct RowUiState {
    pub media_expanded: bool,
}

impl RowUiInventory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, message_id: &str) -> RowUiState {
        self.inner
            .borrow()
            .get(message_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn set_media_expanded(&self, message_id: &str, expanded: bool) {
        let mut inner = self.inner.borrow_mut();
        inner
            .entry(message_id.to_string())
            .or_default()
            .media_expanded = expanded;
    }

    /// Drop entries for ids no longer in the timeline. Called after
    /// soft-cap trims so the map doesn't grow unbounded over a long
    /// session.
    pub fn forget(&self, message_ids: &[String]) {
        let mut inner = self.inner.borrow_mut();
        for id in message_ids {
            inner.remove(id);
        }
    }
}

#[derive(Clone)]
pub struct MessageRowItem {
    pub item: MessageItem,
    pub avatars: AvatarInventory,
    pub media_inv: MediaInventory,
    pub ui_state: RowUiInventory,
    /// Cloned from the parent ChatTab's input sender. Cheap clone
    /// (relm4 senders are mpsc-backed). Click handlers installed in
    /// `setup` capture this through a per-slot `Rc<RefCell<…>>`
    /// updated by `bind`, so rebinding swaps the click target
    /// without re-wiring the gesture controllers.
    pub sender: relm4::Sender<ChatTabInput>,
}

impl MessageRowItem {
    pub fn new(
        item: MessageItem,
        avatars: AvatarInventory,
        media_inv: MediaInventory,
        ui_state: RowUiInventory,
        sender: relm4::Sender<ChatTabInput>,
    ) -> Self {
        Self {
            item,
            avatars,
            media_inv,
            ui_state,
            sender,
        }
    }
}
