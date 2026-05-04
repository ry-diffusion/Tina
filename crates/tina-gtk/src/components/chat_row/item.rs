// `ChatRowItem` — the data passed to the typed list view. Implements
// `Ord` so the SortListModel knows how to order rows
// (pinned → active → newest → alpha).

use std::cmp::Ordering;

use tina_db::ChatRow;

use crate::inventory::AvatarInventory;
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
    pub fn from_row(row: &ChatRow, avatars: AvatarInventory) -> Self {
        let preview = build_preview(row);
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
    use crate::wa_id::WaIdentity;
    let name = row.name.trim();
    if !name.is_empty() && !is_raw_jid(name) {
        return name.to_string();
    }
    // Either the name is empty or it's a raw JID echoed back from
    // the chat_id fallback. Fall through to WaIdentity's display.
    WaIdentity::parse(&row.chat_id).display()
}

/// Recognise the raw `<user>@<server>` form so we don't print it as
/// the chat name. Anything that parses to a non-`Unknown` identity
/// counts.
fn is_raw_jid(s: &str) -> bool {
    use crate::wa_id::WaIdentity;
    !matches!(WaIdentity::parse(s), WaIdentity::Unknown(_))
}

fn build_preview(row: &ChatRow) -> String {
    let raw = row.last_message_preview.clone().unwrap_or_default();
    let mtype = row.last_message_type.as_deref().unwrap_or("");
    let preview = match mtype {
        "image" => "📷 Foto".to_string(),
        "audio" => match row.last_message_duration_secs {
            Some(s) if s > 0 => format!("🎤 {}:{:02}", s / 60, s % 60),
            _ => "🎤 Mensagem de voz".to_string(),
        },
        "video" => match row.last_message_duration_secs {
            Some(s) if s > 0 => format!("🎬 Vídeo {}:{:02}", s / 60, s % 60),
            _ => "🎬 Vídeo".to_string(),
        },
        "sticker" => "🎴 Figurinha".to_string(),
        "document" => "📄 Documento".to_string(),
        "contact" => "👤 Contato".to_string(),
        "location" => "📍 Localização".to_string(),
        _ => match raw.as_str() {
            "[Image]" => "📷 Foto".to_string(),
            "[Audio]" => "🎤 Mensagem de voz".to_string(),
            "[Video]" => "🎬 Vídeo".to_string(),
            "[Sticker]" => "🎴 Figurinha".to_string(),
            "[Document]" => "📄 Documento".to_string(),
            "[Contact]" => "👤 Contato".to_string(),
            "[Location]" => "📍 Localização".to_string(),
            "[Live Location]" => "📍 Localização em tempo real".to_string(),
            other => other.to_string(),
        },
    };
    if preview.is_empty() {
        return preview;
    }
    if row.last_message_from_me {
        return format!("Você: {preview}");
    }
    // Group / newsletter rows prefix the sender's name (matches
    // WhatsApp's preview format). DMs already have the sender as
    // the chat's own name, so no point repeating it. Status rows
    // are aggregated in their own tab and don't carry a useful
    // sender at the chat level.
    if matches!(row.kind.as_str(), "group" | "newsletter" | "broadcast")
        && let Some(sender) = row.last_sender_name.as_deref()
        && !sender.is_empty()
    {
        let short = short_sender_name(sender);
        return format!("{short}: {preview}");
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
