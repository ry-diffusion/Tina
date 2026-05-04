// Commands the UI can send to the worker thread, plus the
// `ServiceHandle` used by the rest of the app to send them.

use tokio::sync::mpsc;
use tracing::error;

#[derive(Debug)]
pub enum Cmd {
    /// Boot: list accounts, auto-create on empty, start chosen account.
    Initialize,
    /// Re-emits the latest snapshot of chats for the active account.
    LoadChats,
    /// Open (or re-load) a chat: fetches metadata + last 200 messages,
    /// adds the chat to the worker's open-tab set, and emits
    /// `AppMsg::ChatOpened`. Membership in the set is what gates whether
    /// new sync rows for that chat get pushed to the UI as
    /// `MessagesAppended` (vs silently merged into the DB).
    OpenChat(String),
    /// UI closed a tab — drop the chat from the worker's open-tab set so
    /// future sync rows for it stop firing `MessagesAppended`.
    CloseChat(String),
    /// Send a plain-text message to a chat.
    SendText { chat_id: String, text: String },
    /// Trigger reconcile (whatsmeow → tina).
    Repair,
    /// Trigger an async media download for a specific message.
    DownloadMedia { message_id: String },
    /// Fetch a profile picture for the given JID (chat_id, contact_id,
    /// etc — anything that resolves through the worker's aliases).
    FetchAvatar { jid: String },
    /// Lazy-load older messages (page back). The UI passes the timestamp
    /// of its currently-oldest row; the worker returns the next batch
    /// strictly older than that.
    LoadOlder {
        chat_id: String,
        before_ts: i64,
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
    /// Wipe the on-disk avatar cache + null out `chats.avatar_path`,
    /// `contacts.avatar_path`. Avatars re-fetch on next render.
    ClearAvatarCache,
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
