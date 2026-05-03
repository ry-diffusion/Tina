// Single row inside the sidebar's chat list. Used as a `FactoryComponent`
// so the parent (`MainPage`) can drive incremental upserts without rebuilding
// the whole list — the same upsert-by-chat_id pattern the Slint version
// used.

use adw::prelude::*;
use gtk::glib;
use relm4::factory::{DynamicIndex, FactoryComponent};
use relm4::prelude::*;
use relm4::FactorySender;

use tina_db::ChatRow;

use crate::time::format_chat_timestamp;

#[derive(Debug, Clone)]
pub struct ChatRowItem {
    pub chat_id: String,
    pub kind: String,
    pub name: String,
    pub preview: String,
    pub timestamp: String,
    pub last_ts: i64,
    pub unread: i64,
    pub pinned: bool,
}

impl ChatRowItem {
    pub fn from_row(row: &ChatRow) -> Self {
        let preview = row.last_message_preview.clone().unwrap_or_default();
        let preview = if row.last_message_from_me && !preview.is_empty() {
            format!("Você: {preview}")
        } else {
            preview
        };
        let last_ts = row.last_message_ts.unwrap_or(0);
        Self {
            chat_id: row.chat_id.clone(),
            kind: row.kind.clone(),
            name: if row.name.is_empty() {
                row.chat_id.clone()
            } else {
                row.name.clone()
            },
            preview,
            timestamp: format_chat_timestamp(last_ts),
            last_ts,
            unread: row.unread_count,
            pinned: row.pinned,
        }
    }
}

#[derive(Debug)]
pub enum ChatRowOutput {}

pub struct ChatRowFactory {
    pub item: ChatRowItem,
}

#[relm4::factory(pub)]
impl FactoryComponent for ChatRowFactory {
    type Init = ChatRowItem;
    type Input = ();
    type Output = ChatRowOutput;
    type CommandOutput = ();
    type ParentWidget = gtk::ListBox;

    view! {
        gtk::ListBoxRow {
            set_activatable: true,
            // The chat_id rides along on the row widget itself; the parent's
            // `connect_row_activated` reads it back to dispatch. Keeps the
            // factory cleanly decoupled from the listbox.
            set_widget_name: &self.item.chat_id,

            gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,
                set_spacing: 12,
                set_margin_top: 8,
                set_margin_bottom: 8,
                set_margin_start: 12,
                set_margin_end: 12,

                adw::Avatar {
                    set_size: 40,
                    set_text: Some(&self.item.name),
                    set_show_initials: true,
                },

                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_spacing: 2,
                    set_hexpand: true,
                    set_valign: gtk::Align::Center,

                    gtk::Box {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_spacing: 8,

                        gtk::Label {
                            set_label: &self.item.name,
                            set_xalign: 0.0,
                            set_hexpand: true,
                            set_ellipsize: gtk::pango::EllipsizeMode::End,
                            add_css_class: "heading",
                        },
                        gtk::Label {
                            set_label: &self.item.timestamp,
                            add_css_class: "dim-label",
                            add_css_class: "caption",
                        },
                    },

                    gtk::Box {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_spacing: 8,

                        gtk::Label {
                            set_label: &self.item.preview,
                            set_xalign: 0.0,
                            set_hexpand: true,
                            // Hard cap: ellipsize, single line, never wrap.
                            // Without these, long URL previews (or media
                            // placeholders) blow the row height up.
                            set_ellipsize: gtk::pango::EllipsizeMode::End,
                            set_wrap: false,
                            set_lines: 1,
                            set_single_line_mode: true,
                            set_max_width_chars: 30,
                            add_css_class: "dim-label",
                            add_css_class: "caption",
                        },

                        #[name(badge)]
                        gtk::Label {
                            set_visible: self.item.unread > 0,
                            set_label: &format!("{}", self.item.unread),
                            add_css_class: "accent",
                            add_css_class: "caption-heading",
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

// Helper: glib-required (some macros assume glib is in scope as gtk::glib).
#[allow(dead_code)]
fn _link_glib() -> glib::Type {
    glib::Type::INVALID
}
