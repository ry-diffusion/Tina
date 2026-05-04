// Public Input / Output / Init / constants for the `ChatTab` component.

use tina_core::WaIdentity;
use tina_db::MessageRow;

use crate::inventory::{AvatarInventory, MediaInventory};

/// Window in seconds within which two messages from the same sender are
/// rendered as a collapsed run (no avatar/header on the second). Mirrors
/// Dissent's 10-minute grouping window.
pub const COLLAPSE_WINDOW_SECS: i64 = 10 * 60;

#[derive(Debug)]
pub enum ChatTabInput {
    SetMeta {
        name: String,
        kind: String,
    },
    Reset(Vec<MessageRow>),
    Append(Vec<MessageRow>),
    Send,
    MediaReady {
        message_ids: Vec<String>,
        path: String,
        mimetype: Option<String>,
    },
    MediaFailed(String),
    RequestMediaDownload(String),
    /// Reply quote-header was clicked — scroll the thread to the
    /// cited message, briefly highlighting it. No-op when the
    /// target isn't currently in the factory.
    JumpToMessage(String),
    /// VAdjustment crossed the load-more threshold. Internal trigger.
    NearTop,
    /// User scrolled back to the bottom — opportunity to prune the top
    /// of the factory if it grew past the soft cap.
    NearBottom,
    /// Deferred trim of the newest rows after a `PrependOlder` settled.
    /// Symmetric counterpart to NearBottom's top-prune: fast scroll-up
    /// stacks 50-row pages on top forever, so we lop off the back when
    /// the factory blows past the cap. Posted from `handle_prepend_older`
    /// via an idle callback so the scroll-position restore runs first.
    TrimBottom,
    /// Older page came back from the worker. `reached_top = true` means
    /// the worker returned fewer rows than requested → we've loaded the
    /// entire history; stop trying.
    PrependOlder {
        messages: Vec<MessageRow>,
        reached_top: bool,
    },
    /// User switched into this tab. Force sticky-bottom + a deferred
    /// scroll so the freshly-realised page lands on the latest message.
    StickToBottom,
    /// Worker resolved a profile picture — apply it to every message
    /// row whose sender JID matches.
    AvatarReady { jid: WaIdentity, path: String },
    /// Identity arrived (or changed) — back-fill `sender_jid` on
    /// existing from_me rows and apply the cached avatar to them.
    SetUserJid(Option<WaIdentity>),
}

#[derive(Debug)]
pub enum ChatTabOutput {
    Send { chat_id: String, text: String },
    Close { chat_id: String },
    RequestMediaDownload(String),
    RequestLoadOlder { chat_id: String, before_ts: i64 },
    /// Ask the worker to fetch a sender's profile picture. Deduped at
    /// the tab level so we only round-trip per JID once.
    RequestFetchAvatar(WaIdentity),
}

pub struct ChatTabInit {
    pub chat_id: String,
    pub name: String,
    pub kind: String,
    pub initial: Vec<MessageRow>,
    pub avatars: AvatarInventory,
    pub media: MediaInventory,
    /// Signed-in user's JID, used to override `sender_jid` for `from_me`
    /// rows (which the DB stores with `sender_contact_id = NULL`, so the
    /// JOIN can't resolve a JID for them).
    pub user_jid: Option<WaIdentity>,
}
