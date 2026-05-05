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

use adw::prelude::*;
use tina_db::MentionCandidate;

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
    url_requested: HashSet<String>,
    textures: Option<TextureCache>,
    /// Paths whose async glycin decode is in flight. Prevents
    /// duplicate spawns when many rows ask for the same avatar at
    /// once (the sidebar's first paint, opening a tab).
    in_flight_decodes: HashSet<String>,
}

/// Single source of truth for "do we already have a profile picture for
/// this JID?" Sidebar, chat header, and every message row in every tab
/// share one map; closing/reopening a chat doesn't refetch avatars.
#[derive(Clone, Default)]
pub struct AvatarInventory {
    inner: Rc<RefCell<AvatarInner>>,
    /// Fires once per successful async glycin decode of an avatar
    /// file. The hosting app wires this to broadcast a "texture
    /// ready" message that nudges the sidebar + open chat tabs into
    /// rebinding rows whose `avatar_path` matches. Without it, the
    /// avatar would only appear after the next unrelated rebind.
    on_texture_ready: Rc<RefCell<Option<Box<dyn Fn(String)>>>>,
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

    /// True iff the caller should issue a `FetchAvatarFromURL` to the worker.
    /// Uses a separate tracking set from `needs_fetch` so URL-based fetches
    /// and API fetches don't interfere with each other.
    pub fn needs_url_fetch(&self, jid: &str) -> bool {
        let mut inner = self.inner.borrow_mut();
        if inner.paths.contains_key(jid) || inner.url_requested.contains(jid) {
            return false;
        }
        inner.url_requested.insert(jid.to_string());
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
        inner.url_requested.remove(jid);
    }

    /// Look up a cached `gdk::Texture` for an avatar file path.
    /// Returns `None` on miss AND fires an async glycin decode in
    /// the background — the cache is populated when the load
    /// completes, and the inventory's `on_texture_ready` callback
    /// fires so UI components can rebind affected rows.
    ///
    /// We never go through GdkPixbuf nor `Texture::from_filename`:
    /// avatars come from arbitrary contacts on the network, and
    /// libwebp / libheif RCEs (CVE-2023-4863 et al) live exactly in
    /// those non-sandboxed loaders. Glycin's external-process
    /// decoder is the safe path.
    pub fn load_texture(&self, path: Option<&str>) -> Option<gtk::gdk::Texture> {
        let path = path?;
        if path.is_empty() {
            return None;
        }
        // Cache hit?
        {
            let mut inner = self.inner.borrow_mut();
            let cache = inner
                .textures
                .get_or_insert_with(|| TextureCache::new(TEXTURE_CACHE_CAP));
            if let Some(tex) = cache.get(path) {
                return Some(tex);
            }
        }
        // Cache miss — fire async glycin decode (deduped per path).
        let need_spawn = {
            let mut inner = self.inner.borrow_mut();
            if inner.in_flight_decodes.contains(path) {
                false
            } else {
                inner.in_flight_decodes.insert(path.to_string());
                true
            }
        };
        if need_spawn {
            self.spawn_glycin_decode(path.to_string());
        }
        None
    }

