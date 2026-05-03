// One open chat — header strip with the contact's name, scrollable thread
// of message bubbles, and a single-line composer. The "active chat" gating
// (which thread receives push updates) is the parent's job; a tab just
// renders whatever it's been handed.

use adw::prelude::*;
use relm4::factory::FactoryVecDeque;
use relm4::prelude::*;
use tina_db::MessageRow;

use crate::components::message_bubble::{MessageBubble, MessageItem};

#[derive(Debug)]
pub enum ChatTabInput {
    SetMeta {
        name: String,
        kind: String,
    },
    Reset(Vec<MessageRow>),
    Append(Vec<MessageRow>),
    Send,
    /// Forwarded from `MainPage` when a media download finishes (or comes
    /// pre-resolved by dedup). The tab walks its factory and patches any
    /// matching bubble in-place.
    MediaReady {
        message_ids: Vec<String>,
        path: String,
        mimetype: Option<String>,
    },
    MediaFailed(String),
    /// A bubble emitted "tap to download". Forward up to MainPage so the
    /// service worker actually issues the IPC command.
    RequestMediaDownload(String),
}

#[derive(Debug)]
pub enum ChatTabOutput {
    Send { chat_id: String, text: String },
    Close { chat_id: String },
    RequestMediaDownload(String),
}

pub struct ChatTabInit {
    pub chat_id: String,
    pub name: String,
    pub kind: String,
    pub initial: Vec<MessageRow>,
}

pub struct ChatTab {
    chat_id: String,
    name: String,
    kind: String,
    messages: FactoryVecDeque<MessageBubble>,
    composer_buffer: gtk::EntryBuffer,
    last_sender: Option<String>,
}

#[relm4::component(pub)]
impl SimpleComponent for ChatTab {
    type Init = ChatTabInit;
    type Input = ChatTabInput;
    type Output = ChatTabOutput;

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,

            // The chat header (avatar + name + kind) lives in the parent
            // window's AdwHeaderBar — set by `MainPage` based on the
            // currently-selected tab — so each tab's content area starts
            // straight at the message thread.

            #[name(scroll)]
            gtk::ScrolledWindow {
                set_vexpand: true,
                set_hscrollbar_policy: gtk::PolicyType::Never,

                #[local_ref]
                messages_list -> gtk::ListBox {
                    set_selection_mode: gtk::SelectionMode::None,
                    add_css_class: "background",
                },
            },

            gtk::Separator {},

            gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,
                set_margin_top: 6,
                set_margin_bottom: 6,
                set_margin_start: 12,
                set_margin_end: 12,
                set_spacing: 6,

                gtk::Entry {
                    set_buffer: &model.composer_buffer,
                    set_hexpand: true,
                    set_placeholder_text: Some("Message…"),
                    connect_activate => ChatTabInput::Send,
                },

