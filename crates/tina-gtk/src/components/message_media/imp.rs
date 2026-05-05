// Subclass internals for `TinaMessageMedia`. See parent module for
// the public surface.

use std::cell::{Cell, RefCell};

use adw::prelude::*;
use glib::clone;
use glib::subclass::prelude::*;
use gtk::subclass::prelude::*;

use crate::components::message_bubble::lightbox::open_media_lightbox;
use crate::components::chat_tab::messages::ChatTabInput;

use super::AnimatedImagePaintable;
use super::{ClickSlot, ClickTarget, MediaKind, MediaState};

// Per-kind size caps — Discord-style. Each axis is the maximum the
// row may take; aspect ratio is preserved by `scale_to_fit` /
// glycin's `ContentFit::ScaleDown`. Discord caps photos at ~400 px
// on the longest side; we do the same for both axes so a tall
// portrait photo doesn't blow the row's height out.
const STICKER_MAX: (i32, i32) = (160, 160);
const IMAGE_MAX: (i32, i32) = (400, 400);
const VIDEO_MAX: (i32, i32) = (480, 360);
/// Used when the proto didn't supply media_width/media_height —
/// the widget reserves this footprint while glycin loads. Picked to
/// match the IMAGE_MAX so most photos land near their final size
/// without shrinking the row.
const FALLBACK_DIMS: (i32, i32) = (400, 300);

const PAGE_EMPTY: &str = "empty";
const PAGE_PLACEHOLDER: &str = "placeholder";
const PAGE_MEDIA: &str = "media";
const PAGE_VIDEO: &str = "video";

pub struct TinaMessageMedia {
    overlay: gtk::Overlay,
    stack: gtk::Stack,
    placeholder_picture: gtk::Picture,
    media_picture: gtk::Picture,
    /// Bin host for the video page. We don't pre-create a
    /// `gtk::Video` (each instance owns a GStreamer pipeline + a
    /// GWakeup pipe fd) — only when the user expands.
    video_bin: adw::Bin,
    /// Active video player widget when expanded. Dropped on rebind
    /// to a different message OR when expansion collapses, so we
    /// don't accumulate pipelines as the user scrolls.
    video_widget: RefCell<Option<gtk::Video>>,
    spinner: gtk::Spinner,
    download_btn: gtk::Button,
    play_overlay: gtk::Image,
    /// Top-right fullscreen icon button shown on the video page.
    /// Click opens the lightbox with the file.
    fullscreen_btn: gtk::Button,

    kind: Cell<MediaKind>,
    state: RefCell<MediaState>,
    /// Live click target. Single shared `Rc<RefCell<…>>` between the
    /// imp and every gesture handler installed in `constructed`. The
    /// public `set_click_target` writes the inner Option, so the
    /// handlers (which captured this Rc by clone at setup time)
    /// always see the latest target.
    click_slot: ClickSlot,
    click_gesture_attached: Cell<bool>,
    /// Currently bound animated paintable, if any. Held so we can
    /// pause it on `unmap` and resume on `map`.
    animated: RefCell<Option<AnimatedImagePaintable>>,
    /// Monotonic generation counter for in-flight glycin loads. A
    /// load whose generation no longer matches `load_gen` at completion
    /// is dropped — happens whenever a recycled slot is rebound to a
    /// different message before the loader returned.
    load_gen: Cell<u64>,
    /// Whether the parent widget is currently mapped. Drives the
    /// animation play/pause for the active animated paintable.
    mapped: Cell<bool>,
    /// `true` while a glycin load is in flight. Combined with the
    /// download status to decide whether to show the spinner — both
    /// states (server fetch / local decode) park the user on a
    /// loading overlay rather than a blank surface.
    decoding: Cell<bool>,
}