    /// Wire the post-decode notification. Called once at app init.
    /// `f` receives the path that just landed; the implementation
    /// dispatches a UI message that fans out to sidebar + chat
    /// area to rebind matching rows.
    pub fn set_texture_ready_handler<F: Fn(String) + 'static>(&self, f: F) {
        *self.on_texture_ready.borrow_mut() = Some(Box::new(f));
    }

    fn spawn_glycin_decode(&self, path: String) {
        let inner_clone = self.inner.clone();
        let cb_clone = self.on_texture_ready.clone();
        gtk::glib::MainContext::default().spawn_local(async move {
            let file = gtk::gio::File::for_path(&path);
            let loader = glycin::Loader::new(file);
            let texture: Option<gtk::gdk::Texture> = async {
                let image = loader.load().await.ok()?;
                let frame = image.next_frame().await.ok()?;
                Some(frame.texture())
            }
            .await;

            let mut inner = inner_clone.borrow_mut();
            inner.in_flight_decodes.remove(&path);
            let Some(tex) = texture else {
                tracing::debug!(path = %path, "glycin: avatar decode failed");
                return;
            };
            let cache = inner
                .textures
                .get_or_insert_with(|| TextureCache::new(TEXTURE_CACHE_CAP));
            cache.insert(path.clone(), tex);
            drop(inner);
            if let Some(cb) = cb_clone.borrow().as_ref() {
                cb(path);
            }
        });
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
    /// Active download policy. Set by the App on PreferencesLoaded /
    /// SetDownloadMethod and read by every chat tab when it gets a
    /// fresh batch of rows. Defaults to OnDemand because that's what
    /// the Settings dialog defaults to.
    download_method: crate::components::settings::DownloadMethod,
    /// Recently auto-queued downloads. Prevents duplicate
    /// RequestMediaDownload emissions when the same row is loaded
    /// twice (e.g. tab close + reopen). Bounded by a hard cap so a
    /// long-lived session doesn't grow this forever.
    auto_queued: HashSet<String>,
    /// LRU cache for inline-thumbnail textures. Decoded once via
    /// glycin (sandboxed) and reused across rebinds. Keyed by
    /// message_id — each row has its own bytes blob from the wire
    /// proto; sharing across messages isn't safe.
    thumbnails: Option<TextureCache>,
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

    pub fn download_method(&self) -> crate::components::settings::DownloadMethod {
        self.inner.borrow().download_method
    }

    /// Update the active download policy. Idempotent — safe to call
    /// from both PreferencesLoaded and SetDownloadMethod paths.
    pub fn set_download_method(&self, m: crate::components::settings::DownloadMethod) {
        self.inner.borrow_mut().download_method = m;
    }

    /// Returns `true` the first time a message id is offered for
    /// auto-download; subsequent calls return `false`. Bounded so a
    /// long session can't grow the set forever.
    pub fn try_mark_auto_queued(&self, message_id: &str) -> bool {
        const CAP: usize = 4_096;
        let mut inner = self.inner.borrow_mut();
        if inner.auto_queued.contains(message_id) {
            return false;
        }
        if inner.auto_queued.len() >= CAP {
            // Drop the whole set rather than trying to track LRU; the
            // worst case is a duplicate request to the worker which
            // dedups via media_status="downloading".
            inner.auto_queued.clear();
        }
        inner.auto_queued.insert(message_id.to_string());
        true
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

    /// Sync cache lookup for an inline-thumbnail paintable keyed by
    /// `message_id`. Returns `None` on miss; the caller is expected
    /// to call `request_thumbnail_decode()` to fire the async glycin
    /// load (sandboxed, no GdkPixbuf surface).
    pub fn cached_thumbnail(&self, message_id: &str) -> Option<gtk::gdk::Paintable> {
        let mut inner = self.inner.borrow_mut();
        let cache = inner
            .thumbnails
            .get_or_insert_with(|| TextureCache::new(THUMBNAIL_CACHE_CAP));
        cache.get(message_id).map(|t| t.upcast())
    }

    /// Insert a freshly-decoded thumbnail texture into the cache.
    /// Called by the widget after its async glycin decode completes.
    pub fn put_thumbnail(&self, message_id: &str, tex: gtk::gdk::Texture) {
        let mut inner = self.inner.borrow_mut();
        let cache = inner
            .thumbnails
            .get_or_insert_with(|| TextureCache::new(THUMBNAIL_CACHE_CAP));
        cache.insert(message_id.to_string(), tex);
    }
}

const THUMBNAIL_CACHE_CAP: usize = 512;

// ============================================================================
// ChatInventory: chat_id → resolved metadata + auto-refresh on miss
// ============================================================================

#[derive(Clone, Default, Debug)]
pub struct ChatMeta {
    pub kind: String,
    pub display_name: Option<String>,
    pub avatar_path: Option<String>,
}

#[derive(Default)]
struct ChatInner {
    metas: HashMap<String, ChatMeta>,
    /// JIDs we've already asked the worker to refresh. Without
    /// this, every chat-tab open or every list-row bind would queue
    /// another fetch — the deepwiki notes whatsmeow's
    /// `GetNewsletterInfo` is a real GraphQL roundtrip, not free.
    refresh_requested: HashSet<String>,
}

/// Cache + auto-refresh for chat-level metadata (display name,
/// avatar). Indexed by raw chat_id (the same key the DB uses). On
/// miss the inventory pings a sender that the AppModel routes back
/// to the worker (`Cmd::RefreshChat`), so callers can stay
/// synchronous without thinking about IPC.
///
/// Inspired by the way "Amnesia" pulls missing data on demand:
/// every render lazily fills in what it doesn't have, and the next
/// frame already has the answer. The inventory dedupes the requests
/// per chat_id so a 50-row sidebar bind doesn't fan out 50 IPCs.
#[derive(Clone, Default)]
pub struct ChatInventory {
    inner: Rc<RefCell<ChatInner>>,
    /// Out-channel for missing-data fetches. Wired by the AppModel
    /// to `AppMsg::RequestRefreshChat`. None during tests / before
    /// the worker is up; misses just stay misses.
    on_miss: Rc<RefCell<Option<Box<dyn Fn(String)>>>>,
}

impl ChatInventory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_miss_handler<F: Fn(String) + 'static>(&self, f: F) {
        *self.on_miss.borrow_mut() = Some(Box::new(f));
    }

    /// Apply a snapshot from `ChatsUpserted` so subsequent renders
    /// hit the cache without firing a refresh. Names that look like
    /// raw JIDs collapse to `display_name: None` so the next miss
    /// check correctly flags the row as still-unresolved — without
    /// that, a chat whose row echoes its own JID back would happily
    /// cache the JID as its "name" and never refresh.
    pub fn ingest_row(&self, chat_id: &str, kind: &str, name: &str, avatar: Option<&str>) {
        let resolved_name = if crate::wa_id::WaIdentity::looks_like_unresolved_name(name) {
            None
        } else {
            Some(name.to_string())
        };
        let meta = ChatMeta {
            kind: kind.to_string(),
            display_name: resolved_name,
            avatar_path: avatar.map(|s| s.to_string()),
        };
        self.inner
            .borrow_mut()
            .metas
            .insert(chat_id.to_string(), meta);
    }

    #[allow(dead_code)] // consumed by future render-time accessors
    pub fn get(&self, chat_id: &str) -> Option<ChatMeta> {
        self.inner.borrow().metas.get(chat_id).cloned()
    }

    /// Returns the cached meta, OR fires a refresh request and
    /// returns whatever stale value we have (possibly `None`). The
    /// refresh is deduped per chat_id so calling this on every row
    /// bind is cheap.
    #[allow(dead_code)] // consumed by future render-time accessors
    pub fn get_or_request(&self, chat_id: &str) -> Option<ChatMeta> {
        let cached = self.get(chat_id);
        let needs_refresh = match cached.as_ref() {
            None => true,
            Some(m) => m.display_name.is_none(),
        };
        if needs_refresh {
            self.request_refresh(chat_id);
        }
        cached
    }

    /// Fire a refresh, deduped per chat_id. Idempotent; safe to call
    /// from binding paths that run on every scroll.
    pub fn request_refresh(&self, chat_id: &str) {
        let already = {
            let mut inner = self.inner.borrow_mut();
            !inner.refresh_requested.insert(chat_id.to_string())
        };
        if already {
            return;
        }
        if let Some(cb) = self.on_miss.borrow().as_ref() {
            cb(chat_id.to_string());
        }
    }
}

