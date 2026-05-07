// Commands the UI can send to the worker thread, plus the
// `ServiceHandle` used by the rest of the app to send them.

use tina_core::WaIdentity;
use tokio::sync::mpsc;
use tracing::error;

#[derive(Debug)]
pub enum Cmd {
    /// Boot: list accounts, auto-create on empty, start chosen account.
    Initialize,
    /// Re-emits the latest snapshot of chats for the active account.
    LoadChats,
    /// Compute the Status authors list (everyone who's posted to
    /// `status@broadcast`) and push it back as
    /// `AppMsg::StatusAuthorsUpserted`. Triggered by the user
    /// switching to the Status tab; not part of the regular
    /// `ChatsUpserted` flow because the rows are aggregated from
    /// messages, not stored as chat rows.
    LoadStatuses,
    /// Fetch the recent `status@broadcast` posts of one sender and
    /// push them back as `AppMsg::ShowStoriesViewer` so the
    /// dispatcher can open the carousel.
    OpenStatusAuthor {
        sender_jid: WaIdentity,
        name: String,
    },
    /// Open (or re-load) a chat: fetches metadata + last 200 messages,
    /// adds the chat to the worker's open-tab set, and emits
    /// `AppMsg::ChatOpened`. Membership in the set is what gates whether
    /// new sync rows for that chat get pushed to the UI as
    /// `MessagesAppended` (vs silently merged into the DB).
    OpenChat(String),
    /// UI closed a tab — drop the chat from the worker's open-tab set so
    /// future sync rows for it stop firing `MessagesAppended`.
    CloseChat(String),
    /// Send a plain-text message to a chat. `mentioned_jids` is
    /// piped through to `IpcCommand::SendMessage` so whatsmeow
    /// attaches a `contextInfo.MentionedJID` array — empty for the
    /// common path of unmentioned text.
    SendText {
        chat_id: String,
        text: String,
        mentioned_jids: Vec<String>,
    },
    /// Send a media message (image / video / audio / voice / sticker
    /// / document). `path` is read by the Go side; the worker just
    /// forwards through IPC. `caption` is honoured for image / video
    /// / document and ignored otherwise.
    SendMedia {
        chat_id: String,
        kind: tina_core::MediaKind,
        path: String,
        caption: Option<String>,
        mimetype: Option<String>,
        filename: Option<String>,
        /// Client-side sentinel id of the optimistic echo. Passed
        /// through so the worker can correlate pending sends with
        /// their local placeholder for failure marking / dedup.
        local_id: Option<String>,
    },
    /// Trigger reconcile (whatsmeow → tina).
    Repair,
    /// Trigger an async media download for a specific message.
    DownloadMedia { message_id: String },
    /// Fetch a profile picture for the given JID (chat_id, contact_id,
    /// etc — anything that resolves through the worker's aliases).
    FetchAvatar { jid: WaIdentity },
    /// Fetch an avatar directly from a known URL (for @newsletter JIDs
    /// where GetProfilePictureInfo returns 504).
    FetchAvatarFromURL { jid: WaIdentity, url: String },
    /// Re-pull a chat's display name + avatar (newsletters / groups).
    /// Triggered by `ChatInventory` when it sees a render miss.
    RefreshChat { chat_jid: WaIdentity },
    /// Lazy-load older messages (page back). The UI passes the timestamp
    /// of its currently-oldest row; the worker returns the next batch
    /// strictly older than that.
    LoadOlder {
        chat_id: String,
        before_ts: i64,
        limit: i64,
    },
    /// Lazy-load newer messages (page forward). Symmetric counterpart
    /// to `LoadOlder`: the UI passes the timestamp of its currently-
    /// newest row, and the worker returns the next batch strictly
    /// newer than that. Triggered when the user scrolls past the
    /// factory's last row after the soft-cap trimmed the tail.
    LoadNewer {
        chat_id: String,
        after_ts: i64,
        limit: i64,
    },
    /// Persist a chat's pinned flag. After the DB write the UI will see
    /// the change on the next `LoadChats` / reconcile push.
    SetChatPinned { chat_id: String, pinned: bool },
    /// Logout the active account.
    Logout,
    /// Read the persisted download method + current nanachi PID and
    /// push them up as `AppMsg`s for the settings dialog to display.
    /// Called when the user opens the preferences pane.
    LoadPreferences,
    /// Persist the user's download-method preference (settings dialog).
    /// The worker writes it to the `settings` table; consumers read on
    /// demand via `worker.get_setting`.
    SetDownloadMethod(crate::components::settings::DownloadMethod),
    /// Wipe the on-disk media cache (`~/.local/share/tina/media/`)
    /// and null out `messages.media_path`. The next access re-fetches.
    ClearMediaCache,
    /// Pull the most recent received stickers (deduped by SHA-256)
    /// for the active account so the sticker-picker popover can
    /// render thumbnails. `chat_id` round-trips with the result so
    /// the right `ChatTab` repaints — necessary because every open
    /// tab can launch its own picker.
    LoadStickers { chat_id: String, limit: i64 },
    /// Send Read receipts for a batch of incoming messages in one
    /// chat. The UI throttles its own emission so we don't fire
    /// once per row during fast scroll.
    MarkChatRead {
        chat_id: String,
        sender_jid: String,
        message_ids: Vec<String>,
    },
    /// Wipe the on-disk avatar cache + null out `chats.avatar_path`,
    /// `contacts.avatar_path`. Avatars re-fetch on next render.
    ClearAvatarCache,
    /// Resolve the `@`-mention picker candidates for a chat. Fired
    /// when a tab opens (groups only — DMs return an empty list).
    /// Result lands as `AppMsg::MentionCandidatesLoaded`.
    LoadMentionCandidates { chat_id: String },
    /// Shut down the worker thread.
    Shutdown,
}

#[derive(Clone)]
pub struct ServiceHandle {
    pub(super) tx: mpsc::UnboundedSender<Cmd>,
}

impl ServiceHandle {
    pub fn send(&self, cmd: Cmd) {
        if let Err(e) = self.tx.send(cmd) {
            error!("service tx closed: {e}");
        }
    }
}
