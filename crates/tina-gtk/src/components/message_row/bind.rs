// Per-bind update pass. Walks the widget refs in `MessageRowWidgets`
// and applies the live `MessageRowItem` data. Replaces the
// `#[watch]`-driven view! macro of the old factory; everything that
// used to re-evaluate on model ticks is now a single bind() call
// triggered by the typed list view (which fires bind only when a
// widget is realized for an item, not on arbitrary updates).

use adw::prelude::*;
use crate::fl;

use super::super::message_bubble::{delivery_css_class, delivery_icon_name};
use super::item::MessageRowItem;
use super::widgets::{MessageRowWidgets, RowContext};

pub fn bind(item: &MessageRowItem, w: &mut MessageRowWidgets, root: &mut gtk::Box) {
    let m = &item.item;

    // ── Day divider ───────────────────────────────────────────────
    if m.is_first_of_day {
        w.day_divider_label.set_label(&m.day_label);
        w.day_divider_box.set_visible(true);
    } else {
        w.day_divider_box.set_visible(false);
    }

    // ── Cozy / collapsed CSS ──────────────────────────────────────
    if m.is_collapsed {
        w.message_box.add_css_class("message-collapsed");
        w.message_box.remove_css_class("message-cozy");
    } else {
        w.message_box.add_css_class("message-cozy");
        w.message_box.remove_css_class("message-collapsed");
    }

    // ── Avatar / collapsed timestamp ──────────────────────────────
    w.avatar.set_visible(!m.is_collapsed);
    w.avatar.set_text(Some(&m.display_sender_name()));
    let avatar_paintable: Option<gtk::gdk::Paintable> = item
        .avatars
        .load_texture(m.display_avatar_path())
        .map(|t| t.upcast());
    w.avatar.set_custom_image(avatar_paintable.as_ref());

    w.collapsed_timestamp.set_visible(m.is_collapsed);
    w.collapsed_timestamp.set_label(m.short_timestamp());

    // ── Header (sender + delivery status) ─────────────────────────
    w.header_box.set_visible(!m.is_collapsed);
    w.header_label.set_markup(&m.header_markup());
    let show_status = m.from_me && m.delivery_status != "sent";
    w.status_icon.set_visible(show_status);
    if show_status {
        w.status_icon
            .set_icon_name(Some(delivery_icon_name(&m.delivery_status)));
        w.status_icon
            .set_css_classes(&["tina-delivery-status", delivery_css_class(&m.delivery_status)]);
    }

    // ── Reply quote header ────────────────────────────────────────
    let has_reply = m.has_reply();
    w.reply_button.set_visible(has_reply);
    if has_reply {
        w.reply_author.set_label(&m.quoted_sender_label());
        w.reply_preview.set_label(&m.quoted_preview_text());
    }

    // ── Visual media (image / sticker / video thumbnail) ──────────
    // Single custom widget handles the trio. Per-kind size caps,
    // overlay state (spinner / download / play), and click dispatch
    // all live inside the widget. Bind just feeds it the row's
    // current state.
    let is_visual = m.is_visual_media();
    let has_local = m.has_local_file();
    let media_expanded = item.ui_state.get(&m.id).media_expanded;
    w.visual_media.set_visible(is_visual);
    if is_visual {
        let kind = match m.message_type.as_str() {
            "sticker" => super::super::message_media::MediaKind::Sticker,
            "video" => super::super::message_media::MediaKind::Video,
            _ => super::super::message_media::MediaKind::Image,
        };
        w.visual_media.set_state(
            super::super::message_media::MediaState {
                kind,
                message_id: m.id.clone(),
                thumbnail: m.thumbnail.clone(),
                path: m.media_path.clone(),
                status: m.media_status.clone(),
                expanded: media_expanded,
                width: m.media_width,
                height: m.media_height,
            },
            &item.avatars,
            &item.media_inv,
        );
        w.visual_media.set_click_target(
            super::super::message_media::ClickTarget {
                message_id: m.id.clone(),
                kind,
                path: m.media_path.clone(),
                status: m.media_status.clone(),
                sender: item.sender.clone(),
                ui_state: item.ui_state.clone(),
            },
        );
    } else {
        w.visual_media.clear();
    }

    // Video expand state is now handled inside `visual_media`
    // (the gtk::Video lazy-instantiates as the 4th stack page on
    // user click). The row no longer carries a separate
    // gtk::Video / Overlay pair.

    // Audio compact
    let show_audio_compact =
        has_local && m.message_type == "audio" && !media_expanded;
    w.audio_compact_box.set_visible(show_audio_compact);
    if show_audio_compact {
        w.audio_compact_kind.set_label(&m.media_kind_label());
        w.audio_compact_summary
            .set_visible(!m.media_summary.is_empty());
        w.audio_compact_summary.set_label(&m.media_summary);
    }

    // Audio expanded
    let show_audio_expanded =
        has_local && m.message_type == "audio" && media_expanded;
    w.audio_controls.set_visible(show_audio_expanded);
    if show_audio_expanded {
        if let Some(p) = m.media_path.as_deref() {
            let stream = gtk::MediaFile::for_filename(p).upcast::<gtk::MediaStream>();
            w.audio_controls.set_media_stream(Some(&stream));
        }
    } else {
        w.audio_controls.set_media_stream(gtk::MediaStream::NONE);
    }

    // ── Generic download row (audio / document, no file) ──────────
    let show_generic_dl =
        !has_local && matches!(m.message_type.as_str(), "audio" | "document");
    w.generic_dl_box.set_visible(show_generic_dl);
    if show_generic_dl {
        w.generic_dl_icon.set_icon_name(Some(m.placeholder_icon()));
        w.generic_dl_kind.set_label(&m.media_kind_label());
        w.generic_dl_summary.set_visible(!m.media_summary.is_empty());
        w.generic_dl_summary.set_label(&m.media_summary);
        let downloading = m.media_status == "downloading";
        w.generic_dl_button.set_visible(!downloading);
        w.generic_dl_spinner.set_visible(downloading);
        if !downloading {
            let icon = match m.media_status.as_str() {
                "failed" => "view-refresh-symbolic",
                _ => "folder-download-symbolic",
            };
            w.generic_dl_button.set_icon_name(icon);
            let tip = match m.media_status.as_str() {
                "failed" => fl!("download-retry"),
                _ => fl!("download-download"),
            };
            w.generic_dl_button.set_tooltip_text(Some(&tip));
        }
    }

    // ── Document downloaded ───────────────────────────────────────
    w.document_open_button
        .set_visible(has_local && m.message_type == "document");

    // ── Text / caption ────────────────────────────────────────────
    let show_text = !m.is_media() || m.caption().is_some();
    w.content_label.set_visible(show_text);
    if show_text {
        w.content_label.set_markup(&m.cached_markup);
    }

    // ── Refresh per-slot context for click handlers ───────────────
    *w.slot.borrow_mut() = Some(RowContext {
        message_id: m.id.clone(),
        message_type: m.message_type.clone(),
        media_path: m.media_path.clone(),
        media_status: m.media_status.clone(),
        quoted_message_id: m.quoted_message_id.clone(),
        sender: item.sender.clone(),
        ui_state: item.ui_state.clone(),
    });

    // No-op on root usage — the `mut root` borrow is required by the
    // RelmListItem trait but we configure everything via the widget
    // bundle. Touching it would only matter for hover-state CSS that
    // depends on the bound row's content (none currently).
    let _ = root;
}

pub fn unbind(w: &mut MessageRowWidgets) {
    // Drop the per-slot context so a click on a recycled-but-
    // not-yet-rebound row no-ops. Without this, fast scroll could
    // dispatch a download for the previous item the moment a click
    // races the rebind pass.
    *w.slot.borrow_mut() = None;
}