impl Default for TinaMessageMedia {
    fn default() -> Self {
        // Stack with crossfade + interpolate-size — same animation
        // as Fractal's overlay so rows don't jump when the real
        // paintable replaces the thumbnail.
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(150)
            .interpolate_size(true)
            .hhomogeneous(false)
            .vhomogeneous(false)
            .build();

        let empty = adw::Bin::new();
        stack.add_named(&empty, Some(PAGE_EMPTY));

        let placeholder_picture = gtk::Picture::builder()
            // Cover (not ScaleDown): the proto thumbnail is tiny
            // (~100×150 JPEG) but the widget reserves the full
            // expected media footprint. ScaleDown would render the
            // thumbnail at its intrinsic size, centred, leaving the
            // rest blank. Cover scales the thumbnail to fill the
            // widget — pixelated, but a CSS blur turns it into a
            // pleasant "blurhash" placeholder. Discord/WhatsApp
            // use the same trick.
            .content_fit(gtk::ContentFit::Cover)
            .build();
        placeholder_picture.add_css_class("message-picture");
        placeholder_picture.add_css_class("message-thumbnail-blur");
        stack.add_named(&placeholder_picture, Some(PAGE_PLACEHOLDER));

        let media_picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::ScaleDown)
            .build();
        media_picture.add_css_class("message-picture");
        stack.add_named(&media_picture, Some(PAGE_MEDIA));

        // Video page — empty bin until the user expands. The
        // gtk::Video widget is lazy-created in apply_state and
        // installed as the bin's child; collapsing or rebinding
        // drops it to free the GStreamer pipeline.
        let video_bin = adw::Bin::new();
        video_bin.add_css_class("message-picture");
        stack.add_named(&video_bin, Some(PAGE_VIDEO));

        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&stack));
        // Default visual: opaque background like Fractal's images.
        // Sticker mode strips it via apply_state.
        overlay.add_css_class("visual-content");
        overlay.add_css_class("opaque-bg");

        let spinner = gtk::Spinner::builder()
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .width_request(32)
            .height_request(32)
            .build();
        spinner.set_spinning(true);
        spinner.set_visible(false);
        overlay.add_overlay(&spinner);

        let download_btn = gtk::Button::builder()
            .icon_name("folder-download-symbolic")
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        download_btn.add_css_class("circular");
        download_btn.add_css_class("osd");
        download_btn.set_visible(false);
        overlay.add_overlay(&download_btn);

        let play_overlay = gtk::Image::builder()
            .icon_name("media-playback-start-symbolic")
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .pixel_size(48)
            .build();
        play_overlay.add_css_class("osd");
        play_overlay.add_css_class("circular");
        play_overlay.set_visible(false);
        overlay.add_overlay(&play_overlay);

        let fullscreen_btn = gtk::Button::builder()
            .icon_name("view-fullscreen-symbolic")
            .tooltip_text("Open fullscreen")
            .halign(gtk::Align::End)
            .valign(gtk::Align::Start)
            .margin_top(8)
            .margin_end(8)
            .build();
        fullscreen_btn.add_css_class("circular");
        fullscreen_btn.add_css_class("osd");
        fullscreen_btn.set_visible(false);
        overlay.add_overlay(&fullscreen_btn);

        Self {
            overlay,
            stack,
            placeholder_picture,
            media_picture,
            video_bin,
            video_widget: RefCell::new(None),
            spinner,
            download_btn,
            play_overlay,
            fullscreen_btn,
            kind: Cell::new(MediaKind::None),
            state: RefCell::new(MediaState::default()),
            click_slot: std::rc::Rc::new(RefCell::new(None)),
            click_gesture_attached: Cell::new(false),
            animated: RefCell::new(None),
            load_gen: Cell::new(0),
            mapped: Cell::new(false),
            decoding: Cell::new(false),
        }
    }
}

#[glib::object_subclass]
impl ObjectSubclass for TinaMessageMedia {
    const NAME: &'static str = "TinaMessageMedia";
    type Type = super::TinaMessageMedia;
    type ParentType = gtk::Widget;
}

