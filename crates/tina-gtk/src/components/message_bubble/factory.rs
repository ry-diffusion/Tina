// `MessageBubble` factory — one row in the message thread, Dissent-style:
// no bubbles, uniform left-aligned layout. Two visual modes:
//
//   * cozy — avatar slot (left) + header (sender name + timestamp) +
//     content. Used as the first message in a sender's run.
//   * collapsed — empty avatar-width slot whose only contents is a
//     timestamp shown on hover, plus the content. Used for runs of
//     messages from the same sender within ~10 minutes.
//
// Hover/focus highlighting is driven by a `.message-box` CSS class. The
// factory itself is dumb — `ChatTab` decides cozy-vs-collapsed when
// constructing each `MessageItem`.

use adw::prelude::*;
use gtk::gio;
use relm4::FactorySender;
use relm4::factory::{DynamicIndex, FactoryComponent};
use relm4::prelude::*;

use super::image::load_image_paintable;
use super::item::MessageItem;
use super::lightbox::open_media_lightbox;

/// Width of the left "gutter" column (avatar OR hover-timestamp).
/// Matches the avatar size + horizontal margins so cozy and collapsed
/// rows line up vertically along the content edge.
pub const GUTTER_WIDTH: i32 = 56;
const AVATAR_SIZE: i32 = 36;

/// Inline media heights. Videos get more vertical room because portrait
/// shoot (TikToks, Reels) is the dominant case in chat.
const IMAGE_HEIGHT: i32 = 360;
const VIDEO_HEIGHT: i32 = 480;
const STICKER_SIZE: i32 = 128;

#[derive(Debug)]
pub enum MessageBubbleOut {
    DownloadRequested(String),
}

#[derive(Debug, Clone)]
pub enum MessageBubbleInput {
    UpdateMedia {
        path: Option<String>,
        status: String,
        mimetype: Option<String>,
    },
    /// Resolved sender avatar arrived. Setting it on the item flips the
    /// `set_custom_image` binding so the AdwAvatar paints the picture.
    SetAvatar(String),
    /// Back-fill `sender_jid` on existing rows (e.g. from_me rows built
    /// before identity was known). Lets future `AvatarReady` broadcasts
    /// match these rows.
    SetSenderJid(String),
    Confirmed,
}

pub struct MessageBubble {
    pub item: MessageItem,
    /// Live media path for the lightbox click closures. The closures
    /// are wired in the `view!` macro at row-construction time and
    /// can't re-read `self.item.media_path` later — so without this
    /// shared cell, a freshly-downloaded file (path = None at init,
    /// flipped to Some(_) by `UpdateMedia`) is rendered correctly via
    /// `#[watch]` but its click handler still sees the captured None.
    media_path_cell: std::rc::Rc<std::cell::RefCell<Option<String>>>,
}

#[relm4::factory(pub)]
impl FactoryComponent for MessageBubble {
    type Init = MessageItem;
    type Input = MessageBubbleInput;
    type Output = MessageBubbleOut;
    type CommandOutput = ();
    type ParentWidget = gtk::ListBox;

