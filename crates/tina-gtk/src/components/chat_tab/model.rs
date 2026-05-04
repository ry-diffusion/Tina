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
    pub(super) user_jid: Option<tina_core::WaIdentity>,
    pub(super) scroll: Option<gtk::ScrolledWindow>,
    pub(super) seen_message_ids: HashSet<String>,
    pub(super) last_send: Option<(String, std::time::Instant)>,
    pub(super) oldest_ts: Option<i64>,
    pub(super) loading_older: bool,
    pub(super) reached_top: bool,
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

    pub(super) fn read_only_label(&self) -> &'static str {
        match self.kind.as_str() {
            "newsletter" => "You can't reply to channels.",
            "status" => "Status updates can't be answered here.",
            "broadcast" => "Broadcast lists are read-only.",
            _ => "Read-only.",
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
