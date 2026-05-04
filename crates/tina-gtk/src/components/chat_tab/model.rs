// State carried by the `ChatTab` component. Fields are split across two
// sibling files (`actions.rs` for the message-handler bodies, `view.rs`
// for the relm4 view + update dispatcher) — this file owns the struct
// definition + the small read-only helpers everyone reuses.

use std::cell::Cell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use relm4::factory::FactoryVecDeque;

use crate::components::message_bubble::MessageBubble;
use crate::inventory::{AvatarInventory, MediaInventory};

pub struct ChatTab {
    pub(super) chat_id: String,
    pub(super) name: String,
    pub(super) kind: String,
    pub(super) messages: FactoryVecDeque<MessageBubble>,
    pub(super) composer_buffer: gtk::EntryBuffer,
    pub(super) avatars: AvatarInventory,
    pub(super) media: MediaInventory,
    pub(super) user_jid: Option<String>,
    pub(super) scroll: Option<gtk::ScrolledWindow>,
    pub(super) seen_message_ids: HashSet<String>,
    pub(super) last_send: Option<(String, std::time::Instant)>,
    pub(super) oldest_ts: Option<i64>,
    pub(super) loading_older: bool,
    pub(super) reached_top: bool,
    pub(super) pending_echoes: HashMap<String, VecDeque<String>>,
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
}

impl ChatTab {
    pub fn chat_id(&self) -> &str {
        &self.chat_id
    }

    /// `(sender_key, timestamp)` for the trailing item in the factory.
    /// Used to seed collapse decisions for incoming Append batches and
    /// optimistic Send echoes — the factory is the single source of
    /// truth for "what was just rendered", so this avoids the state
    /// drift that creeps in when a separate `last_sender` field is
    /// kept in sync across many code paths.
    pub(super) fn factory_tail_cursor(&self) -> (Option<String>, Option<i64>) {
        let Some(last) = self.messages.back() else {
            return (None, None);
        };
        let key = if last.item.from_me {
            "\0me".to_string()
        } else {
            last.item.sender_name.clone()
        };
        (Some(key), Some(last.item.timestamp_unix))
    }
}