    view! {
        gtk::ListBoxRow {
            set_activatable: false,
            set_selectable: false,
            add_css_class: "message-row",

            gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,
                set_spacing: 0,
                add_css_class: "message-box",
                add_css_class: if self.item.is_collapsed {
                    "message-collapsed"
                } else {
                    "message-cozy"
                },

                // ── Gutter: avatar (cozy) OR hover-only timestamp (collapsed)
                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_size_request: (GUTTER_WIDTH, -1),
                    set_valign: gtk::Align::Start,

                    adw::Avatar {
                        set_visible: !self.item.is_collapsed,
                        set_size: AVATAR_SIZE,
                        set_show_initials: true,
                        set_text: Some(self.item.display_sender_name()),
                        #[watch]
                        set_custom_image: self.item.display_avatar_path()
                            .and_then(|p| gtk::gdk::Texture::from_filename(p).ok())
                            .map(|t| t.upcast::<gtk::gdk::Paintable>())
                            .as_ref(),
                        set_margin_top: 4,
                        add_css_class: "message-cozy-avatar",
                    },

                    gtk::Label {
                        set_visible: self.item.is_collapsed,
                        set_label: self.item.short_timestamp(),
                        set_xalign: 0.5,
                        set_valign: gtk::Align::Start,
                        set_margin_top: 4,
                        add_css_class: "message-collapsed-timestamp",
                    },
                },

                // ── Right column: header + content
                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_hexpand: true,
                    set_spacing: 2,
                    set_margin_end: 12,

                    gtk::Label {
                        set_visible: !self.item.is_collapsed,
                        set_use_markup: true,
                        set_label: &self.item.header_markup(),
                        set_xalign: 0.0,
                        set_ellipsize: gtk::pango::EllipsizeMode::End,
                        set_single_line_mode: true,
                        add_css_class: "message-cozy-header",
                    },

                    // Image / sticker / video placeholder (not yet
                    // downloaded). Same dimensions as the downloaded
                    // gtk::Picture below so swapping in the real file
                    // doesn't reflow the thread. When the proto carried
                    // an inline thumbnail, paint it as the background;
                    // the icon falls through only when we have nothing
                    // better.
                    gtk::Overlay {
                        #[watch]
                        set_visible: self.item.is_visual_media() && !self.item.has_local_file(),
                        set_halign: gtk::Align::Start,
                        #[wrap(Some)]
                        set_child = &gtk::Picture {
                            set_size_request: (
                                match self.item.message_type.as_str() {
                                    "sticker" => STICKER_SIZE,
                                    _ => -1,
                                },
                                match self.item.message_type.as_str() {
                                    "sticker" => STICKER_SIZE,
                                    "video" => VIDEO_HEIGHT,
                                    _ => IMAGE_HEIGHT,
                                },
                            ),
                            #[watch]
                            set_paintable: self.item.thumbnail_paintable().as_ref(),
                            set_can_shrink: true,
                            add_css_class: "message-picture",
                        },

                        add_overlay = &gtk::Image {
                            #[watch]
                            set_visible: self.item.thumbnail.is_none()
                                || self.item.thumbnail.as_ref().map(|b| b.is_empty()).unwrap_or(true),
                            set_icon_name: Some(self.item.placeholder_icon()),
                            set_pixel_size: 48,
                            set_halign: gtk::Align::Center,
                            set_valign: gtk::Align::Center,
                            add_css_class: "dim-label",
                        },

                        add_overlay = &gtk::Button {
                            #[watch]
                            set_visible: self.item.media_status != "downloading",
                            set_halign: gtk::Align::Center,
                            set_valign: gtk::Align::Center,
                            #[watch]
                            set_icon_name: match self.item.media_status.as_str() {
                                "failed" => "view-refresh-symbolic",
                                _ => "folder-download-symbolic",
                            },
                            add_css_class: "circular",
                            add_css_class: "osd",
                            #[watch]
                            set_tooltip_text: Some(match self.item.media_status.as_str() {
                                "failed" => "Retry download",
                                _ => "Download",
                            }),
                            connect_clicked[sender, id = self.item.id.clone()] => move |_| {
                                let _ = sender.output(
                                    MessageBubbleOut::DownloadRequested(id.clone())
                                );
                            },
                        },

                        add_overlay = &gtk::Spinner {
                            #[watch]
                            set_visible: self.item.media_status == "downloading",
                            set_halign: gtk::Align::Center,
                            set_valign: gtk::Align::Center,
                            set_spinning: true,
                            set_width_request: 32,
                            set_height_request: 32,
                        },
                    },

                    // Image / sticker downloaded. Sticker swaps to a
                    // smaller square so we don't render a giant 360-px-
                    // tall sticker on full-resolution images. Click
                    // opens a fullscreen lightbox.
                    //
                    // We don't use `set_filename` — that bottoms out in
                    // `gdk::Texture::from_filename`, which since GTK
                    // 4.16-ish has internal decoders for PNG/JPEG and
                    // only falls back to GdkPixbuf for "unknown"
                    // formats. WebP detection in that fallback chain
                    // has been flaky across GTK versions, so for image
                    // and sticker payloads we go through GdkPixbuf
                    // explicitly: that path is guaranteed to honour
                    // `webp-pixbuf-loader` (and any other registered
                    // loaders) without depending on whatever GTK's
                    // texture loader decides today.
                    gtk::Picture {
                        #[watch]
                        set_visible: self.item.has_local_file()
                            && matches!(self.item.message_type.as_str(), "image" | "sticker"),
                        // Gate the decode on `message_type` too:
                        // `set_visible` only hides the widget, but the
                        // paintable expression still runs for every
                        // bubble — and feeding GdkPixbuf an audio file
                        // (e.g. an OGG voice note) was logging
                        // "Couldn't recognise the image file format"
                        // for every audio message in the tab.
                        #[watch]
                        set_paintable: load_image_paintable(
                            self.item.media_path
                                .as_deref()
                                .filter(|_| matches!(
                                    self.item.message_type.as_str(),
                                    "image" | "sticker"
                                ))
                        )
                            .as_ref()
                            .map(|t| t.upcast_ref::<gtk::gdk::Paintable>()),
                        set_can_shrink: true,
                        set_halign: gtk::Align::Start,
                        #[watch]
                        set_size_request: (
                            if self.item.message_type == "sticker" { STICKER_SIZE } else { -1 },
                            if self.item.message_type == "sticker" { STICKER_SIZE } else { IMAGE_HEIGHT },
                        ),
                        add_css_class: "message-picture",
                        set_cursor_from_name: Some("pointer"),
                        add_controller = gtk::GestureClick {
                            connect_released[
                                path_cell = self.media_path_cell.clone(),
                                kind = self.item.message_type.clone()
                            ] => move |gesture, _, _, _| {
                                let Some(widget) = gesture.widget() else { return };
                                let path = path_cell.borrow().clone();
                                if let Some(p) = path {
                                    open_media_lightbox(&widget, p, kind.clone());
                                }
                            },
                        },
                    },

                    // Video downloaded. Wrapped in an Overlay so we can
                    // pin an "expand" button in the corner — clicking
                    // the video itself toggles play/pause via the
                    // built-in MediaControls, so a separate affordance
                    // is needed for the lightbox.
                    gtk::Overlay {
                        #[watch]
                        set_visible: self.item.has_local_file()
                            && self.item.message_type == "video",
                        set_halign: gtk::Align::Start,

                        #[wrap(Some)]
                        set_child = &gtk::Video {
                            #[watch]
                            set_filename: self.item.media_path
                                .as_deref()
                                .filter(|_| self.item.message_type == "video"),
                            set_size_request: (-1, VIDEO_HEIGHT),
                        },

                        add_overlay = &gtk::Button {
                            set_icon_name: "view-fullscreen-symbolic",
                            set_tooltip_text: Some("Expand"),
                            set_halign: gtk::Align::End,
                            set_valign: gtk::Align::Start,
                            set_margin_top: 8,
                            set_margin_end: 8,
                            add_css_class: "circular",
                            add_css_class: "osd",
                            connect_clicked[
                                path_cell = self.media_path_cell.clone(),
                                kind = self.item.message_type.clone()
                            ] => move |btn| {
                                let path = path_cell.borrow().clone();
                                if let Some(p) = path {
                                    open_media_lightbox(btn, p, kind.clone());
                                }
                            },
                        },
                    },

                    // Audio (downloaded).
                    gtk::MediaControls {
                        #[watch]
                        set_visible: self.item.has_local_file()
                            && self.item.message_type == "audio",
                        #[watch]
                        set_media_stream: self.item.media_path
                            .as_deref()
                            .filter(|_| self.item.message_type == "audio")
                            .map(|p| gtk::MediaFile::for_filename(p).upcast::<gtk::MediaStream>())
                            .as_ref(),
                    },

                    // Audio / document not yet downloaded — compact row.
                    gtk::Box {
                        #[watch]
                        set_visible: !self.item.has_local_file()
                            && matches!(self.item.message_type.as_str(), "audio" | "document"),
                        set_orientation: gtk::Orientation::Horizontal,
                        set_spacing: 10,
                        set_margin_top: 4,
                        set_margin_bottom: 4,

                        gtk::Image {
                            set_icon_name: Some(self.item.placeholder_icon()),
                            set_pixel_size: 28,
                            add_css_class: "dim-label",
                        },

                        gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_spacing: 2,
                            set_valign: gtk::Align::Center,
                            set_hexpand: true,

                            gtk::Label {
                                set_label: self.item.media_kind_label(),
                                set_xalign: 0.0,
                                add_css_class: "heading",
                            },
                            gtk::Label {
                                set_visible: !self.item.media_summary.is_empty(),
                                set_label: &self.item.media_summary,
                                set_xalign: 0.0,
                                add_css_class: "dim-label",
                                add_css_class: "caption",
                            },
                        },

                        gtk::Button {
                            #[watch]
                            set_visible: self.item.media_status != "downloading",
                            #[watch]
                            set_icon_name: match self.item.media_status.as_str() {
                                "failed" => "view-refresh-symbolic",
                                _ => "folder-download-symbolic",
                            },
                            #[watch]
                            set_tooltip_text: Some(match self.item.media_status.as_str() {
                                "failed" => "Retry download",
                                _ => "Download",
                            }),
                            add_css_class: "circular",
                            add_css_class: "flat",
                            set_valign: gtk::Align::Center,
                            connect_clicked[sender, id = self.item.id.clone()] => move |_| {
                                let _ = sender.output(
                                    MessageBubbleOut::DownloadRequested(id.clone())
                                );
                            },
                        },

                        gtk::Spinner {
                            #[watch]
                            set_visible: self.item.media_status == "downloading",
                            set_spinning: true,
                            set_valign: gtk::Align::Center,
                            set_width_request: 18,
                            set_height_request: 18,
                        },
                    },

