// Process-wide caches for resources that the worker resolves once but
// the UI references in many places.
//
// Both inventories are `Rc<RefCell<...>>` (single-threaded, owned by the
// GTK main loop). Components share clones via `Init`; reads are cheap and
// writes happen on `*Ready` events. The inventories don't push updates
// themselves — the existing parent-broadcast pattern handles that. They
// just dedupe fetch requests and let new widgets read previously-resolved
// state without round-tripping the worker again.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// JID kinds the WhatsApp profile-picture endpoint never serves. We
/// short-circuit fetches for these so the worker doesn't burn its
/// 30-second deadline on every channel/status row in the chat list.
fn is_avatar_fetchable(jid: &str) -> bool {
    !jid.is_empty()
        && !jid.ends_with("@broadcast")
        && !jid.ends_with("@newsletter")
}

// ============================================================================
// AvatarInventory: jid → cached profile picture path
// ============================================================================

#[derive(Default)]
struct AvatarInner {
    paths: HashMap<String, String>,
    requested: HashSet<String>,
}

/// Single source of truth for "do we already have a profile picture for
/// this JID?" Sidebar, chat header, and every message row in every tab
/// share one map; closing/reopening a chat doesn't refetch avatars.
#[derive(Clone, Default)]
pub struct AvatarInventory {
    inner: Rc<RefCell<AvatarInner>>,
}

impl AvatarInventory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the cached path, or `None` if we haven't resolved this
    /// JID yet. Doesn't record interest — pair with `mark_requested` if
    /// you intend to ask the worker.
    pub fn get(&self, jid: &str) -> Option<String> {
        self.inner.borrow().paths.get(jid).cloned()
    }

    /// True iff the caller should issue a `FetchAvatar` to the worker:
    /// no cached path AND no prior request. Atomically marks `jid` as
    /// requested on `true` so concurrent callers don't double-fetch.
    ///
    /// JIDs that the WhatsApp profile-picture API doesn't serve are
    /// short-circuited to `false`: `status@broadcast` (the status feed)
    /// and `*@newsletter` channels both 504 with "context deadline
    /// exceeded", and the failure flooded the log. We mark them
    /// "requested" so subsequent calls also short-circuit.
    pub fn needs_fetch(&self, jid: &str) -> bool {
        if !is_avatar_fetchable(jid) {
            self.inner.borrow_mut().requested.insert(jid.to_string());
            return false;
        }
        let mut inner = self.inner.borrow_mut();
        if inner.paths.contains_key(jid) || inner.requested.contains(jid) {
            return false;
        }
        inner.requested.insert(jid.to_string());
        true
    }

    /// Record a resolved avatar.
    pub fn put(&self, jid: String, path: String) {
        let mut inner = self.inner.borrow_mut();
        inner.paths.insert(jid.clone(), path);
        // Keep `requested` populated — the request resolved, no need to
        // re-issue. `invalidate` clears both when the avatar changes.
        inner.requested.insert(jid);
    }

    /// Drop the cached entry for `jid`. Use when the worker signals
    /// that the avatar changed and we want a refetch on next access.
    #[allow(dead_code)]
    pub fn invalidate(&self, jid: &str) {
        let mut inner = self.inner.borrow_mut();
        inner.paths.remove(jid);
        inner.requested.remove(jid);
    }
}

// ============================================================================
// MediaInventory: message_id → in-flight / resolved download state
// ============================================================================

#[derive(Debug, Clone)]
pub struct MediaState {
    pub path: Option<String>,
    pub status: String,
    pub mimetype: Option<String>,
}

#[derive(Default)]
struct MediaInner {
    states: HashMap<String, MediaState>,
}

/// Tracks media download state across tab open/close. The DB persists
/// `media_path`/`media_status` per row, but in-flight states ("downloading"
/// after the user clicked) only live in memory — without this, closing
/// a chat mid-download and reopening would re-show the download button
/// while the worker is still fetching.
#[derive(Clone, Default)]
pub struct MediaInventory {
    inner: Rc<RefCell<MediaInner>>,
}

impl MediaInventory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, message_id: &str) -> Option<MediaState> {
        self.inner.borrow().states.get(message_id).cloned()
    }

    /// Mark a message as actively downloading. Called when the user
    /// clicks the download button.
    pub fn set_downloading(&self, message_id: &str) {
        self.inner.borrow_mut().states.insert(
            message_id.to_string(),
            MediaState {
                path: None,
                status: "downloading".into(),
                mimetype: None,
            },
        );
    }

    /// Apply a `MediaReady` to the cache for every affected id.
    pub fn set_ready(&self, message_ids: &[String], path: &str, mimetype: Option<&str>) {
        let mut inner = self.inner.borrow_mut();
        for id in message_ids {
            inner.states.insert(
                id.clone(),
                MediaState {
                    path: Some(path.to_string()),
                    status: "done".into(),
                    mimetype: mimetype.map(|s| s.to_string()),
                },
            );
        }
    }

    pub fn set_failed(&self, message_id: &str) {
        self.inner.borrow_mut().states.insert(
            message_id.to_string(),
            MediaState {
                path: None,
                status: "failed".into(),
                mimetype: None,
            },
        );
    }
}