// ============================================================================
// MessageInventory: message_id → metadata for replies / quoted messages
// ============================================================================

/// Slim copy of a message used by the bubble's reply-quote header
/// and (eventually) by mention/forward affordances. Built lazily —
/// every chat tab feeds its `bind` rows in here so replies can
/// resolve the cited message synchronously without a DB roundtrip.
#[allow(dead_code)] // fields read by the future reply-quote renderer
#[derive(Clone, Debug)]
pub struct MessageMeta {
    pub message_id: String,
    /// Resolved sender name (post-contact-resolution) when the row
    /// is from another participant; `None` for the user's own messages.
    pub sender_name: Option<String>,
    pub message_type: String,
    /// Short text preview suitable for a one-line quote header.
    /// Caller decides what to put here — typically the first 80
    /// chars of `content` or a `📷 Foto`-style fallback for media.
    pub preview: String,
    pub timestamp_unix: i64,
}

#[derive(Default)]
struct MessageInventoryInner {
    /// Bounded so a long-running session doesn't grow forever.
    /// Reply lookups are recent-skewed (the user usually replies
    /// to something they just saw), so an LRU on the latest few
    /// thousand messages covers the realistic working set.
    metas: HashMap<String, MessageMeta>,
    order: VecDeque<String>,
}

const MESSAGE_INVENTORY_CAP: usize = 4_096;

/// Companion to `ChatInventory`, but indexed by message_id. The
/// reply UI we'll layer on top of this asks for the cited message's
/// sender + preview at render time; without a cache we'd round-trip
/// the worker for every quoted bubble in the thread.
#[derive(Clone, Default)]
pub struct MessageInventory {
    inner: Rc<RefCell<MessageInventoryInner>>,
}

