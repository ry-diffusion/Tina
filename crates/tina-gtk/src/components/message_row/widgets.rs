// All widget refs that `bind()` needs to mutate when a recycled row
// is mapped to a new `MessageRowItem`. Built once per slot in
// `setup()`; `bind()` walks them; `unbind()` no-ops here because the
// per-slot `RowSlot` (held inside `RowContext`) is the shared cell
// gestures read through and we leave it populated until the next
// bind overwrites it.

use std::cell::RefCell;
use std::rc::Rc;

pub struct MessageRowWidgets {
    // ── Day divider ───────────────────────────────────────────────
    pub day_divider_box: gtk::Box,
    pub day_divider_label: gtk::Label,

    // ── Top-level layout ──────────────────────────────────────────
    pub message_box: gtk::Box,

    // ── Left gutter ───────────────────────────────────────────────
    pub avatar: adw::Avatar,
    pub collapsed_timestamp: gtk::Label,

    // ── Header row (sender + delivery status) ─────────────────────
    pub header_box: gtk::Box,
    pub header_label: gtk::Label,
    pub status_icon: gtk::Image,

    // ── Reply quote header ────────────────────────────────────────
    pub reply_button: gtk::Button,
    pub reply_author: gtk::Label,
    pub reply_preview: gtk::Label,

    // ── Visual media (image / sticker / video thumbnail) ──────────
    /// Custom widget porting Fractal's `MessageVisualMedia` —
    /// handles placeholder → real-texture crossfade, per-kind size
    /// caps via custom `WidgetImpl::measure`, opaque-bg toggle for
    /// sticker, and click dispatch through an internal slot.
    pub visual_media: super::super::message_media::TinaMessageMedia,

    // ── Audio not yet expanded ────────────────────────────────────
    pub audio_compact_box: gtk::Box,
    pub audio_compact_kind: gtk::Label,
    pub audio_compact_summary: gtk::Label,

    // ── Audio expanded ────────────────────────────────────────────
    pub audio_controls: gtk::MediaControls,

    // ── Audio / document not downloaded ───────────────────────────
    pub generic_dl_box: gtk::Box,
    pub generic_dl_icon: gtk::Image,
    pub generic_dl_kind: gtk::Label,
    pub generic_dl_summary: gtk::Label,
    pub generic_dl_button: gtk::Button,
    pub generic_dl_spinner: gtk::Spinner,

    // ── Document downloaded ───────────────────────────────────────
    pub document_open_button: gtk::Button,

    // ── Text / caption ────────────────────────────────────────────
    pub content_label: gtk::Label,

    // ── Per-slot mutable context shared with click handlers ───────
    /// Holds the live row data so gestures wired in `setup` see the
    /// CURRENT message, not whatever was bound first. Mirrors the
    /// `menu_target` cell pattern from `chat_row::widgets`.
    pub slot: Rc<RefCell<Option<RowContext>>>,
}

/// Minimal data the click closures need to act on the bound row at
/// click time. `bind` rewrites this; `unbind` clears it (so a stray
/// click on a not-yet-rebound recycled row no-ops).
pub struct RowContext {
    pub message_id: String,
    pub message_type: String,
    pub media_path: Option<String>,
    pub media_status: String,
    pub quoted_message_id: Option<String>,
    pub sender: relm4::Sender<super::super::chat_tab::messages::ChatTabInput>,
    pub ui_state: super::item::RowUiInventory,
}
