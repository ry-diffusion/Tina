// Send-confirmation dialog for media attachments.
//
// Mirrors WhatsApp's "review before send" sheet: shows a preview of
// the picked file (image/sticker render inline; video shows the
// filename with a play icon; audio/voice expose a controls strip via
// gtk::MediaControls; documents drop down to a generic icon + name +
// size). Image/Video/Document expose a caption Entry; sticker / audio
// / voice skip it.
//
// The dialog is intentionally a one-shot: a fresh instance is built
// per attachment, presented modal-on-window, and consumes itself on
// Send / Cancel — no relm4 component lifecycle to manage and no
// retained state to forget to wipe between attachments.

use std::cell::RefCell;
use crate::fl;
use std::path::Path;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use relm4::ComponentSender;

use super::messages::ChatTabInput;
use super::model::ChatTab;

const PREVIEW_WIDTH: i32 = 360;
const IMAGE_HEIGHT: i32 = 320;
const STICKER_SIZE: i32 = 192;

/// Build + present the preview. `kind` controls layout; `mimetype`
/// and `filename` are forwarded onto the eventual `SendMedia` so the
/// worker doesn't have to re-derive them.
pub fn present(
    parent: &gtk::Widget,
    sender: ComponentSender<ChatTab>,
    kind: tina_core::MediaKind,
    path: String,
    mimetype: Option<String>,
    filename: Option<String>,
) {
    let dialog = adw::AlertDialog::builder()
        .heading(&heading_for(kind))
        .body("")
        .close_response("cancel")
        .default_response("send")
        .build();

    dialog.add_response("cancel", &fl!("send-cancel"));
    dialog.add_response("send", &fl!("send-send"));
    dialog.set_response_appearance("send", adw::ResponseAppearance::Suggested);

    let content = build_preview_widget(kind, &path, &filename);
    let caption_entry: Option<gtk::Entry> = if kind_has_caption(kind) {
        let entry = gtk::Entry::builder()
            .placeholder_text(&fl!("send-caption-placeholder"))
            .activates_default(true)
            .hexpand(true)
            .margin_top(8)
            .build();
        Some(entry)
    } else {
        None
    };

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    body.append(&content);
    if let Some(entry) = &caption_entry {
        body.append(entry);
    }
    body.set_size_request(PREVIEW_WIDTH, -1);
    dialog.set_extra_child(Some(&body));

    // RefCell so the closure can move the entry / sender / path
    // out on Send without `Clone` impls we don't control.
    let state = Rc::new(RefCell::new(Some((path, mimetype, filename, caption_entry))));
    let sender_for_response = sender.clone();
    dialog.connect_response(None, move |dlg, response| {
        if response == "send" {
            if let Some((path, mimetype, filename, caption_entry)) = state.borrow_mut().take() {
                let caption = caption_entry
                    .as_ref()
                    .map(|e| e.text().to_string())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                let _ = sender_for_response.input_sender().send(ChatTabInput::SendMedia {
                    kind,
                    path,
                    caption,
                    mimetype,
                    filename,
                });
            }
        }
        dlg.close();
    });

    dialog.present(Some(parent));
}

fn heading_for(kind: tina_core::MediaKind) -> String {
    match kind {
        tina_core::MediaKind::Image => fl!("send-heading-photo"),
        tina_core::MediaKind::Video => fl!("send-heading-video"),
        tina_core::MediaKind::Audio => fl!("send-heading-audio"),
        tina_core::MediaKind::Voice => fl!("send-heading-voice"),
        tina_core::MediaKind::Sticker => fl!("send-heading-sticker"),
        tina_core::MediaKind::Document => fl!("send-heading-document"),
    }
}

fn kind_has_caption(kind: tina_core::MediaKind) -> bool {
    matches!(
        kind,
        tina_core::MediaKind::Image
            | tina_core::MediaKind::Video
            | tina_core::MediaKind::Document
    )
}

fn build_preview_widget(
    kind: tina_core::MediaKind,
    path: &str,
    filename: &Option<String>,
) -> gtk::Widget {
    match kind {
        tina_core::MediaKind::Image => image_preview(path, IMAGE_HEIGHT),
        tina_core::MediaKind::Sticker => image_preview(path, STICKER_SIZE),
        tina_core::MediaKind::Video => video_preview(path),
        tina_core::MediaKind::Audio | tina_core::MediaKind::Voice => audio_preview(path),
        tina_core::MediaKind::Document => document_preview(path, filename),
    }
}

fn image_preview(path: &str, max_height: i32) -> gtk::Widget {
    // Using gdk::Texture::from_filename keeps things lazy — GTK
    // streams the decode without us having to pull GdkPixbuf in
    // explicitly. On failure we fall back to a generic file icon so
    // the dialog still opens.
    match gdk::Texture::from_filename(path) {
        Ok(tex) => {
            let pic = gtk::Picture::for_paintable(&tex);
            pic.set_can_shrink(true);
            pic.set_content_fit(gtk::ContentFit::Contain);
            pic.set_height_request(max_height);
            pic.set_width_request(PREVIEW_WIDTH);
            pic.upcast()
        }
        Err(_) => generic_file_row(path, "image-x-generic-symbolic"),
    }
}

fn video_preview(path: &str) -> gtk::Widget {
    // gtk::Video covers playback of common containers via gstreamer.
    // We size it conservatively so the dialog stays a reasonable
    // shape even on tall portrait clips.
    let video = gtk::Video::for_filename(Some(Path::new(path)));
    video.set_size_request(PREVIEW_WIDTH, 240);
    video.set_autoplay(false);
    video.upcast()
}

fn audio_preview(path: &str) -> gtk::Widget {
    let media = gtk::MediaFile::for_filename(Path::new(path));
    let controls = gtk::MediaControls::new(Some(&media));
    controls.set_hexpand(true);
    controls.upcast()
}

fn document_preview(path: &str, filename: &Option<String>) -> gtk::Widget {
    let display_name = filename
        .clone()
        .unwrap_or_else(|| Path::new(path).file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(path)
            .to_string());
    generic_file_row(&display_name, "document-symbolic")
}

fn generic_file_row(label: &str, icon_name: &str) -> gtk::Widget {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(48);
    let lbl = gtk::Label::builder()
        .label(label)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .max_width_chars(36)
        .hexpand(true)
        .xalign(0.0)
        .build();
    row.append(&icon);
    row.append(&lbl);
    row.upcast()
}

// Suppress an `unused` warning when only some kinds compile-in (e.g.
// future feature flag for video). Currently every helper is used,
// but keeping the lint silenced costs nothing.
#[allow(dead_code)]
fn _unused(_g: glib::WeakRef<gtk::Box>) {}
