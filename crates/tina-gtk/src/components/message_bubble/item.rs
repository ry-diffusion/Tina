// `MessageItem` — the data model passed to the `MessageBubble` factory.
// Carries everything the view needs to paint one row, with derived
// helpers for header markup, media kind, etc.

use adw::prelude::*;
use tina_db::MessageRow;

use crate::time::format_message_time;

use super::format::{build_media_summary, glib_markup_escape};

#[derive(Debug, Clone)]
pub struct MessageItem {
    pub id: String,
    pub from_me: bool,
    pub sender_name: String,
    pub sender_jid: Option<String>,
    pub sender_avatar_path: Option<String>,
    /// Chat kind (`dm`, `group`, `newsletter`, …) — used by the
    /// header to swap "Unknown" for the channel's own name when the
    /// row sits inside a newsletter (every post comes from the
    /// channel itself, individual senders aren't exposed).
    pub chat_kind: String,
    /// Display name of the enclosing chat. Only meaningful for
    /// newsletters; `None` for everything else so the regular sender
    /// resolution stays in charge.
    pub chat_display_name: Option<String>,
    /// Cached avatar of the enclosing chat. Same scope as
    /// `chat_display_name`.
    pub chat_avatar_path: Option<String>,
    /// `true` when the previous row in the thread had the same sender
    /// within ~10 minutes. Suppresses the avatar/header; only the
    /// content (and a hover-only timestamp) is shown.
    pub is_collapsed: bool,
    pub content: String,
    pub message_type: String,
    pub timestamp: String,
    pub timestamp_unix: i64,
    pub media_summary: String,
    pub media_mimetype: Option<String>,
    pub media_size_bytes: Option<i64>,
    pub media_duration_secs: Option<i64>,
    pub media_path: Option<String>,
    pub media_status: String,
    pub media_filename: Option<String>,
    /// Inline preview (JPEG/PNG bytes) for image/video/sticker/document.
    /// Rendered as a `gtk::Picture` placeholder while the user hasn't
    /// triggered the full download yet — much nicer than the generic
    /// icon. Decoded into a `gdk::Texture` lazily by the view.
    pub thumbnail: Option<Vec<u8>>,
    /// Reply / quoted-message metadata. `quoted_message_id` being
    /// `Some` is the type-level signal that the reply header should
    /// be rendered.
    pub quoted_message_id: Option<String>,
    pub quoted_sender_id: Option<String>,
    pub quoted_sender_name: Option<String>,
    pub quoted_preview: Option<String>,
    /// JIDs mentioned in the message text, decoded from the
    /// `mentions_json` column. Each entry is a JID — the renderer
    /// pulls the user portion (digits) and substitutes `@<digits>`
    /// in the content with a styled span.
    pub mentions: Vec<String>,
}

impl MessageItem {
    pub fn from_row(row: &MessageRow, is_collapsed: bool) -> Self {
        let content = row.content.clone().unwrap_or_default();
        let display = if content.is_empty() {
            format!("[{}]", row.message_type)
        } else {
            content
        };
        Self {
            id: row.message_id.clone(),
            from_me: row.is_from_me,
            sender_name: crate::format::format_jid_or_phone(
                &row.sender_name.clone().unwrap_or_default(),
            ),
            sender_jid: row.sender_jid.clone(),
            sender_avatar_path: row.sender_avatar_path.clone(),
            chat_kind: String::new(),
            chat_display_name: None,
            chat_avatar_path: None,
            is_collapsed,
            content: display,
            message_type: row.message_type.clone(),
            timestamp: format_message_time(row.timestamp),
            timestamp_unix: row.timestamp,
            media_summary: build_media_summary(row),
            media_mimetype: row.media_mimetype.clone(),
            media_size_bytes: row.media_size_bytes,
            media_duration_secs: row.media_duration_secs,
            media_path: row.media_path.clone(),
            media_status: row.media_status.clone(),
            media_filename: row.media_filename.clone(),
            thumbnail: row.media_thumbnail.clone(),
            quoted_message_id: row.quoted_message_id.clone(),
            quoted_sender_id: row.quoted_sender_id.clone(),
            quoted_sender_name: row.quoted_sender_name.clone(),
            quoted_preview: row.quoted_preview.clone(),
            mentions: row
                .mentions_json
                .as_deref()
                .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
                .unwrap_or_default(),
        }
    }

