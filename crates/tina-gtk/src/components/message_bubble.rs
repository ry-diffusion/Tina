// Single message bubble inside the chat thread. Plain-text only for now;
// image/audio/video bubbles will hang off the same factory once the worker
// starts surfacing media payloads.

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
    pub content: String,
    pub message_type: String,
    pub timestamp: String,
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
        }
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
                        set_spacing: 2,
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

                        gtk::Label {
                            set_label: &self.item.content,
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
