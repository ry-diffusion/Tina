// Single message bubble inside the chat thread.
//
// Two visual modes:
//   * Image-as-bubble — downloaded images and stickers render edge-to-edge
//     (no card frame), with an OSD timestamp overlaid on the bottom-right
//     and the caption rendered below. Mirrors WhatsApp's photo style.
//   * Standard bubble — text, audio, video, document and any non-yet-
//     downloaded media. Wrapped in a regular AdwCard frame.
//
// Un-downloaded image/video/sticker render as a square placeholder with a
// big download circular button in the centre (also WhatsApp-style); audio
// and document fall back to a compact horizontal row.

use adw::prelude::*;
use gtk::gio;
use relm4::factory::{DynamicIndex, FactoryComponent};
use relm4::prelude::*;
use relm4::FactorySender;
use tina_db::MessageRow;

use crate::time::format_message_time;

#[derive(Debug)]
pub enum MessageBubbleOut {
    DownloadRequested(String),
}

#[derive(Debug, Clone)]
pub enum MessageBubbleInput {
    /// Mutate the item in place — preserves the row widget so the
    /// listbox doesn't reallocate (no scroll jump on download click).
    UpdateMedia {
        path: Option<String>,
        status: String,
        mimetype: Option<String>,
    },
    /// Local-echo bubble was confirmed by the server: replace the
    /// temporary id (and any other "in transit" state) with the real
    /// row data. Currently a no-op since we drop the local entirely on
    /// match — left here so the wiring is in place.
    Confirmed,
}

#[derive(Debug, Clone)]
pub struct MessageItem {
    pub id: String,
    pub from_me: bool,
    pub sender_name: String,
    pub show_sender: bool,
    pub content: String,
    pub message_type: String,
    pub timestamp: String,
    /// Raw unix timestamp — kept alongside the formatted version so the
    /// chat tab can recompute `oldest_ts` after dropping rows from the
    /// top of the factory.
    pub timestamp_unix: i64,
    pub media_summary: String,
    pub media_mimetype: Option<String>,
    pub media_size_bytes: Option<i64>,
    pub media_duration_secs: Option<i64>,
    pub media_path: Option<String>,
    pub media_status: String,
    pub media_filename: Option<String>,
}

