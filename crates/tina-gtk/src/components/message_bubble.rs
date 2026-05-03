// Single message bubble inside the chat thread.
//
// Text-only payloads render the classic chat bubble. Media payloads (image,
// audio, video, sticker, document) render a structured placeholder with an
// icon, type-specific metadata (dimensions, duration, file size) and a
// "Download" affordance — PR2 wires those buttons to the actual download
// pipeline.

use adw::prelude::*;
use relm4::factory::{DynamicIndex, FactoryComponent};
use relm4::prelude::*;
use relm4::FactorySender;
use tina_db::MessageRow;

use crate::time::format_message_time;

#[derive(Debug, Clone)]
pub struct MessageItem {
    pub id: String,
    pub from_me: bool,
    pub sender_name: String,
    pub show_sender: bool,
    /// User-facing payload (caption / text). For pure media w/o caption,
    /// this holds the bracketed placeholder ("[Image]") so plain-text
    /// fallback doesn't render empty.
    pub content: String,
    pub message_type: String,
    pub timestamp: String,
    /// Pre-rendered media metadata line (e.g. "1024×768 · 1.2 MB"). Empty
    /// when no media or no metadata available.
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

    /// Returns the user-typed caption when distinct from the placeholder.
    fn caption(&self) -> Option<&str> {
        if self.content.starts_with('[') && self.content.ends_with(']') {
            None
        } else {
            Some(&self.content)
        }
    }
}

/// Builds the secondary descriptor line shown under the media-type header
/// (e.g. "1024×768 · 1.2 MB" for an image, "0:23 · 145 KB" for audio).
fn build_media_summary(row: &MessageRow) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let (Some(w), Some(h)) = (row.media_width, row.media_height) {
        if w > 0 && h > 0 {
            parts.push(format!("{w}×{h}"));
        }
    }

    if let Some(secs) = row.media_duration_secs {
        if secs > 0 {
            let m = secs / 60;
            let s = secs % 60;
            parts.push(format!("{m}:{s:02}"));
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
    item: MessageItem,
}

#[relm4::factory(pub)]
impl FactoryComponent for MessageBubble {
    type Init = MessageItem;
    type Input = ();
    type Output = ();
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

                        gtk::Box {
                            set_visible: self.item.is_media(),
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

                                // Status pill: "Tap to download" / "Downloading…"
                                // / "Failed". Wired to actions in PR2.
                                gtk::Label {
                                    set_visible: self.item.is_media()
                                        && self.item.media_path.is_none()
                                        && !matches!(
                                            self.item.media_status.as_str(),
                                            "downloading"
                                        ),
                                    set_label: match self.item.media_status.as_str() {
                                        "failed" => "Download failed",
                                        _ => "Tap to download",
                                    },
                                    set_xalign: 0.0,
                                    add_css_class: "caption",
                                    add_css_class: "accent",
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