                gtk::Button {
                    // `send-symbolic` is not part of the Adwaita icon theme;
                    // `mail-send-symbolic` is and matches the visual cue.
                    set_icon_name: "mail-send-symbolic",
                    set_tooltip_text: Some("Send"),
                    add_css_class: "suggested-action",
                    connect_clicked => ChatTabInput::Send,
                },
            },
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let mut messages = FactoryVecDeque::builder()
            .launch(gtk::ListBox::default())
            .forward(sender.input_sender(), |o| match o {
                crate::components::message_bubble::MessageBubbleOut::DownloadRequested(id) => {
                    ChatTabInput::RequestMediaDownload(id)
                }
            });

        // Seed with initial history.
        {
            let mut guard = messages.guard();
            let mut last_sender: Option<String> = None;
            let kind = init.kind.clone();
            for row in &init.initial {
                let show = !row.is_from_me
                    && kind != "dm"
                    && last_sender.as_deref()
                        != Some(row.sender_name.as_deref().unwrap_or(""));
                last_sender = row.sender_name.clone();
                guard.push_back(MessageItem::from_row(row, show));
            }
        }

        let model = ChatTab {
            chat_id: init.chat_id,
            name: init.name,
            kind: init.kind,
            messages,
            composer_buffer: gtk::EntryBuffer::default(),
            last_sender: None,
        };

        let messages_list = model.messages.widget();
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: ChatTabInput, sender: ComponentSender<Self>) {
        match msg {
            ChatTabInput::SetMeta { name, kind } => {
                self.name = name;
                self.kind = kind;
            }
            ChatTabInput::Reset(rows) => {
                let mut guard = self.messages.guard();
                guard.clear();
                self.last_sender = None;
                for row in &rows {
                    let show = !row.is_from_me
                        && self.kind != "dm"
                        && self.last_sender.as_deref()
                            != Some(row.sender_name.as_deref().unwrap_or(""));
                    self.last_sender = row.sender_name.clone();
                    guard.push_back(MessageItem::from_row(row, show));
                }
            }
            ChatTabInput::Append(rows) => {
                let mut guard = self.messages.guard();
                for row in &rows {
                    let show = !row.is_from_me
                        && self.kind != "dm"
                        && self.last_sender.as_deref()
                            != Some(row.sender_name.as_deref().unwrap_or(""));
                    self.last_sender = row.sender_name.clone();
                    guard.push_back(MessageItem::from_row(row, show));
                }
            }
            ChatTabInput::Send => {
                let text = self.composer_buffer.text().to_string();
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    return;
                }
                let _ = sender.output(ChatTabOutput::Send {
                    chat_id: self.chat_id.clone(),
                    text: trimmed.to_string(),
                });
                self.composer_buffer.set_text("");
            }
            ChatTabInput::MediaReady {
                message_ids,
                path,
                mimetype,
            } => {
                let id_set: std::collections::HashSet<&String> = message_ids.iter().collect();
                let mut guard = self.messages.guard();
                // FactoryVecDeque doesn't expose mut iter; rebuild by index.
                let mut to_replace: Vec<(usize, MessageItem)> = Vec::new();
                for (idx, fac) in guard.iter().enumerate() {
                    if id_set.contains(&fac.item.id) {
                        let mut new_item = fac.item.clone();
                        new_item.media_path = Some(path.clone());
                        new_item.media_status = "done".into();
                        if new_item.media_mimetype.is_none() {
                            new_item.media_mimetype = mimetype.clone();
                        }
                        to_replace.push((idx, new_item));
                    }
                }
                for (idx, item) in to_replace {
                    guard.remove(idx);
                    guard.insert(idx, item);
                }
            }
            ChatTabInput::MediaFailed(message_id) => {
                let mut guard = self.messages.guard();
                let mut to_replace: Vec<(usize, MessageItem)> = Vec::new();
                for (idx, fac) in guard.iter().enumerate() {
                    if fac.item.id == message_id {
                        let mut new_item = fac.item.clone();
                        new_item.media_status = "failed".into();
                        to_replace.push((idx, new_item));
                    }
                }
                for (idx, item) in to_replace {
                    guard.remove(idx);
                    guard.insert(idx, item);
                }
            }
            ChatTabInput::RequestMediaDownload(id) => {
                // Optimistically mark downloading immediately for snappy UI;
                // worker confirms via MediaReady (or rolls back via MediaFailed).
                let mut guard = self.messages.guard();
                let mut to_replace: Vec<(usize, MessageItem)> = Vec::new();
                for (idx, fac) in guard.iter().enumerate() {
                    if fac.item.id == id {
                        let mut new_item = fac.item.clone();
                        new_item.media_status = "downloading".into();
                        to_replace.push((idx, new_item));
                    }
                }
                for (idx, item) in to_replace {
                    guard.remove(idx);
                    guard.insert(idx, item);
                }
                let _ = sender.output(ChatTabOutput::RequestMediaDownload(id));
            }
        }
    }
}

impl ChatTab {
    pub fn chat_id(&self) -> &str {
        &self.chat_id
    }
}