                    // Document downloaded — open externally.
                    gtk::Button {
                        #[watch]
                        set_visible: self.item.has_local_file()
                            && self.item.message_type == "document",
                        set_label: "Open externally",
                        set_halign: gtk::Align::Start,
                        connect_clicked[path_cell = self.media_path_cell.clone()] => move |_| {
                            let path = path_cell.borrow().clone();
                            if let Some(p) = path {
                                let file = gio::File::for_path(&p);
                                let launcher = gtk::FileLauncher::new(Some(&file));
                                launcher.launch(
                                    gtk::Window::NONE,
                                    gio::Cancellable::NONE,
                                    |_| {},
                                );
                            }
                        },
                    },

                    // Text / caption — rendered as Pango markup so
                    // WhatsApp's `*bold*`, `_italic_`, `~strike~`,
                    // backtick code spans, and bare URLs come through
                    // looking like the official client. Conversion
                    // lives in `format::wa_markdown_to_pango`.
                    gtk::Label {
                        set_visible: !self.item.is_media() || self.item.caption().is_some(),
                        set_use_markup: true,
                        set_markup: &super::format::wa_markdown_to_pango(
                            self.item.caption().unwrap_or(&self.item.content),
                        ),
                        set_xalign: 0.0,
                        set_wrap: true,
                        set_wrap_mode: gtk::pango::WrapMode::WordChar,
                        set_selectable: true,
                        add_css_class: "message-content",
                    },
                },
            },
        }
    }

    fn init_model(init: Self::Init, _index: &DynamicIndex, _sender: FactorySender<Self>) -> Self {
        let media_path_cell =
            std::rc::Rc::new(std::cell::RefCell::new(init.media_path.clone()));
        Self {
            item: init,
            media_path_cell,
        }
    }

    fn update(&mut self, msg: Self::Input, _sender: FactorySender<Self>) {
        match msg {
            MessageBubbleInput::UpdateMedia {
                path,
                status,
                mimetype,
            } => {
                *self.media_path_cell.borrow_mut() = path.clone();
                self.item.media_path = path;
                self.item.media_status = status;
                if self.item.media_mimetype.is_none() {
                    self.item.media_mimetype = mimetype;
                }
            }
            MessageBubbleInput::SetAvatar(path) => {
                self.item.sender_avatar_path = Some(path);
            }
            MessageBubbleInput::SetSenderJid(jid) => {
                self.item.sender_jid = Some(jid);
            }
            MessageBubbleInput::Confirmed => {}
        }
    }
}
