// `MessageItem` — the data model passed to the `MessageBubble` factory.
// Carries everything the view needs to paint one row, with derived
// helpers for header markup, media kind, etc.

use adw::prelude::*;
use crate::fl;
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
    /// `true` when this row is the first message of its local-time
    /// day in the loaded window. Drives the day-divider pill rendered
    /// at the top of the bubble — same idea as Fractal's
    /// `VirtualItemKind::DayDivider`, but inlined into the row to
    /// avoid restructuring the FactoryVecDeque around a polymorphic
    /// item type.
    pub is_first_of_day: bool,
    /// `Today` / `Yesterday` / weekday / full date — populated when
    /// `is_first_of_day` is `true`, empty otherwise. Computed at
    /// `build_item` time so the bubble's render pass doesn't redo
    /// the chrono cascade on every `#[watch]` tick.
    pub day_label: String,
    pub content: String,
    pub message_type: String,
    pub timestamp: String,
    /// `HH:MM` only — used by the collapsed-row hover-timestamp slot
    /// which has 56px of gutter to fit into. The fuller `timestamp`
    /// (which can be `04/05 22:20`) overflows the gutter and pushes
    /// the right column out of alignment with the cozy rows above
    /// it. Computed once at build time.
    pub short_time: String,
    pub timestamp_unix: i64,
    pub media_summary: String,
    pub media_mimetype: Option<String>,
    pub media_size_bytes: Option<i64>,
    /// Source dimensions reported by the proto for image / sticker /
    /// video. Used by `TinaMessageMedia::measure` to lay out the
    /// row at the EXPECTED final size while the proto thumbnail
    /// (which is much smaller) is shown — without these the
    /// placeholder would render at ~100×150 (the thumbnail's
    /// intrinsic size) and the row would jump when the full file
    /// arrives. Discord-style: max 400×400 on the long axis.
    pub media_width: Option<i32>,
    pub media_height: Option<i32>,
    pub media_duration_secs: Option<i64>,
    pub media_path: Option<String>,
    pub media_status: String,
    pub media_filename: Option<String>,
    /// Lower-hex SHA-256 of the cleartext media. Used by the
    /// optimistic-echo dedup logic to match a local placeholder
    /// against the real row when the worker echoes it back.
    pub media_sha256: Option<String>,
    /// Delivery status for outgoing rows. One of:
    /// `pending` | `sent` | `delivered` | `read` | `played` |
    /// `failed`. Default is `sent` for any incoming history-sync
    /// row (we don't track receipts retroactively); local
    /// optimistic echoes start at `pending` and flip as receipts
    /// arrive.
    pub delivery_status: String,
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
    /// Mentions decoded from `mentions_json` and pre-resolved
    /// against the active `MentionInventory`. Each entry is a
    /// `(digits, optional_name)` pair — the renderer scans the
    /// content for `@<digits>` and emits `@Name` (or falls back to
    /// the bare digits when the inventory had no entry yet).
    /// Resolved at build_item time rather than at render time so
    /// the factory only re-runs the lookup when a row is rebuilt.
    pub mentions: Vec<(String, Option<String>)>,
    /// Cached Pango markup for the content/caption. Pre-rendered
    /// from `wa_markdown_to_pango` + `apply_mentions_pango_resolved`
    /// once and reused across every `#[watch]` re-evaluation in the
    /// view! macro. Without it the conversion ran for every model
    /// update (any media-status flip / avatar arrival anywhere in
    /// the row), which on a chat with 100+ rows showed up as visible
    /// CPU drag. The string is immutable after build — neither media
    /// nor avatar inputs touch `content` or `mentions`.
    pub cached_markup: String,
}

impl MessageItem {
    pub fn from_row(row: &MessageRow, is_collapsed: bool) -> Self {
        let content = row.content.clone().unwrap_or_default();
        let display = if content.is_empty() {
            format!("[{}]", row.message_type)
        } else {
            content
        };
        let mut item = Self {
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
            is_first_of_day: false,
            day_label: String::new(),
            content: display,
            message_type: row.message_type.clone(),
            timestamp: format_message_time(row.timestamp),
            short_time: crate::time::format_short_time(row.timestamp),
            timestamp_unix: row.timestamp,
            media_summary: build_media_summary(row),
            media_mimetype: row.media_mimetype.clone(),
            media_size_bytes: row.media_size_bytes,
            media_width: row.media_width.and_then(|v| i32::try_from(v).ok()),
            media_height: row.media_height.and_then(|v| i32::try_from(v).ok()),
            media_duration_secs: row.media_duration_secs,
            media_path: row.media_path.clone(),
            media_status: row.media_status.clone(),
            media_filename: row.media_filename.clone(),
            media_sha256: row.media_sha256.clone(),
            delivery_status: row.delivery_status.clone(),
            thumbnail: row.media_thumbnail.clone(),
            quoted_message_id: row.quoted_message_id.clone(),
            quoted_sender_id: row.quoted_sender_id.clone(),
            quoted_sender_name: row.quoted_sender_name.clone(),
            quoted_preview: row.quoted_preview.clone(),
            // No name resolution at this stage — the build_item
            // helper layered above this calls `resolve_mentions`
            // with the live `MentionInventory`. Default to the raw
            // digit form so direct callers (tests, isolated
            // construction) still see the fallback chip.
            mentions: row
                .mentions_json
                .as_deref()
                .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
                .unwrap_or_default()
                .into_iter()
                .map(|jid| {
                    let digits = jid.split('@').next().unwrap_or(&jid).to_string();
                    (digits, None)
                })
                .collect(),
            cached_markup: String::new(),
        };
        // Pre-render once. Callers that go through `build_item` will
        // overwrite this after `resolve_mentions` populates names.
        item.recompute_markup();
        item
    }

