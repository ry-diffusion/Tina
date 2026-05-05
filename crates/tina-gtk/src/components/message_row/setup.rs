// Widget construction for one recycled row slot. Runs once per slot
// allocated by `gtk::ListView`; the same widget tree is reused across
// many bound items. All click handlers / gesture controllers are
// wired here; they read through `widgets.slot` so a click sees
// whatever the most recent `bind()` wrote, not the first item that
// was ever bound to this slot.

use std::cell::RefCell;
use crate::fl;
use std::rc::Rc;

use adw::prelude::*;

use super::super::chat_tab::messages::ChatTabInput;

use super::widgets::{MessageRowWidgets, RowContext};

/// Visual constants shared with the bind path. Same values the old
/// factory used so visual layout doesn't shift during the migration.
pub const GUTTER_WIDTH: i32 = 56;
pub const AVATAR_SIZE: i32 = 36;
pub const IMAGE_HEIGHT: i32 = 360;
pub const VIDEO_HEIGHT: i32 = 480;
pub const STICKER_SIZE: i32 = 128;

pub fn build_root_and_widgets() -> (gtk::Box, MessageRowWidgets) {
    let slot: Rc<RefCell<Option<RowContext>>> = Rc::new(RefCell::new(None));

    // Outermost: vertical wrapper holding the day-divider pill + the
    // horizontal message body. Same role as the wrapper Box added to
    // the old factory's view! macro for day dividers.
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .build();
    root.add_css_class("message-row");

    // ── Day divider ────────────────────────────────────────────────
    let day_divider_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .halign(gtk::Align::Center)
        .margin_top(12)
        .margin_bottom(4)
        .build();
    day_divider_box.add_css_class("message-day-divider");
    let day_divider_label = gtk::Label::new(None);
    day_divider_label.add_css_class("message-day-divider-label");
    day_divider_box.append(&day_divider_label);
    root.append(&day_divider_box);

    // ── Horizontal message body ───────────────────────────────────
    let message_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .build();
    message_box.add_css_class("message-box");
    root.append(&message_box);

    // ── Gutter (avatar OR collapsed timestamp) ────────────────────
    let gutter = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::Start)
        .build();
    gutter.set_size_request(GUTTER_WIDTH, -1);
    let avatar = adw::Avatar::builder()
        .size(AVATAR_SIZE)
        .show_initials(true)
        .margin_top(4)
        .build();
    avatar.add_css_class("message-cozy-avatar");
    gutter.append(&avatar);
    let collapsed_timestamp = gtk::Label::builder()
        .xalign(0.5)
        .valign(gtk::Align::Start)
        .margin_top(4)
        // Defense against the gutter expanding past 56 px when the
        // timestamp is wider (e.g. multi-day chats with `04/05 22:20`
        // formatted timestamps). Pairs with `short_time` keeping the
        // bound text at `HH:MM`; the ellipsis is just a backstop.
        .max_width_chars(5)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    collapsed_timestamp.add_css_class("message-collapsed-timestamp");
    gutter.append(&collapsed_timestamp);
    message_box.append(&gutter);

    // ── Right column ──────────────────────────────────────────────
    let right_col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .hexpand(true)
        .spacing(2)
        .margin_end(12)
        .build();
    message_box.append(&right_col);

    // Header row
    let header_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    let header_label = gtk::Label::builder()
        .use_markup(true)
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .single_line_mode(true)
        .build();
    header_label.add_css_class("message-cozy-header");
    let status_icon = gtk::Image::builder()
        .pixel_size(14)
        .valign(gtk::Align::Center)
        .build();
    header_box.append(&header_label);
    header_box.append(&status_icon);
    right_col.append(&header_box);

    // Reply button
    let reply_button = gtk::Button::builder()
        .halign(gtk::Align::Start)
        .build();
    reply_button.add_css_class("flat");
    reply_button.add_css_class("message-reply-button");
    let reply_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    reply_box.add_css_class("message-reply-box");
    let reply_author = gtk::Label::builder().xalign(0.0).build();
    reply_author.add_css_class("message-reply-author");
    let reply_preview = gtk::Label::builder()
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .single_line_mode(true)
        .build();
    reply_preview.add_css_class("message-reply-preview");
    reply_box.append(&reply_author);
    reply_box.append(&reply_preview);
    reply_button.set_child(Some(&reply_box));
    right_col.append(&reply_button);
    {
        let slot_c = slot.clone();
        reply_button.connect_clicked(move |_| {
            let Some(ctx) = slot_c.borrow().as_ref().map(|c| (c.quoted_message_id.clone(), c.sender.clone())) else {
                return;
            };
            if let Some(target) = ctx.0
                && !target.is_empty()
            {
                let _ = ctx.1.send(ChatTabInput::JumpToMessage(target));
            }
        });
    }

    // ── Visual media (image / sticker / video thumbnail) ──────────
    // One custom widget replaces the placeholder + image_picture +
    // video_thumb trio. The widget owns its own size caps, stack
    // crossfade, and click dispatch — see message_media::imp.
    let visual_media = super::super::message_media::TinaMessageMedia::new();
    visual_media.set_halign(gtk::Align::Start);
    right_col.append(&visual_media);

    // ── Audio compact (downloaded, not expanded) ──────────────────
    let audio_compact_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .margin_top(4)
        .margin_bottom(4)
        .build();
    let audio_play_btn = gtk::Button::builder()
        .icon_name("media-playback-start-symbolic")
        .tooltip_text(&fl!("play"))
        .valign(gtk::Align::Center)
        .build();
    audio_play_btn.add_css_class("circular");
    audio_compact_box.append(&audio_play_btn);
    let audio_compact_text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .build();
    let audio_compact_kind = gtk::Label::builder().xalign(0.0).build();
    audio_compact_kind.add_css_class("heading");
    let audio_compact_summary = gtk::Label::builder().xalign(0.0).build();
    audio_compact_summary.add_css_class("dim-label");
    audio_compact_summary.add_css_class("caption");
    audio_compact_text.append(&audio_compact_kind);
    audio_compact_text.append(&audio_compact_summary);
    audio_compact_box.append(&audio_compact_text);
    right_col.append(&audio_compact_box);
    {
        let slot_c = slot.clone();
        audio_play_btn.connect_clicked(move |_| expand_media(&slot_c));
    }

    // ── Audio controls (expanded) ─────────────────────────────────
    let audio_controls = gtk::MediaControls::builder().build();
    right_col.append(&audio_controls);

    // ── Generic download row (audio/document not yet downloaded) ──
    let generic_dl_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .margin_top(4)
        .margin_bottom(4)
        .build();
    let generic_dl_icon = gtk::Image::builder().pixel_size(28).build();
    generic_dl_icon.add_css_class("dim-label");
    let generic_dl_text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .build();
    let generic_dl_kind = gtk::Label::builder().xalign(0.0).build();
    generic_dl_kind.add_css_class("heading");
    let generic_dl_summary = gtk::Label::builder().xalign(0.0).build();
    generic_dl_summary.add_css_class("dim-label");
    generic_dl_summary.add_css_class("caption");
    generic_dl_text.append(&generic_dl_kind);
    generic_dl_text.append(&generic_dl_summary);
    let generic_dl_button = gtk::Button::builder().valign(gtk::Align::Center).build();
    generic_dl_button.add_css_class("circular");
    generic_dl_button.add_css_class("flat");
    let generic_dl_spinner = gtk::Spinner::builder()
        .valign(gtk::Align::Center)
        .width_request(18)
        .height_request(18)
        .build();
    generic_dl_spinner.set_spinning(true);
    generic_dl_box.append(&generic_dl_icon);
    generic_dl_box.append(&generic_dl_text);
    generic_dl_box.append(&generic_dl_button);
    generic_dl_box.append(&generic_dl_spinner);
    right_col.append(&generic_dl_box);
    {
        let slot_c = slot.clone();
        generic_dl_button.connect_clicked(move |_| {
            let Some((id, sender)) = slot_c
                .borrow()
                .as_ref()
                .map(|c| (c.message_id.clone(), c.sender.clone()))
            else { return };
            let _ = sender.send(ChatTabInput::RequestMediaDownload(id));
        });
    }

    // ── Document downloaded (open externally) ─────────────────────
    let document_open_button = gtk::Button::builder()
        .label(&fl!("open-externally"))
        .halign(gtk::Align::Start)
        .build();
    right_col.append(&document_open_button);
    {
        let slot_c = slot.clone();
        document_open_button.connect_clicked(move |_| {
            let Some(path) = slot_c.borrow().as_ref().and_then(|c| c.media_path.clone()) else {
                return;
            };
            let file = gtk::gio::File::for_path(&path);
            let launcher = gtk::FileLauncher::new(Some(&file));
            launcher.launch(
                gtk::Window::NONE,
                gtk::gio::Cancellable::NONE,
                |_| {},
            );
        });
    }

    // ── Text / caption ────────────────────────────────────────────
    let content_label = gtk::Label::builder()
        .use_markup(true)
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .selectable(true)
        .build();
    content_label.add_css_class("message-content");
    right_col.append(&content_label);

    let widgets = MessageRowWidgets {
        day_divider_box,
        day_divider_label,
        message_box,
        avatar,
        collapsed_timestamp,
        header_box,
        header_label,
        status_icon,
        reply_button,
        reply_author,
        reply_preview,
        visual_media,
        audio_compact_box,
        audio_compact_kind,
        audio_compact_summary,
        audio_controls,
        generic_dl_box,
        generic_dl_icon,
        generic_dl_kind,
        generic_dl_summary,
        generic_dl_button,
        generic_dl_spinner,
        document_open_button,
        content_label,
        slot,
    };

    (root, widgets)
}

/// Flip `media_expanded` for the row currently bound to this slot
/// and ask the chat tab to rebind it (which causes the bind pass to
/// swap visibility from compact / thumb → controls / video).
fn expand_media(slot: &Rc<RefCell<Option<RowContext>>>) {
    let Some((id, ui_state, sender)) = slot
        .borrow()
        .as_ref()
        .map(|c| (c.message_id.clone(), c.ui_state.clone(), c.sender.clone()))
    else { return };
    ui_state.set_media_expanded(&id, true);
    let _ = sender.send(ChatTabInput::RebindRow(id));
}