impl ObjectImpl for TinaMessageMedia {
    fn constructed(&self) {
        self.parent_constructed();
        self.overlay.set_parent(&*self.obj());

        // Download button click goes through the click slot (same
        // dispatch path as a tap on the surface). The Rc clone here
        // shares the inner cell with `self.click_slot` — bind-time
        // writes to either are visible at click time.
        let slot_for_btn = self.click_slot.clone();
        self.download_btn.connect_clicked(move |_| {
            let Some(target) = slot_for_btn.borrow().clone() else {
                return;
            };
            if target.status == "downloading" {
                return;
            }
            let _ = target
                .sender
                .send(ChatTabInput::RequestMediaDownload(target.message_id));
        });

        // Tap-anywhere gesture on the overlay — dispatches
        // download / lightbox / inline-expand depending on the row
        // state. Stickers are intentionally inert (no click
        // activation) — same UX as Fractal.
        let gesture = gtk::GestureClick::new();
        let slot_for_gesture = self.click_slot.clone();
        gesture.connect_released(move |g, _, _, _| {
            let Some(target) = slot_for_gesture.borrow().clone() else {
                return;
            };
            if matches!(target.kind, MediaKind::Sticker) {
                return;
            }
            let downloaded = target
                .path
                .as_deref()
                .map(|p| !p.is_empty())
                .unwrap_or(false);
            if !downloaded {
                if target.status != "downloading" {
                    let _ = target
                        .sender
                        .send(ChatTabInput::RequestMediaDownload(target.message_id));
                }
                return;
            }
            match target.kind {
                MediaKind::Image => {
                    if let (Some(p), Some(w)) = (target.path, g.widget()) {
                        open_media_lightbox(&w, p, "image".to_string());
                    }
                }
                MediaKind::Video => {
                    // Video click expands inline — flip the shared
                    // ui_state and ask the chat tab to rebind. The
                    // next bind sees `expanded=true` and switches
                    // the stack to the `video` page (lazy-creating
                    // the gtk::Video).
                    target.ui_state.set_media_expanded(&target.message_id, true);
                    let _ = target
                        .sender
                        .send(ChatTabInput::RebindRow(target.message_id));
                }
                _ => {}
            }
        });
        self.overlay.add_controller(gesture);
        self.click_gesture_attached.set(true);

        // Fullscreen button — only visible when video is expanded;
        // opens the lightbox with the file.
        let slot_for_fs = self.click_slot.clone();
        self.fullscreen_btn.connect_clicked(move |btn| {
            let Some(target) = slot_for_fs.borrow().clone() else {
                return;
            };
            if let Some(p) = target.path.clone() {
                open_media_lightbox(btn, p, "video".to_string());
            }
        });
    }

    fn dispose(&self) {
        self.overlay.unparent();
    }
}

impl WidgetImpl for TinaMessageMedia {
    fn request_mode(&self) -> gtk::SizeRequestMode {
        gtk::SizeRequestMode::HeightForWidth
    }

    fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
        let (overlay_min, ..) = self.overlay.measure(orientation, for_size);

        let (max_w, max_h) = match self.kind.get() {
            MediaKind::Sticker => STICKER_MAX,
            MediaKind::Image => IMAGE_MAX,
            MediaKind::Video => VIDEO_MAX,
            MediaKind::None => return (overlay_min, overlay_min, -1, -1),
        };
        let max = if orientation == gtk::Orientation::Vertical {
            max_h
        } else {
            max_w
        };
        let max_for_size = if orientation == gtk::Orientation::Vertical {
            max_w
        } else {
            max_h
        };

        // Limit incoming for_size to the orthogonal max so a wide
        // chat split doesn't ask for a 1200-px-wide allocation that
        // we'd then have to clamp.
        let for_size = if for_size == -1 {
            max_for_size
        } else {
            for_size.min(max_for_size)
        };

