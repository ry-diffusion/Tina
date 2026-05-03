// One row in the message thread, Dissent-style: no bubbles, uniform
// left-aligned layout. Two visual modes:
//
//   * cozy — avatar slot (left) + header (sender name + timestamp) +
//     content. Used as the first message in a sender's run.
//   * collapsed — empty avatar-width slot whose only contents is a
//     timestamp shown on hover, plus the content. Used for runs of
//     messages from the same sender within ~10 minutes.
//
// Hover/focus highlighting is driven by a `.message-box` CSS class
// (registered via `set_global_css` at app start). The factory itself is
// dumb — `ChatTab` decides cozy-vs-collapsed when constructing each
// `MessageItem`.

use adw::prelude::*;
use gtk::gio;
use gtk::glib;
use relm4::FactorySender;
use relm4::factory::{DynamicIndex, FactoryComponent};
use relm4::prelude::*;
use tina_db::MessageRow;

use crate::time::format_message_time;

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

/// Open a Discord/WhatsApp-style media overlay: an `adw::Dialog`
/// (libadwaita's canonical "in-window modal") that presents over the
/// app's main window with the media filling the body, a HeaderBar
/// carrying Save / Open-externally actions, and Escape / close button
/// to dismiss.
///
/// `anchor` is any widget inside the parent window — `adw::Dialog`
/// walks the tree up to find the AdwApplicationWindow it should attach
/// to. We use the clicked widget itself.
pub fn open_media_lightbox(anchor: &impl IsA<gtk::Widget>, path: String, message_type: String) {
    let dialog = adw::Dialog::builder()
        .content_width(1100)
        .content_height(800)
        .build();

    let header = adw::HeaderBar::new();
    header.set_show_end_title_buttons(true);
    header.set_title_widget(Some(&adw::WindowTitle::new(
        match message_type.as_str() {
            "video" => "Video",
            "sticker" => "Sticker",
            _ => "Image",
        },
        &short_filename(&path),
    )));

    // "Open externally" — hands the file to the system handler. Useful
    // for video editors / image viewers / external default apps.
    {
        let btn = gtk::Button::from_icon_name("document-open-symbolic");
        btn.set_tooltip_text(Some("Open externally"));
        btn.add_css_class("flat");
        let p = path.clone();
        btn.connect_clicked(move |_| {
            let file = gio::File::for_path(&p);
            let launcher = gtk::FileLauncher::new(Some(&file));
            launcher.launch(gtk::Window::NONE, gio::Cancellable::NONE, |_| {});
        });
        header.pack_end(&btn);
    }

    // "Save as…" — copies the cached file into a user-chosen location.
    {
        let btn = gtk::Button::from_icon_name("document-save-symbolic");
        btn.set_tooltip_text(Some("Save as…"));
        btn.add_css_class("flat");
        let p = path.clone();
        let anchor_weak = anchor.upcast_ref::<gtk::Widget>().downgrade();
        btn.connect_clicked(move |_| {
            let save = gtk::FileDialog::builder()
                .title("Save media")
                .initial_name(default_save_name(&p))
                .modal(true)
                .build();
            let parent_window = anchor_weak
                .upgrade()
                .and_then(|w| w.root())
                .and_then(|r| r.downcast::<gtk::Window>().ok());
            let src = std::path::PathBuf::from(&p);
            save.save(parent_window.as_ref(), gio::Cancellable::NONE, move |res| {
                let Ok(file) = res else { return };
                let Some(dest) = file.path() else { return };
                if let Err(e) = std::fs::copy(&src, &dest) {
                    tracing::warn!(?dest, error = %e, "lightbox: save copy failed");
                }
            });
        });
        header.pack_end(&btn);
    }

    let body: gtk::Widget = match message_type.as_str() {
        "video" => {
            let video = gtk::Video::for_filename(Some(std::path::Path::new(&path)));
            video.set_autoplay(true);
            video.set_hexpand(true);
            video.set_vexpand(true);
            video.upcast()
        }
        _ => {
            let pic = gtk::Picture::for_filename(&path);
            pic.set_can_shrink(true);
            pic.set_content_fit(gtk::ContentFit::Contain);
            pic.set_hexpand(true);
            pic.set_vexpand(true);
            pic.upcast()
        }
    };

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&body));

    dialog.set_child(Some(&toolbar));
    dialog.present(Some(anchor.upcast_ref::<gtk::Widget>()));
}

