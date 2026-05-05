// `TinaMessageMedia` — a custom `gtk::Widget` subclass that handles
// image / sticker / video-thumbnail rendering with the same approach
// Fractal uses in `room_history::message_row::visual_media`:
//
//   • A `gtk::Stack` 3-page layout (empty / placeholder / media) with
//     crossfade + interpolate-size, so the row shape doesn't jump
//     when the real texture replaces a thumbnail.
//   • `gtk::Picture` children with `content_fit: ScaleDown` so a 4K
//     photo never upscales above the row's allocation.
//   • Custom `WidgetImpl::measure()` that clamps the natural size to
//     a per-kind cap (sticker: 128×128, image: 600×360, video: -×480).
//     This is the only way to actually CAP a widget's size in GTK4 —
//     CSS max-width/max-height aren't honoured for layout, and
//     `set_size_request` is min-only.
//   • Sticker rows opt out of opaque background and click activation;
//     image rows are clickable to open the lightbox; video rows expand
//     into the parent's `gtk::Video` widget on click.

mod animated;
mod imp;

pub use animated::AnimatedImagePaintable;

use std::cell::RefCell;
use std::rc::Rc;

use glib::Object;
use glib::subclass::prelude::ObjectSubclassIsExt;

use crate::components::message_row::RowUiInventory;

glib::wrapper! {
    pub struct TinaMessageMedia(ObjectSubclass<imp::TinaMessageMedia>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for TinaMessageMedia {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MediaKind {
    #[default]
    None,
    Image,
    Sticker,
    Video,
}

/// What the widget should currently show.
#[derive(Clone, Default)]
pub struct MediaState {
    pub kind: MediaKind,
    pub message_id: String,
    /// Inline thumbnail bytes (the proto's blurhash equivalent —
    /// rendered before the full media is downloaded).
    pub thumbnail: Option<Vec<u8>>,
    /// Resolved file path of the downloaded media (when present).
    pub path: Option<String>,
    /// `none | downloading | done | failed`.
    pub status: String,
    /// `true` when this row is in the "expanded" state for video —
    /// the widget shows a `gtk::Video` player on the `video` stack
    /// page. Read from `RowUiInventory` per row by the parent.
    pub expanded: bool,
    /// Source media dimensions reported by the proto (image/sticker/
    /// video). The custom `measure()` clamps this to the per-kind
    /// max so the placeholder reserves the same footprint the full
    /// file will occupy — no row resize when glycin lands. `None`
    /// means we fall back to a per-kind sensible default.
    pub width: Option<i32>,
    pub height: Option<i32>,
}

/// Slot shared with the row's gesture handlers — reused across
/// rebinds. Same pattern as `MessageRowWidgets::slot`. The bind pass
/// writes the current message_id; click handlers read it.
pub type ClickSlot = Rc<RefCell<Option<ClickTarget>>>;

#[derive(Clone)]
pub struct ClickTarget {
    pub message_id: String,
    pub kind: MediaKind,
    pub path: Option<String>,
    pub status: String,
    pub sender: relm4::Sender<crate::components::chat_tab::messages::ChatTabInput>,
    /// Per-row UI state inventory — used by the video click handler
    /// to flip `media_expanded` before triggering a rebind.
    pub ui_state: RowUiInventory,
}

impl TinaMessageMedia {
    pub fn new() -> Self {
        Object::builder().build()
    }

    /// Apply a fresh state. Idempotent — calling with the same kind
    /// + path is cheap (the underlying paintables are inventory-
    /// cached).
    pub fn set_state(
        &self,
        state: MediaState,
        avatars: &crate::inventory::AvatarInventory,
        media_inv: &crate::inventory::MediaInventory,
    ) {
        let _ = avatars; // placeholder for future per-author tinting
        self.imp().apply_state(state, media_inv);
    }

    /// Update the live click target. Bind calls this once per
    /// re-bind so the widget's internal gestures dispatch against
    /// the current message.
    pub fn set_click_target(&self, target: ClickTarget) {
        self.imp().set_click_target_inner(target);
    }

    /// Drop the click target. Recycled rows that no longer carry
    /// visual media should call this so a stray click doesn't
    /// dispatch against the previous message.
    pub fn clear_click_target(&self) {
        self.imp().clear_click_target_inner();
    }

    /// Clear the visible content (used when the row is rebound to a
    /// non-media message — without this the previous photo would
    /// flash before the row collapsed via `set_visible(false)`).
    pub fn clear(&self) {
        self.imp().clear();
        self.imp().clear_click_target_inner();
    }
}