        // If the media page is showing a paintable, use its
        // intrinsic size — clamped to max. This matches Fractal's
        // logic for rows that have already loaded their full
        // resolution.
        if self.stack.visible_child_name().as_deref() == Some(PAGE_MEDIA) {
            let other = if orientation == gtk::Orientation::Vertical {
                gtk::Orientation::Horizontal
            } else {
                gtk::Orientation::Vertical
            };
            let (_, intrinsic_for_size, ..) = self.media_picture.measure(other, -1);
            let (child_min, child_nat, ..) = self
                .media_picture
                .measure(orientation, for_size.min(intrinsic_for_size));
            if child_nat != 0 {
                let min = child_min.max(overlay_min).min(max);
                let nat = child_nat.max(overlay_min).min(max);
                return (min, nat, -1, -1);
            }
        }

        // Pre-load / placeholder path: use the proto's media
        // dimensions when available so the row reserves the EXACT
        // footprint the full file will occupy (zero row-resize when
        // glycin lands). Without these we'd use the proto thumbnail's
        // intrinsic size — typically ~100×150 — and the layout would
        // jump from a tiny placeholder to a 400 px photo.
        let state = self.state.borrow();
        let intrinsic = match (state.width, state.height) {
            (Some(w), Some(h)) if w > 0 && h > 0 => (w, h),
            _ => FALLBACK_DIMS,
        };
        drop(state);
        let (nat_w, nat_h) = if orientation == gtk::Orientation::Vertical {
            scale_to_fit(intrinsic, (for_size, max))
        } else {
            scale_to_fit(intrinsic, (max, for_size))
        };
        let nat = if orientation == gtk::Orientation::Vertical {
            nat_h
        } else {
            nat_w
        };
        let nat = nat.max(overlay_min).min(max);
        let min = overlay_min.min(max);
        (min, nat, -1, -1)
    }

    fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
        self.overlay.allocate(width, height, baseline, None);
    }

    fn map(&self) {
        self.parent_map();
        self.mapped.set(true);
        if let Some(anim) = self.animated.borrow().as_ref() {
            anim.set_playing(true);
        }
    }

    fn unmap(&self) {
        self.mapped.set(false);
        if let Some(anim) = self.animated.borrow().as_ref() {
            anim.set_playing(false);
        }
        self.parent_unmap();
    }
}

impl TinaMessageMedia {
    /// Update the click target the gesture handlers read through.
    /// Called from bind once per re-bind. The handlers see the new
    /// target on the next click because they captured the same Rc
    /// in `constructed`.
    pub(super) fn set_click_target_inner(&self, target: ClickTarget) {
        *self.click_slot.borrow_mut() = Some(target);
    }

    pub(super) fn clear_click_target_inner(&self) {
        *self.click_slot.borrow_mut() = None;
    }