impl MessageItem {
    pub fn from_row(row: &MessageRow, show_sender: bool) -> Self {
        let content = row.content.clone().unwrap_or_default();
        let display = if content.is_empty() {
            format!("[{}]", row.message_type)
        } else {
            content
        };
        Self {
            id: row.message_id.clone(),
            from_me: row.is_from_me,
            sender_name: row.sender_name.clone().unwrap_or_default(),
            show_sender,
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
        }
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

    /// `true` when the bubble should drop the card frame and render the
    /// media as the bubble itself. Applies to ANY image / sticker / video,
    /// regardless of download state — the placeholder is also shown in the
    /// edge-to-edge style so the visual doesn't jump when the download
    /// resolves.
    fn show_as_image_bubble(&self) -> bool {
        matches!(self.message_type.as_str(), "image" | "sticker" | "video")
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

            gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,
                set_margin_top: 2,
                set_margin_bottom: 2,
                set_margin_start: 8,
                set_margin_end: 8,
                set_halign: if self.item.from_me {
                    gtk::Align::End
                } else {
                    gtk::Align::Start
                },

                // ━━━━━━━━━━━ MODE A: Image-as-bubble ━━━━━━━━━━━
                // Image (or sticker) is rendered as the bubble itself —
                // no surrounding card, just the picture with an OSD
                // timestamp overlaid. WhatsApp / Signal style.
                gtk::Box {
                    set_visible: self.item.show_as_image_bubble(),
                    set_orientation: gtk::Orientation::Vertical,
                    set_spacing: 4,

                    gtk::Label {
                        set_visible: self.item.show_sender
                            && !self.item.from_me
                            && !self.item.sender_name.is_empty(),
                        set_label: &self.item.sender_name,
                        set_xalign: 0.0,
                        add_css_class: "caption-heading",
                        add_css_class: "accent",
                    },

                    gtk::Overlay {
                        #[wrap(Some)]
                        set_child = &gtk::Frame {
                            add_css_class: "card",
                            set_size_request: (
                                if self.item.message_type == "sticker" { 128 } else { 280 },
                                if self.item.message_type == "sticker" { 128 } else { 210 },
                            ),

                            // Loaded image / sticker. #[watch] so the
                            // visibility flips when MediaReady arrives.
                            gtk::Picture {
                                #[watch]
                                set_visible: self.item.has_local_file()
                                    && matches!(
                                        self.item.message_type.as_str(),
                                        "image" | "sticker"
                                    ),
                                #[watch]
                                set_filename: self.item.media_path.as_deref(),
                                set_can_shrink: true,
                            },
                        },

                        add_overlay = &gtk::Video {
                            #[watch]
                            set_visible: self.item.has_local_file()
                                && self.item.message_type == "video",
                            #[watch]
                            set_filename: self.item.media_path
                                .as_deref()
                                .filter(|_| self.item.message_type == "video"),
                        },

                        add_overlay = &gtk::Image {
                            #[watch]
                            set_visible: !self.item.has_local_file(),
                            set_icon_name: Some(self.item.placeholder_icon()),
                            set_pixel_size: 56,
                            set_halign: gtk::Align::Center,
                            set_valign: gtk::Align::Center,
                            add_css_class: "dim-label",
                        },

                        add_overlay = &gtk::Button {
                            #[watch]
                            set_visible: !self.item.has_local_file()
                                && self.item.media_status != "downloading",
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
                            set_visible: !self.item.has_local_file()
                                && self.item.media_status == "downloading",
                            set_halign: gtk::Align::Center,
                            set_valign: gtk::Align::Center,
                            set_spinning: true,
                            set_width_request: 32,
                            set_height_request: 32,
                        },

                        add_overlay = &gtk::Box {
                            #[watch]
                            set_visible: !self.item.has_local_file()
                                && !self.item.media_summary.is_empty(),
                            set_halign: gtk::Align::Start,
                            set_valign: gtk::Align::End,
                            set_margin_start: 8,
                            set_margin_bottom: 8,
                            add_css_class: "osd",
                            add_css_class: "card",
                            gtk::Label {
                                set_label: &self.item.media_summary,
                                set_margin_start: 6,
                                set_margin_end: 6,
                                set_margin_top: 1,
                                set_margin_bottom: 1,
                                add_css_class: "caption",
                            },
                        },

                        // Bottom-right pill: timestamp.
                        add_overlay = &gtk::Box {
                            set_halign: gtk::Align::End,
                            set_valign: gtk::Align::End,
                            set_margin_end: 8,
                            set_margin_bottom: 8,
                            add_css_class: "osd",
                            add_css_class: "card",

                            gtk::Label {
                                set_label: &self.item.timestamp,
                                set_margin_start: 8,
                                set_margin_end: 8,
                                set_margin_top: 2,
                                set_margin_bottom: 2,
                                add_css_class: "caption",
                            },
                        },
                    },

                    // Caption below the image, no bubble.
                    gtk::Label {
                        set_visible: self.item.caption().is_some(),
                        set_label: self.item.caption().unwrap_or(""),
                        set_xalign: 0.0,
                        set_wrap: true,
                        set_wrap_mode: gtk::pango::WrapMode::WordChar,
                        set_selectable: true,
                        set_max_width_chars: 50,
                    },
                },

                // ━━━━━━━━━━━ MODE B: Standard bubble ━━━━━━━━━━━
                gtk::Frame {
                    set_visible: !self.item.show_as_image_bubble(),
                    add_css_class: "card",
                    add_css_class: if self.item.from_me { "accent" } else { "view" },

                    gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                        set_spacing: 4,
                        set_margin_top: 6,
                        set_margin_bottom: 6,
                        set_margin_start: 10,
                        set_margin_end: 10,

                        gtk::Label {
                            set_visible: self.item.show_sender
                                && !self.item.from_me
                                && !self.item.sender_name.is_empty(),
                            set_label: &self.item.sender_name,
                            set_xalign: 0.0,
                            add_css_class: "caption-heading",
                            add_css_class: "accent",
                        },

                        // Image / sticker / video are rendered in MODE A
                        // above; here we only handle audio / document /
                        // text. ── Compact row: audio / document not yet
                        // downloaded.
                        gtk::Box {
                            #[watch]
                            set_visible: !self.item.has_local_file()
                                && matches!(
                                    self.item.message_type.as_str(),
                                    "audio" | "document"
                                ),
                            set_orientation: gtk::Orientation::Horizontal,
                            set_spacing: 10,

                            gtk::Image {
                                set_icon_name: Some(self.item.placeholder_icon()),
                                set_pixel_size: 32,
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

                        // ── Audio downloaded: inline MediaControls.
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

                        // ── Document downloaded: open externally.
                        gtk::Button {
                            #[watch]
                            set_visible: self.item.has_local_file()
                                && self.item.message_type == "document",
                            set_label: "Open externally",
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

                        // ── Caption / text.
                        gtk::Label {
                            set_visible: !self.item.is_media() || self.item.caption().is_some(),
                            set_label: self.item.caption().unwrap_or(&self.item.content),
                            set_xalign: 0.0,
                            set_wrap: true,
                            set_wrap_mode: gtk::pango::WrapMode::WordChar,
                            set_selectable: true,
                            set_max_width_chars: 60,
                        },

                        gtk::Label {
                            set_label: &self.item.timestamp,
                            set_xalign: 1.0,
                            add_css_class: "dim-label",
                            add_css_class: "caption",
                        },
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
            MessageBubbleInput::Confirmed => {}
        }
    }
}