    pub(super) fn has_reply(&self) -> bool {
        self.quoted_message_id
            .as_deref()
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    }

    /// Display name for the citation header. Prefers the contact
    /// name resolved via JOIN; falls back to the JID short form, then
    /// to "Unknown" when the proto didn't carry a participant.
    pub(super) fn quoted_sender_label(&self) -> String {
        if let Some(name) = self.quoted_sender_name.as_deref()
            && !name.is_empty()
        {
            return crate::format::format_jid_or_phone(name);
        }
        match self.quoted_sender_id.as_deref() {
            Some(s) if !s.is_empty() => {
                tina_core::WaIdentity::parse(s).display_short().to_string()
            }
            _ => "Unknown".to_string(),
        }
    }

    pub(super) fn quoted_preview_text(&self) -> &str {
        self.quoted_preview
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("Replied message")
    }

    pub(super) fn thumbnail_paintable(&self) -> Option<gtk::gdk::Paintable> {
        let bytes = self.thumbnail.as_ref()?;
        if bytes.is_empty() {
            return None;
        }
        gtk::gdk::Texture::from_bytes(&gtk::glib::Bytes::from(bytes.as_slice()))
            .ok()
            .map(|t| t.upcast::<gtk::gdk::Paintable>())
    }

    pub(super) fn is_media(&self) -> bool {
        matches!(
            self.message_type.as_str(),
            "image" | "audio" | "video" | "sticker" | "document"
        )
    }

    pub(super) fn is_visual_media(&self) -> bool {
        matches!(self.message_type.as_str(), "image" | "video" | "sticker")
    }

    pub(super) fn media_kind_label(&self) -> &'static str {
        match self.message_type.as_str() {
            "image" => "Image",
            "audio" => "Voice / Audio",
            "video" => "Video",
            "sticker" => "Sticker",
            "document" => "Document",
            _ => "Attachment",
        }
    }

    pub(super) fn placeholder_icon(&self) -> &'static str {
        match self.message_type.as_str() {
            "image" => "image-x-generic-symbolic",
            "audio" => "audio-x-generic-symbolic",
            "video" => "video-x-generic-symbolic",
            "sticker" => "emoji-symbols-symbolic",
            "document" => "text-x-generic-symbolic",
            _ => "mail-attachment-symbolic",
        }
    }

    pub(super) fn caption(&self) -> Option<&str> {
        if self.content.starts_with('[') && self.content.ends_with(']') {
            None
        } else {
            Some(&self.content)
        }
    }

    pub(super) fn has_local_file(&self) -> bool {
        self.media_path
            .as_deref()
            .map(|p| !p.is_empty())
            .unwrap_or(false)
    }

    /// Display name shown above the bubble. Newsletters always speak
    /// as the channel itself, so we substitute the chat's own name +
    /// avatar instead of letting "Unknown" leak through when the
    /// per-message sender lookup fails.
    pub(super) fn display_sender_name(&self) -> &str {
        if self.from_me {
            return "You";
        }
        if self.chat_kind == "newsletter"
            && let Some(n) = self.chat_display_name.as_deref()
            && !n.is_empty()
        {
            return n;
        }
        if self.sender_name.is_empty() {
            "Unknown"
        } else {
            self.sender_name.as_str()
        }
    }

    /// Avatar path used by the bubble's gutter. Newsletters reuse the
    /// chat's own avatar — same fallback path as the sender name.
    pub(super) fn display_avatar_path(&self) -> Option<&str> {
        if self.from_me {
            return self.sender_avatar_path.as_deref();
        }
        if self.chat_kind == "newsletter" {
            if let Some(p) = self.chat_avatar_path.as_deref() {
                return Some(p);
            }
        }
        self.sender_avatar_path.as_deref()
    }

    pub(super) fn header_markup(&self) -> String {
        format!(
            "<b>{}</b>  <span alpha=\"60%\" size=\"small\">{}</span>",
            glib_markup_escape(self.display_sender_name()),
            glib_markup_escape(&self.timestamp),
        )
    }

    pub(super) fn short_timestamp(&self) -> &str {
        &self.timestamp
    }
}