    pub(super) fn apply_state(
        &self,
        new_state: MediaState,
        media_inv: &crate::inventory::MediaInventory,
    ) {
        // Sticker vs image visual difference — only the opaque-bg
        // class. Fractal does the same.
        match new_state.kind {
            MediaKind::Sticker => self.overlay.remove_css_class("opaque-bg"),
            _ => self.overlay.add_css_class("opaque-bg"),
        }
        self.kind.set(new_state.kind);

        let downloaded = new_state
            .path
            .as_deref()
            .map(|p| !p.is_empty())
            .unwrap_or(false);
        let downloading = new_state.status == "downloading";

        // Drop any active animated paintable from a previous bind —
        // a recycled slot must not paint the previous sticker's
        // frames over the new row's content.
        if let Some(prev) = self.animated.take() {
            prev.set_playing(false);
        }
        // Bump the load generation so any in-flight async glycin
        // task whose result is about to land becomes a no-op.
        let generation = self.load_gen.get().wrapping_add(1);
        self.load_gen.set(generation);

        // Pick the rendering strategy by kind. Every code path here
        // routes pixel decoding through glycin (sandboxed) — we do
        // NOT touch GdkPixbuf or `gtk::gdk::Texture::from_*` for
        // user-supplied bytes/files. The attack surface for things
        // like CVE-2023-4863 (libwebp RCE) lives in those decoders;
        // glycin runs them in a separate process with seccomp.
        self.decoding.set(false);
        if downloaded {
            match new_state.kind {
                MediaKind::Sticker | MediaKind::Image => {
                    // Park on the cached proto thumbnail (placeholder
                    // page) while glycin decodes the full file. If
                    // the thumbnail isn't decoded yet, fire an async
                    // decode for it too — that lands much faster
                    // than the full file because the bytes are
                    // already in memory (proto blob).
                    self.show_proto_thumbnail(&new_state, media_inv, generation);
                    self.media_picture
                        .set_paintable(gtk::gdk::Paintable::NONE);
                    self.decoding.set(true);
                    if let Some(path) = new_state.path.clone() {
                        self.spawn_glycin_load(
                            generation,
                            path,
                            new_state.kind,
                            media_inv.clone(),
                        );
                    }
                }
                MediaKind::Video => {
                    if new_state.expanded {
                        // Lazy-instantiate the gtk::Video. One per
                        // expanded row; collapses + rebinds drop it
                        // to free the GStreamer pipeline + GWakeup
                        // pipe fd. Without this we'd accumulate
                        // pipelines across recycled slots and burn
                        // through the per-process fd limit on a
                        // chat with several voice notes / videos.
                        if let Some(path) = new_state.path.clone() {
                            self.expand_video(path);
                        }
                    } else {
                        // Tear down any previous expanded video so
                        // we're back on the thumbnail surface.
                        self.collapse_video();
                        self.show_proto_thumbnail_in_media(
                            &new_state,
                            media_inv,
                            generation,
                        );
                    }
                }
                MediaKind::None => {}
            }
        } else {
            // Pre-download state — proto thumbnail (or empty) on
            // placeholder page.
            self.show_proto_thumbnail(&new_state, media_inv, generation);
        }

        // Overlays. The spinner shows for both states the user
        // perceives as "loading": server fetch in progress, AND
        // glycin decoding a downloaded file. They look identical
        // (same widget) so we collapse them into one visibility.
        self.spinner
            .set_visible(downloading || self.decoding.get());
        self.download_btn
            .set_visible(!downloaded && !downloading && !matches!(new_state.kind, MediaKind::Sticker));
        // Play icon only on the un-expanded video thumbnail.
        self.play_overlay.set_visible(
            downloaded
                && matches!(new_state.kind, MediaKind::Video)
                && !new_state.expanded,
        );
        // Fullscreen button only when video is actually expanded.
        self.fullscreen_btn
            .set_visible(matches!(new_state.kind, MediaKind::Video) && new_state.expanded);
        if !downloaded && !downloading {
            // Pick icon based on retry vs first-time
            let icon = if new_state.status == "failed" {
                "view-refresh-symbolic"
            } else {
                "folder-download-symbolic"
            };
            self.download_btn.set_icon_name(icon);
        }

        *self.state.borrow_mut() = new_state;
        self.obj().queue_resize();
    }

    pub(super) fn clear(&self) {
        self.kind.set(MediaKind::None);
        if let Some(prev) = self.animated.take() {
            prev.set_playing(false);
        }
        self.collapse_video();
        // Bump the generation so any pending glycin load becomes a
        // no-op when it lands.
        self.load_gen.set(self.load_gen.get().wrapping_add(1));
        self.media_picture.set_paintable(gtk::gdk::Paintable::NONE);
        self.placeholder_picture.set_paintable(gtk::gdk::Paintable::NONE);
        self.stack.set_visible_child_name(PAGE_EMPTY);
        self.spinner.set_visible(false);
        self.download_btn.set_visible(false);
        self.play_overlay.set_visible(false);
        self.fullscreen_btn.set_visible(false);
        let _ = &self.obj();
    }