    /// Re-resolve every mention's `name` against `resolve`.
    /// `build_item` calls this with a closure backed by the live
    /// `MentionInventory`; a row may pre-date the candidate event,
    /// in which case the inventory will return `None` and the chip
    /// stays as `@<digits>` until the next rebuild.
    pub fn resolve_mentions(&mut self, resolve: impl Fn(&str) -> Option<String>) {
        for (digits, name) in &mut self.mentions {
            *name = resolve(digits);
        }
        // Mentions changed → markup needs a re-render.
        self.recompute_markup();
    }

    /// Re-render the Pango markup cache from the current
    /// content/mentions. Cheap: a single pass through `wa_markdown`
    /// + `apply_mentions_pango_resolved`. Called on creation and
    /// whenever `mentions` is repopulated.
    pub fn recompute_markup(&mut self) {
        let body = self.caption().unwrap_or(&self.content);
        self.cached_markup = super::format::apply_mentions_pango_resolved(
            &super::format::wa_markdown_to_pango(body),
            &self.mentions,
        );
    }

    pub fn has_reply(&self) -> bool {
        self.quoted_message_id
            .as_deref()
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    }

    /// Display name for the citation header. Prefers the contact
    /// name resolved via JOIN; falls back to the JID short form, then
    /// to "Unknown" when the proto didn't carry a participant.
    pub fn quoted_sender_label(&self) -> String {
        if let Some(name) = self.quoted_sender_name.as_deref()
            && !name.is_empty()
        {
            return crate::format::format_jid_or_phone(name);
        }
        match self.quoted_sender_id.as_deref() {
            Some(s) if !s.is_empty() => {
                tina_core::WaIdentity::parse(s).display_short().to_string()
            }
            _ => fl!("sender-unknown"),
        }
    }

    pub fn quoted_preview_text(&self) -> String {
        self.quoted_preview
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| fl!("quoted-replied-message"))
    }

    pub fn thumbnail_paintable(&self) -> Option<gtk::gdk::Paintable> {
        let bytes = self.thumbnail.as_ref()?;
        if bytes.is_empty() {
            return None;
        }
        gtk::gdk::Texture::from_bytes(&gtk::glib::Bytes::from(bytes.as_slice()))
            .ok()
            .map(|t| t.upcast::<gtk::gdk::Paintable>())
    }

    pub fn is_media(&self) -> bool {
        matches!(
            self.message_type.as_str(),
            "image" | "audio" | "video" | "sticker" | "document"
        )
    }

    pub fn is_visual_media(&self) -> bool {
        matches!(self.message_type.as_str(), "image" | "video" | "sticker")
    }

    pub fn media_kind_label(&self) -> String {
        match self.message_type.as_str() {
            "image" => fl!("media-image"),
            "audio" => fl!("media-voice-audio"),
            "video" => fl!("media-video"),
            "sticker" => fl!("media-sticker"),
            "document" => fl!("media-document"),
            _ => fl!("media-attachment"),
        }
    }

    pub fn placeholder_icon(&self) -> &'static str {
        match self.message_type.as_str() {
            "image" => "image-x-generic-symbolic",
            "audio" => "audio-x-generic-symbolic",
            "video" => "video-x-generic-symbolic",
            "sticker" => "emoji-symbols-symbolic",
            "document" => "text-x-generic-symbolic",
            _ => "mail-attachment-symbolic",
        }
    }

    pub fn caption(&self) -> Option<&str> {
        if self.content.starts_with('[') && self.content.ends_with(']') {
            None
        } else {
            Some(&self.content)
        }
    }

    pub fn has_local_file(&self) -> bool {
        self.media_path
            .as_deref()
            .map(|p| !p.is_empty())
            .unwrap_or(false)
    }

    /// Display name shown above the bubble. Newsletters always speak
    /// as the channel itself, so we substitute the chat's own name +
    /// avatar instead of letting "Unknown" leak through when the
    /// per-message sender lookup fails.
    pub fn display_sender_name(&self) -> String {
        if self.from_me {
            return fl!("sender-you");
        }
        if self.chat_kind == "newsletter"
            && let Some(n) = self.chat_display_name.as_deref()
            && !n.is_empty()
        {
            return n.to_string();
        }
        if self.sender_name.is_empty() {
            fl!("sender-unknown")
        } else {
            self.sender_name.clone()
        }
    }

    /// Avatar path used by the bubble's gutter. Newsletters reuse the
    /// chat's own avatar — same fallback path as the sender name.
    pub fn display_avatar_path(&self) -> Option<&str> {
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

    pub fn header_markup(&self) -> String {
        // Cheap by itself, but called from `#[watch]` so it runs on
        // every tick. We don't cache it as a field because the
        // sender's display name can change post-construction (e.g.
        // newsletters rename, contact aliases resolve), and the
        // computation is simpler than the markdown pipeline. Two
        // String allocations per tick per row is well below the
        // markdown converter's cost — leaving uncached is the right
        // tradeoff here.
        format!(
            "<b>{}</b>  <span alpha=\"60%\" size=\"small\">{}</span>",
            glib_markup_escape(&self.display_sender_name()),
            glib_markup_escape(&self.timestamp),
        )
    }

    pub fn short_timestamp(&self) -> &str {
        &self.short_time
    }
}
