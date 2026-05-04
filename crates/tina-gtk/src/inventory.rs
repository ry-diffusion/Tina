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
use std::collections::{HashMap, HashSet, VecDeque};
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

/// Bounded LRU keyed by avatar path. Decoding a 64x64 JPEG via
/// `Texture::from_filename` is cheap individually but `chat_row::bind`
/// runs every time a row scrolls into view — at hundreds of rows in the
/// sidebar that's the same file decoded over and over. The cache is
/// keyed by path (not JID) because avatars dedupe by sha256, so multiple
/// JIDs share files.
struct TextureCache {
    map: HashMap<String, gtk::gdk::Texture>,
    order: VecDeque<String>,
    cap: usize,
}

impl TextureCache {
    fn new(cap: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            cap,
        }
    }

    fn touch(&mut self, path: &str) {
        if let Some(pos) = self.order.iter().position(|p| p == path) {
            self.order.remove(pos);
        }
        self.order.push_back(path.to_string());
    }

    fn get(&mut self, path: &str) -> Option<gtk::gdk::Texture> {
        let tex = self.map.get(path).cloned()?;
        self.touch(path);
        Some(tex)
    }

    fn insert(&mut self, path: String, tex: gtk::gdk::Texture) {
        if self.map.contains_key(&path) {
            self.touch(&path);
            self.map.insert(path, tex);
            return;
        }
        if self.map.len() >= self.cap
            && let Some(oldest) = self.order.pop_front()
        {
            self.map.remove(&oldest);
        }
        self.order.push_back(path.clone());
        self.map.insert(path, tex);
    }

    fn invalidate(&mut self, path: &str) {
        if self.map.remove(path).is_some()
            && let Some(pos) = self.order.iter().position(|p| p == path)
        {
            self.order.remove(pos);
        }
    }
}

#[derive(Default)]
struct AvatarInner {
    paths: HashMap<String, String>,
    requested: HashSet<String>,
    textures: Option<TextureCache>,
}

/// Single source of truth for "do we already have a profile picture for
/// this JID?" Sidebar, chat header, and every message row in every tab
/// share one map; closing/reopening a chat doesn't refetch avatars.
#[derive(Clone, Default)]
pub struct AvatarInventory {
    inner: Rc<RefCell<AvatarInner>>,
}

const TEXTURE_CACHE_CAP: usize = 256;

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
        // The path may have replaced an older one for the same JID
        // (avatar refresh) — drop any cached texture for the *previous*
        // path so we don't keep serving a stale paintable. We can't drop
        // it from here because we don't track per-JID path history; the
        // worker emits `AvatarReady` with the new path and we just trust
        // that consumers re-`load_texture` after `put`.
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
        if let Some(path) = inner.paths.remove(jid)
            && let Some(cache) = inner.textures.as_mut()
        {
            cache.invalidate(&path);
        }
        inner.requested.remove(jid);
    }

    /// Look up (and amortise the decode of) a `gdk::Texture` for an
    /// avatar file path. Returns `None` if `path` is `None`/empty or the
    /// file can't be decoded. Callers should prefer this over
    /// `Texture::from_filename` directly so repeated binds of the same
    /// row don't redo the decode.
    pub fn load_texture(&self, path: Option<&str>) -> Option<gtk::gdk::Texture> {
        let path = path?;
        if path.is_empty() {
            return None;
        }
        let mut inner = self.inner.borrow_mut();
        let cache = inner
            .textures
            .get_or_insert_with(|| TextureCache::new(TEXTURE_CACHE_CAP));
        if let Some(tex) = cache.get(path) {
            return Some(tex);
        }
        let tex = gtk::gdk::Texture::from_filename(path).ok()?;
        cache.insert(path.to_string(), tex.clone());
        Some(tex)
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
