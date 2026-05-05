// Helpers for building MessageItems and computing collapse state from
// raw `MessageRow`s. Pure data shaping — no widget side effects, just
// some inventory lookups and an out-channel for "we want this avatar
// fetched".

use tina_db::MessageRow;

use crate::components::message_bubble::MessageItem;
use crate::inventory::{AvatarInventory, MediaInventory, MentionInventory};

use super::messages::COLLAPSE_WINDOW_SECS;

pub fn sender_key(row: &MessageRow) -> String {
    if row.is_from_me {
        "\0me".to_string()
    } else {
        row.sender_name.clone().unwrap_or_default()
    }
}

/// Optional context about the chat the row sits in. Threaded into
/// `build_item` so newsletter posts can render the channel's name +
/// avatar (the per-message sender for a newsletter is always the
/// channel itself; resolving it through the contacts table dead-ends
/// at "Unknown" because newsletters aren't contacts).
#[derive(Default, Clone)]
pub struct ChatContext {
    pub kind: String,
    pub display_name: Option<String>,
    pub avatar_path: Option<String>,
}

/// Build a `MessageItem` from a row, hydrating avatar + media state from
/// the shared inventories and emitting fetch requests as needed via
/// `request_fetch_avatar`.
pub fn build_item(
    row: &MessageRow,
    is_collapsed: bool,
    avatars: &AvatarInventory,
    media: &MediaInventory,
    mentions: &MentionInventory,
    user_jid: Option<&str>,
    chat: &ChatContext,
    request_fetch_avatar: &mut impl FnMut(String),
) -> MessageItem {
    let mut item = MessageItem::from_row(row, is_collapsed);

    item.chat_kind = chat.kind.clone();
    item.chat_display_name = chat.display_name.clone();
    item.chat_avatar_path = chat.avatar_path.clone();
    // Resolve mention chips against the live inventory. Stale or
    // unresolved mentions stay as bare `@<digits>` until the next
    // rebuild — the next batch fetch / scrollback page will pick
    // up names freshly populated by `set_candidates`.
    item.resolve_mentions(|digits| mentions.name_for_digits(digits));

    // For from_me messages the DB stores sender_contact_id=NULL (we
    // never auto-register a contact for the signed-in user), so the
    // JOIN can't resolve a sender_jid — override with the known
    // identity here so the avatar lookup matches against the same key
    // the inventory was populated with via SetIdentity.
    if item.from_me && item.sender_jid.is_none() {
        item.sender_jid = user_jid.map(|s| s.to_string());
    }

    // Avatar: prefer the live inventory over the JOIN'd snapshot.
    if let Some(jid) = item.sender_jid.clone()
        && !jid.is_empty() {
            if let Some(p) = avatars.get(&jid) {
                item.sender_avatar_path = Some(p);
            } else if avatars.needs_fetch(&jid) {
                request_fetch_avatar(jid);
            }
        }

    // Media: in-flight states + recent successes live here, not in the
    // DB. Override the row's snapshot when the inventory has fresher info.
    if let Some(state) = media.get(&item.id) {
        if state.path.is_some() {
            item.media_path = state.path;
        }
        if !state.status.is_empty() {
            item.media_status = state.status;
        }
        if item.media_mimetype.is_none() && state.mimetype.is_some() {
            item.media_mimetype = state.mimetype;
        }
    }

    item
}

/// Decide whether `row` should be rendered as a collapsed continuation
/// of the previous row in the thread. Updates the trailing-state cursor
/// in place so callers can iterate without manual bookkeeping.
pub fn collapse_against(
    row: &MessageRow,
    last_sender: &mut Option<String>,
    last_ts: &mut Option<i64>,
) -> bool {
    let key = sender_key(row);
    let collapsed = match (last_sender.as_deref(), *last_ts) {
        (Some(prev_sender), Some(prev_ts)) => {
            prev_sender == key && row.timestamp.saturating_sub(prev_ts) <= COLLAPSE_WINDOW_SECS
        }
        _ => false,
    };
    *last_sender = Some(key);
    *last_ts = Some(row.timestamp);
    collapsed
}

/// True when `row`'s local-time day differs from `last_day`'s. Updates
/// `last_day` in place. The first row in any iteration always returns
/// `true` (no previous day to compare against), which is what the chat
/// thread wants — show the date pill at the top of the loaded window.
/// Mirrors Fractal's day-divider insertion logic from
/// `room_history::timeline::update_items_headers`, scoped down to the
/// inline-pill rendering Tina uses.
pub fn day_flips(row: &MessageRow, last_day: &mut Option<String>) -> bool {
    let key = crate::time::local_day_key(row.timestamp);
    if key.is_empty() {
        return false;
    }
    let flipped = last_day.as_deref() != Some(key.as_str());
    *last_day = Some(key);
    flipped
}