fn short_filename(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn default_save_name(path: &str) -> String {
    let base = short_filename(path);
    if base.is_empty() {
        "media".to_string()
    } else {
        base
    }
}

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

    fn thumbnail_paintable(&self) -> Option<gtk::gdk::Paintable> {
        let bytes = self.thumbnail.as_ref()?;
        if bytes.is_empty() {
            return None;
        }
        gtk::gdk::Texture::from_bytes(&gtk::glib::Bytes::from(bytes.as_slice()))
            .ok()
            .map(|t| t.upcast::<gtk::gdk::Paintable>())
    }

    fn is_media(&self) -> bool {
        matches!(
            self.message_type.as_str(),
            "image" | "audio" | "video" | "sticker" | "document"
        )
    }

    fn is_visual_media(&self) -> bool {
        matches!(self.message_type.as_str(), "image" | "video" | "sticker")
    }

    fn media_kind_label(&self) -> &'static str {
        match self.message_type.as_str() {
            "image" => "Image",
            "audio" => "Voice / Audio",
            "video" => "Video",
            "sticker" => "Sticker",
            "document" => "Document",
            _ => "Attachment",
        }
    }

    fn placeholder_icon(&self) -> &'static str {
        match self.message_type.as_str() {
            "image" => "image-x-generic-symbolic",
            "audio" => "audio-x-generic-symbolic",
            "video" => "video-x-generic-symbolic",
            "sticker" => "emoji-symbols-symbolic",
            "document" => "text-x-generic-symbolic",
            _ => "mail-attachment-symbolic",
        }
    }

    fn caption(&self) -> Option<&str> {
        if self.content.starts_with('[') && self.content.ends_with(']') {
            None
        } else {
            Some(&self.content)
        }
    }

    fn has_local_file(&self) -> bool {
        self.media_path
            .as_deref()
            .map(|p| !p.is_empty())
            .unwrap_or(false)
    }

    fn header_markup(&self) -> String {
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

    fn short_timestamp(&self) -> &str {
        &self.timestamp
    }
}

fn glib_markup_escape(s: &str) -> String {
    gtk::glib::markup_escape_text(s).to_string()
}

fn build_media_summary(row: &MessageRow) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let (Some(w), Some(h)) = (row.media_width, row.media_height) {
        if w > 0 && h > 0 {
            parts.push(format!("{w}×{h}"));
        }
    }
    if let Some(secs) = row.media_duration_secs {
        if secs > 0 {
            parts.push(format!("{}:{:02}", secs / 60, secs % 60));
        }
    }
    if let Some(bytes) = row.media_size_bytes {
        if bytes > 0 {
            parts.push(format_size(bytes));
        }
    }
    if let Some(name) = row.media_filename.as_deref() {
        if !name.is_empty() {
            parts.push(name.to_string());
        }
    }
    parts.join(" · ")
}

fn format_size(bytes: i64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

pub struct MessageBubble {
    pub item: MessageItem,
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
                        set_text: Some(if self.item.from_me {
                            "You"
                        } else if self.item.sender_name.is_empty() {
                            "?"
                        } else {
                            self.item.sender_name.as_str()
                        }),
                        #[watch]
                        set_custom_image: self.item.sender_avatar_path
                            .as_deref()
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
                    gtk::Picture {
                        #[watch]
                        set_visible: self.item.has_local_file()
                            && matches!(self.item.message_type.as_str(), "image" | "sticker"),
                        #[watch]
                        set_filename: self.item.media_path.as_deref(),
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
                                path = self.item.media_path.clone(),
                                kind = self.item.message_type.clone()
                            ] => move |gesture, _, _, _| {
                                let Some(widget) = gesture.widget() else { return };
                                if let Some(p) = path.as_deref() {
                                    open_media_lightbox(&widget, p.to_string(), kind.clone());
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
                                path = self.item.media_path.clone(),
                                kind = self.item.message_type.clone()
                            ] => move |btn| {
                                if let Some(p) = path.as_deref() {
                                    open_media_lightbox(btn, p.to_string(), kind.clone());
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
                        connect_clicked[path = self.item.media_path.clone()] => move |_| {
                            if let Some(p) = path.as_deref() {
                                let file = gio::File::for_path(p);
                                let launcher = gtk::FileLauncher::new(Some(&file));
                                launcher.launch(
                                    gtk::Window::NONE,
                                    gio::Cancellable::NONE,
                                    |_| {},
                                );
                            }
                        },
                    },

                    // Text / caption.
                    gtk::Label {
                        set_visible: !self.item.is_media() || self.item.caption().is_some(),
                        set_label: self.item.caption().unwrap_or(&self.item.content),
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
        Self { item: init }
    }

    fn update(&mut self, msg: Self::Input, _sender: FactorySender<Self>) {
        match msg {
            MessageBubbleInput::UpdateMedia {
                path,
                status,
                mimetype,
            } => {
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

/// CSS shipped alongside the factory. Loaded once at app start via
/// `relm4::set_global_css`.
pub const MESSAGE_ROW_CSS: &str = r#"
.message-row {
    padding: 0;
}
.message-box {
    border: 2px solid transparent;
    transition: linear 150ms background-color;
    padding: 2px 4px;
}
.message-row:hover .message-box {
    background-color: alpha(@theme_fg_color, 0.06);
    transition: none;
}
.message-row:focus .message-box {
    background-color: alpha(@theme_fg_color, 0.10);
    transition: none;
}
.message-cozy {
    padding-top: 6px;
}
.message-collapsed {
    padding-top: 0;
}
.message-cozy-header {
    min-height: 1.4em;
}
.message-cozy-avatar {
    margin-left: 8px;
    margin-right: 12px;
}
.message-collapsed-timestamp {
    opacity: 0;
    font-size: 0.7em;
    color: alpha(@theme_fg_color, 0.7);
}
.message-row:hover .message-collapsed-timestamp,
.message-row:focus .message-collapsed-timestamp {
    opacity: 1;
}
.message-content {
    margin-top: 1px;
    margin-bottom: 1px;
}
.message-picture {
    border-radius: 6px;
}
"#;