    /// Build (if needed) and show the gtk::Video for the given
    /// path. Reuses an existing widget if the path matches.
    fn expand_video(&self, path: String) {
        let need_new = match self.video_widget.borrow().as_ref() {
            None => true,
            Some(v) => v.file().and_then(|f| f.path()).map(|p| p.to_string_lossy().into_owned()) != Some(path.clone()),
        };
        if need_new {
            // Drop the previous pipeline before building a new one
            // — without this, switching videos would keep both
            // GstPipelines alive until the bin's child slot was
            // overwritten.
            self.collapse_video();
            let video = gtk::Video::for_filename(Some(std::path::Path::new(&path)));
            video.set_autoplay(true);
            video.set_hexpand(true);
            video.set_vexpand(true);
            self.video_bin.set_child(Some(&video));
            *self.video_widget.borrow_mut() = Some(video);
        }
        self.stack.set_visible_child_name(PAGE_VIDEO);
    }

    /// Drop the gtk::Video (releases the GStreamer pipeline) and
    /// clear the bin. Cheap no-op if no video was active.
    fn collapse_video(&self) {
        if self.video_widget.take().is_some() {
            self.video_bin.set_child(gtk::Widget::NONE);
        }
    }

    /// Async-decode the given file via glycin and install the
    /// resulting paintable. The `generation` argument is used to detect
    /// stale completions: if `load_gen` has advanced past it by the
    /// time the future returns, the row was rebound to a different
    /// message and we drop the result.
    fn spawn_glycin_load(
        &self,
        generation: u64,
        path: String,
        kind: MediaKind,
        _media_inv: crate::inventory::MediaInventory,
    ) {
        // Glycin decodes still images (sticker, image). Video
        // containers are handled separately by the proto-thumbnail
        // path; the video file itself never goes through any image
        // decoder.
        if !matches!(kind, MediaKind::Image | MediaKind::Sticker) {
            return;
        }
        glib::MainContext::default().spawn_local(clone!(
            #[weak(rename_to = obj)]
            self.obj(),
            async move {
                let file = gtk::gio::File::for_path(&path);
                let loader = glycin::Loader::new(file);
                let result: Option<(glycin::Image, glycin::Frame)> = async {
                    let image = loader.load().await.ok()?;
                    let first = image.next_frame().await.ok()?;
                    Some((image, first))
                }
                .await;

                let imp = obj.imp();
                if imp.load_gen.get() != generation {
                    return; // row rebinded; result is stale
                }

                if let Some((image, first)) = result {
                    let start_playing = imp.mapped.get();
                    let paintable =
                        AnimatedImagePaintable::new(image, first, start_playing);
                    imp.media_picture.set_paintable(Some(&paintable));
                    imp.stack.set_visible_child_name(PAGE_MEDIA);
                    *imp.animated.borrow_mut() = Some(paintable);
                } else {
                    // Glycin failed — leave the placeholder thumbnail
                    // in place. We deliberately do NOT fall back to
                    // GdkPixbuf so a malformed file can't pivot the
                    // decode into an unsandboxed loader.
                    tracing::debug!(
                        path = %path,
                        "glycin: load failed; staying on thumbnail",
                    );
                }

                imp.decoding.set(false);
                imp.spinner.set_visible(false);
            }
        ));
    }

    /// Show the proto-thumbnail blob on the placeholder page (or
    /// empty page if no thumbnail). On cache miss, fires an async
    /// glycin decode of the bytes — when it lands the page swaps to
    /// `placeholder` automatically.
    fn show_proto_thumbnail(
        &self,
        new_state: &MediaState,
        media_inv: &crate::inventory::MediaInventory,
        generation: u64,
    ) {
        if let Some(paintable) = media_inv.cached_thumbnail(&new_state.message_id) {
            self.placeholder_picture.set_paintable(Some(&paintable));
            self.stack.set_visible_child_name(PAGE_PLACEHOLDER);
            return;
        }
        // Cache miss — empty page until glycin lands the bytes.
        self.stack.set_visible_child_name(PAGE_EMPTY);
        let Some(bytes) = new_state.thumbnail.clone() else {
            return;
        };
        if bytes.is_empty() {
            return;
        }
        self.spawn_thumbnail_decode(
            generation,
            new_state.message_id.clone(),
            bytes,
            ThumbTarget::Placeholder,
            media_inv.clone(),
        );
    }

