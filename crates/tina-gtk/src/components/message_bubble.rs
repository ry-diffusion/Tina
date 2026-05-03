// Single message bubble inside the chat thread.
//
// Three states per bubble:
//   * Plain text — classic chat bubble with the body and timestamp.
//   * Media without a local cache — placeholder (icon + metadata + a
//     clickable "Tap to download" pill that emits `DownloadRequested`).
//   * Media with a cached file — inline rendering: gtk::Picture for
//     images/stickers, gtk::MediaControls (driven by gtk::MediaFile) for
//     audio/video, "Open externally" button for documents.

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
pub struct MessageItem {
    pub id: String,
    pub from_me: bool,
    pub sender_name: String,
    pub show_sender: bool,
    pub content: String,
    pub message_type: String,
    pub timestamp: String,
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

    fn media_icon(&self) -> &'static str {
        match self.message_type.as_str() {
            "image" => "image-x-generic-symbolic",
            "audio" => "audio-x-generic-symbolic",
            "video" => "video-x-generic-symbolic",
            "sticker" => "emoji-symbols-symbolic",
            "document" => "text-x-generic-symbolic",
            _ => "mail-attachment-symbolic",
        }
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
    type Input = ();
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

                gtk::Frame {
                    set_halign: if self.item.from_me {
                        gtk::Align::End
                    } else {
                        gtk::Align::Start
                    },
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

                        // ── Inline rendering when the cache file is present ──
                        gtk::Picture {
                            set_visible: self.item.is_media()
                                && self.item.has_local_file()
                                && matches!(
                                    self.item.message_type.as_str(),
                                    "image" | "sticker"
                                ),
                            set_filename: self.item.media_path.as_deref(),
                            set_can_shrink: true,
                            // Hard caps so a 4000×3000 photo doesn't blow
                            // up a chat row to fullscreen.
                            set_size_request: (
                                if self.item.message_type == "sticker" { 96 } else { 280 },
                                -1,
                            ),
                            set_height_request: if self.item.message_type == "sticker" {
                                96
                            } else {
                                240
                            },
                        },

                        // Audio: play/pause + scrubber via gtk::MediaControls
                        // backed by a MediaFile (GTK plugs into gstreamer
                        // automatically when the platform has it).
                        gtk::MediaControls {
                            set_visible: self.item.is_media()
                                && self.item.has_local_file()
                                && self.item.message_type == "audio",
                            set_media_stream: self.item.media_path
                                .as_deref()
                                .filter(|_| self.item.message_type == "audio")
                                .map(|p| gtk::MediaFile::for_filename(p).upcast::<gtk::MediaStream>())
                                .as_ref(),
                        },

                        gtk::Video {
                            set_visible: self.item.is_media()
                                && self.item.has_local_file()
                                && self.item.message_type == "video",
                            set_filename: self.item.media_path
                                .as_deref()
                                .filter(|_| self.item.message_type == "video"),
                            set_size_request: (320, 240),
                        },

                        // Document: just an "Open externally" button —
                        // rendering arbitrary file types inline is out of
                        // scope.
                        gtk::Button {
                            set_visible: self.item.is_media()
                                && self.item.has_local_file()
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

                        // ── Placeholder block (shown only when no local file) ──
                        gtk::Box {
                            set_visible: self.item.is_media() && !self.item.has_local_file(),
                            set_orientation: gtk::Orientation::Horizontal,
                            set_spacing: 10,
                            set_margin_top: 4,
                            set_margin_bottom: 4,

                            gtk::Image {
                                set_icon_name: Some(self.item.media_icon()),
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

                                // Idle / failed → clickable button.
                                gtk::Button {
                                    set_visible: !self.item.has_local_file()
                                        && self.item.media_status != "downloading",
                                    set_label: match self.item.media_status.as_str() {
                                        "failed" => "Retry download",
                                        _ => "Tap to download",
                                    },
                                    add_css_class: "flat",
                                    add_css_class: "accent",
                                    connect_clicked[sender, id = self.item.id.clone()] => move |_| {
                                        let _ = sender.output(
                                            MessageBubbleOut::DownloadRequested(id.clone())
                                        );
                                    },
                                },

                                gtk::Box {
                                    set_visible: self.item.media_status == "downloading",
                                    set_orientation: gtk::Orientation::Horizontal,
                                    set_spacing: 6,
                                    gtk::Spinner {
                                        set_spinning: true,
                                        set_width_request: 14,
                                        set_height_request: 14,
                                    },
                                    gtk::Label {
                                        set_label: "Downloading…",
                                        add_css_class: "caption",
                                        add_css_class: "dim-label",
                                    },
                                },
                            },
                        },

                        // Caption / text payload. Hidden when the bubble
                        // is pure media without a user-typed caption.
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
}