impl MessageInventory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cache (or update) one message. Cheap enough to call from
    /// every bubble-bind path; the LRU bookkeeping keeps the map
    /// bounded.
    #[allow(dead_code)] // first consumer lands with reply rendering
    pub fn put(&self, meta: MessageMeta) {
        let mut inner = self.inner.borrow_mut();
        if !inner.metas.contains_key(&meta.message_id) {
            inner.order.push_back(meta.message_id.clone());
            // LRU evict from the front when over capacity. Cheap
            // because the front is the oldest insertion, not the
            // oldest access — replies skew recent so the order ≈
            // access pattern in practice.
            while inner.order.len() > MESSAGE_INVENTORY_CAP {
                if let Some(victim) = inner.order.pop_front() {
                    inner.metas.remove(&victim);
                } else {
                    break;
                }
            }
        }
        inner.metas.insert(meta.message_id.clone(), meta);
    }

    /// Look up a cached message. Returns `None` for evicted /
    /// never-cached IDs; the reply renderer falls back to a stub
    /// "Original message" header in that case.
    #[allow(dead_code)] // first consumer lands with reply rendering
    pub fn get(&self, message_id: &str) -> Option<MessageMeta> {
        self.inner.borrow().metas.get(message_id).cloned()
    }
}

// ============================================================================
// MentionInventory: chat_id → autocomplete list + jid → display_name
// ============================================================================

#[derive(Default)]
struct MentionInner {
    /// Per-chat candidate list. Loaded once per chat from the
    /// worker's `list_mention_candidates` and refreshed on the next
    /// open. Empty for DMs/newsletters.
    by_chat: HashMap<String, Vec<MentionCandidate>>,
    /// jid → display_name. Populated alongside `by_chat` so the
    /// bubble renderer can resolve `@<digits>` to a human name even
    /// for chats whose tab isn't currently open.
    names_by_jid: HashMap<String, String>,
    /// digits → display_name. Mentions in WhatsApp text reference
    /// the user-part of the JID (e.g. `@5511999999999`), not the
    /// full JID; the renderer scans for `@<digits>` so it needs a
    /// digits-keyed lookup.
    names_by_digits: HashMap<String, String>,
}

/// Process-wide cache of `@`-mention data: per-chat candidate lists
/// (the popover's filter input) and a global digits-keyed name map
/// (the bubble renderer's resolver). Single inventory rather than
/// two so the worker only fires one event per chat-open and both
/// sides stay in sync.
#[derive(Clone, Default)]
pub struct MentionInventory {
    inner: Rc<RefCell<MentionInner>>,
}

impl MentionInventory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the candidate list for `chat_id` and merge each
    /// candidate's name into the global digits-keyed map. We never
    /// evict from the global map — names only get more accurate
    /// over time, and forgetting one means the bubble briefly
    /// regresses to bare digits.
    pub fn set_candidates(&self, chat_id: &str, candidates: &[MentionCandidate]) {
        let mut inner = self.inner.borrow_mut();
        for c in candidates {
            inner
                .names_by_jid
                .insert(c.jid.clone(), c.display_name.clone());
            let digits = c.jid.split('@').next().unwrap_or(&c.jid).to_string();
            if !digits.is_empty() {
                inner.names_by_digits.insert(digits, c.display_name.clone());
            }
            // Also key by `phone` directly — group `participants_json`
            // sometimes embeds the LID form on `jid` while the wire
            // mention uses the phone digits, so we want both
            // resolved to the same name.
            if !c.phone.is_empty() {
                inner
                    .names_by_digits
                    .insert(c.phone.clone(), c.display_name.clone());
            }
        }
        inner.by_chat.insert(chat_id.to_string(), candidates.to_vec());
    }

    /// Snapshot of the candidate list for `chat_id`. Cheap clone —
    /// the popover takes a copy each time it filters so the
    /// inventory stays free for concurrent updates.
    pub fn candidates_for(&self, chat_id: &str) -> Vec<MentionCandidate> {
        self.inner
            .borrow()
            .by_chat
            .get(chat_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Resolve a `@<digits>` mention to a display name. Returns
    /// `None` if the digits don't match any cached candidate, in
    /// which case the renderer keeps the raw digits.
    pub fn name_for_digits(&self, digits: &str) -> Option<String> {
        self.inner.borrow().names_by_digits.get(digits).cloned()
    }
}
