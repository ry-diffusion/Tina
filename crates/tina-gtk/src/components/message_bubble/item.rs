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
        }
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

    pub(super) fn header_markup(&self) -> String {
        let name = if self.from_me {
            "You"
        } else if self.sender_name.is_empty() {
            "Unknown"
        } else {
            self.sender_name.as_str()
        };
        format!(
            "<b>{}</b>  <span alpha=\"60%\" size=\"small\">{}</span>",
            glib_markup_escape(name),
            glib_markup_escape(&self.timestamp),
        )
    }

    pub(super) fn short_timestamp(&self) -> &str {
        &self.timestamp
    }
}
