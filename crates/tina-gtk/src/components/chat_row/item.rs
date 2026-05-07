// `ChatRowItem` — the data passed to the typed list view. Implements
// `Ord` so the SortListModel knows how to order rows
// (pinned → active → newest → alpha).

use std::cmp::Ordering;
use crate::fl;

use tina_db::ChatRow;

use crate::inventory::{AvatarInventory, MentionInventory};
use crate::time::format_chat_timestamp;

#[derive(Clone)]
pub struct ChatRowItem {
    pub chat_id: String,
    pub kind: String,
    pub name: String,
    pub preview: String,
    pub timestamp: String,
    pub last_ts: i64,
    pub unread: i64,
    pub pinned: bool,
    pub avatar_path: Option<String>,
    /// `true` when the chat currently has a tab open in the chat area.
    /// Drives both the sort key (active chats float to the top) and the
    /// `tina-tab-open` CSS class for the visual highlight.
    pub is_active: bool,
    /// Carried so `bind` can hit the shared texture cache instead of
    /// re-decoding the avatar file every time the row scrolls into
    /// view. Cloning is just an `Rc` bump.
    pub avatars: AvatarInventory,
}

impl ChatRowItem {
    pub fn from_row(row: &ChatRow, avatars: AvatarInventory, mentions: &MentionInventory) -> Self {
        let preview = resolve_preview_mentions(&build_preview(row), mentions);
        let last_ts = row.last_message_ts.unwrap_or(0);
        Self {
            chat_id: row.chat_id.clone(),
            kind: row.kind.clone(),
            name: resolve_display_name(row),
            preview,
            timestamp: format_chat_timestamp(last_ts),
            last_ts,
            unread: row.unread_count,
            pinned: row.pinned,
            avatar_path: row.avatar_path.clone(),
            is_active: false,
            avatars,
        }
    }
}

/// Pick a row label that doesn't put a phone-formatted JID up front.
/// `format_jid_or_phone` was hard-coding `+1 1203…` for newsletter
/// JIDs because the user-part looks like an E.164 number. The
/// chat_row select clause falls back to `chat_id` when no
/// `display_name` is set, so for newsletters/groups whose metadata
/// hasn't landed yet we still see the raw JID — route through
/// `WaIdentity::display` which formats by server type.
fn resolve_display_name(row: &ChatRow) -> String {
    use crate::wa_id::{self, WaIdentity};
    if !WaIdentity::looks_like_unresolved_name(&row.name) {
        return row.name.trim().to_string();
    }
    // Either the name is empty or it's a raw JID echoed back from
    // the chat_id fallback. Fall through to the phone-aware display
    // helper (formats `+55 61 …` for phones, `Channel #abc` for
    // newsletters, etc.).
    wa_id::display(&WaIdentity::parse(&row.chat_id))
}

fn build_preview(row: &ChatRow) -> String {
    let raw = row.last_message_preview.clone().unwrap_or_default();
    let mtype = row.last_message_type.as_deref().unwrap_or("");
    let preview = match mtype {
        "image" => fl!("preview-photo"),
        "audio" => match row.last_message_duration_secs {
            Some(s) if s > 0 => fl!("preview-voice-duration",
                "min" = format!("{}", s / 60),
                "sec" = format!("{:02}", s % 60)
            ),
            _ => fl!("preview-voice-note"),
        },
        "video" => match row.last_message_duration_secs {
            Some(s) if s > 0 => fl!("preview-video-duration",
                "min" = format!("{}", s / 60),
                "sec" = format!("{:02}", s % 60)
            ),
            _ => fl!("preview-video"),
        },
        "sticker" => fl!("preview-sticker"),
        "document" => fl!("preview-document"),
        "contact" => fl!("preview-contact"),
        "location" => fl!("preview-location"),
        _ => match raw.as_str() {
            "[Image]" => fl!("preview-photo"),
            "[Audio]" => fl!("preview-voice-note"),
            "[Video]" => fl!("preview-video"),
            "[Sticker]" => fl!("preview-sticker"),
            "[Document]" => fl!("preview-document"),
            "[Contact]" => fl!("preview-contact"),
            "[Location]" => fl!("preview-location"),
            "[Live Location]" => fl!("preview-live-location"),
            other => other.to_string(),
        },
    };
    if preview.is_empty() {
        return preview;
    }
    if row.last_message_from_me {
        return fl!("preview-you", "text" = preview);
    }
    // Group / newsletter rows prefix the sender's name (matches
    // WhatsApp's preview format). DMs already have the sender as
    // the chat's own name, so no point repeating it. Status rows
    // are aggregated in their own tab and don't carry a useful
    // sender at the chat level.
    if matches!(row.kind.as_str(), "group" | "broadcast")
        && let Some(sender) = row.last_sender_name.as_deref()
        && !sender.is_empty()
    {
        let short = short_sender_name(sender);
        return fl!("preview-sender", "short" = short, "text" = preview);
    }
    preview
}