    /// Same role as `show_proto_thumbnail` but lands the result on
    /// the placeholder page (which already has the blur class +
    /// Cover sizing). Video rows use this so the play overlay sits
    /// above a properly-sized blurred preview that matches the
    /// widget's allocated footprint instead of the tiny intrinsic
    /// thumbnail.
    fn show_proto_thumbnail_in_media(
        &self,
        new_state: &MediaState,
        media_inv: &crate::inventory::MediaInventory,
        generation: u64,
    ) {
        // Reuse the placeholder path — it already has the right
        // ContentFit + blur styling. Differences vs the regular
        // image case: the play overlay sits over this, which is
        // fine because overlays are siblings of the stack, not
        // children of any one page.
        self.show_proto_thumbnail(new_state, media_inv, generation);
    }

    fn spawn_thumbnail_decode(
        &self,
        generation: u64,
        message_id: String,
        bytes: Vec<u8>,
        target: ThumbTarget,
        media_inv: crate::inventory::MediaInventory,
    ) {
        glib::MainContext::default().spawn_local(clone!(
            #[weak(rename_to = obj)]
            self.obj(),
            async move {
                let glib_bytes = glib::Bytes::from(&bytes[..]);
                let loader = glycin::Loader::new_bytes(glib_bytes);
                let texture: Option<gtk::gdk::Texture> = async {
                    let image = loader.load().await.ok()?;
                    let frame = image.next_frame().await.ok()?;
                    Some(frame.texture())
                }
                .await;
                let Some(texture) = texture else {
                    tracing::debug!(
                        message_id = %message_id,
                        "glycin: proto thumbnail decode failed",
                    );
                    return;
                };
                media_inv.put_thumbnail(&message_id, texture.clone());
                let imp = obj.imp();
                if imp.load_gen.get() != generation {
                    return;
                }
                let paintable: gtk::gdk::Paintable = texture.upcast();
                match target {
                    ThumbTarget::Placeholder => {
                        imp.placeholder_picture.set_paintable(Some(&paintable));
                        // Don't override the media page if it has
                        // already been replaced by the full glycin
                        // load that landed faster (rare but possible).
                        if imp.stack.visible_child_name().as_deref() != Some(PAGE_MEDIA) {
                            imp.stack.set_visible_child_name(PAGE_PLACEHOLDER);
                        }
                    }
                    ThumbTarget::Media => {
                        imp.media_picture.set_paintable(Some(&paintable));
                        imp.stack.set_visible_child_name(PAGE_MEDIA);
                    }
                }
            }
        ));
    }
}

/// Where a freshly-decoded thumbnail should land.
#[derive(Clone, Copy)]
enum ThumbTarget {
    Placeholder,
    Media,
}

fn scale_to_fit(content: (i32, i32), available: (i32, i32)) -> (i32, i32) {
    // ContentFit::ScaleDown semantics — never scale up. Preserve
    // aspect ratio when shrinking to fit `available`.
    let (cw, ch) = (content.0 as f64, content.1 as f64);
    let (aw, ah) = (available.0.max(0) as f64, available.1.max(0) as f64);
    if cw <= 0.0 || ch <= 0.0 {
        return (0, 0);
    }
    if aw <= 0.0 || ah <= 0.0 {
        return (content.0, content.1);
    }
    let ratio_w = aw / cw;
    let ratio_h = ah / ch;
    let scale = ratio_w.min(ratio_h).min(1.0);
    let w = (cw * scale).round() as i32;
    let h = (ch * scale).round() as i32;
    (w, h)
}

/// Suppress an unused-import lint; `ClickTarget` is consumed by the
/// closure inside `constructed`.
#[allow(dead_code)]
fn _suppress(_: ClickTarget) {}