/// Trim the sender to first name (or the first 18 chars if there's
/// no whitespace). Keeps the preview line within ellipsize budget on
/// narrow sidebars; the full name still appears when the user opens
/// the chat.
fn short_sender_name(name: &str) -> String {
    if let Some(first) = name.split_whitespace().next() {
        if first.chars().count() <= 18 {
            return first.to_string();
        }
    }
    let trimmed: String = name.chars().take(18).collect();
    if trimmed.chars().count() < name.chars().count() {
        format!("{trimmed}…")
    } else {
        trimmed
    }
}

// Sort order: pinned first → active (currently in a tab) next →
// newest next → alpha last. Reverse-compare bools so `true` floats
// first. The pinned-before-active ordering matches what users expect
// from messengers like Telegram/WhatsApp — explicit pins outrank
// transient "I happen to be chatting here right now".
impl ChatRowItem {
    /// True if replacing `prev` with `self` would change either the
    /// sort key or any visible field. Used to skip no-op `replace_at`
    /// calls during reconnect batches — each skipped replace saves two
    /// `items_changed` signals to the SortListModel.
    pub fn differs_from(&self, prev: &ChatRowItem) -> bool {
        self.last_ts != prev.last_ts
            || self.pinned != prev.pinned
            || self.name != prev.name
            || self.preview != prev.preview
            || self.unread != prev.unread
            || self.avatar_path != prev.avatar_path
    }
}

impl Ord for ChatRowItem {
    fn cmp(&self, other: &Self) -> Ordering {
        match other.pinned.cmp(&self.pinned) {
            Ordering::Equal => {}
            o => return o,
        }
        match other.is_active.cmp(&self.is_active) {
            Ordering::Equal => {}
            o => return o,
        }
        match other.last_ts.cmp(&self.last_ts) {
            Ordering::Equal => {}
            o => return o,
        }
        self.name.cmp(&other.name)
    }
}
impl PartialOrd for ChatRowItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for ChatRowItem {
    fn eq(&self, other: &Self) -> bool {
        self.chat_id == other.chat_id
    }
}
impl Eq for ChatRowItem {}

/// Scan `text` for `@<digits>` patterns and replace with the resolved
/// display name from `mentions` when available. Runs once at
/// `from_row` time so the preview string is always up-to-date with
/// whatever names the inventory currently holds.
fn resolve_preview_mentions(text: &str, mentions: &MentionInventory) -> String {
    if !text.contains('@') {
        return text.to_string();
    }
    let mut result = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        if c == '@' {
            let mut digits = String::new();
            while let Some(&(_, d)) = chars.peek() {
                if d.is_ascii_digit() {
                    digits.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            result.push('@');
            if digits.is_empty() {
                // bare `@` with no digits — keep as-is
            } else if let Some(name) = mentions.name_for_digits(&digits) {
                result.push_str(&name);
            } else {
                result.push_str(&digits);
            }
        } else {
            result.push(c);
        }
    }
    result
}
