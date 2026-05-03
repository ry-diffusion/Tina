// One open chat — header strip with the contact's name, scrollable thread
// of message bubbles, and a single-line composer. The "active chat" gating
// (which thread receives push updates) is the parent's job; a tab just
// renders whatever it's been handed.

use adw::prelude::*;
use gtk::glib;
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
    MediaReady {
        message_ids: Vec<String>,
        path: String,
        mimetype: Option<String>,
    },
    MediaFailed(String),
    RequestMediaDownload(String),
    /// VAdjustment crossed the load-more threshold. Internal trigger.
    NearTop,
    /// Older page came back from the worker. `reached_top = true` means
    /// the worker returned fewer rows than requested → we've loaded the
    /// entire history; stop trying.
    PrependOlder {
        messages: Vec<MessageRow>,
        reached_top: bool,
    },
}

#[derive(Debug)]
pub enum ChatTabOutput {
    Send { chat_id: String, text: String },
    Close { chat_id: String },
    RequestMediaDownload(String),
    RequestLoadOlder { chat_id: String, before_ts: i64 },
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
    scroll: Option<gtk::ScrolledWindow>,
    seen_message_ids: std::collections::HashSet<String>,
    last_send: Option<(String, std::time::Instant)>,
    /// Timestamp of the OLDEST message currently in the factory; used as
    /// the cursor for LoadOlder requests.
    oldest_ts: Option<i64>,
    /// True while a LoadOlder request is in flight. Prevents the scroll
    /// listener from firing the same request 60×/s.
    loading_older: bool,
    /// Latched once the worker returns fewer rows than requested.
    reached_top: bool,
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

        let mut seen: std::collections::HashSet<String> =
            std::collections::HashSet::with_capacity(init.initial.len());
        for r in &init.initial {
            seen.insert(r.message_id.clone());
        }

        let oldest_ts = init.initial.iter().map(|r| r.timestamp).min();

        let mut model = ChatTab {
            chat_id: init.chat_id,
            name: init.name,
            kind: init.kind,
            messages,
            composer_buffer: gtk::EntryBuffer::default(),
            last_sender: None,
            scroll: None,
            seen_message_ids: seen,
            last_send: None,
            oldest_ts,
            loading_older: false,
            reached_top: false,
        };

        let messages_list = model.messages.widget();
        let widgets = view_output!();
        model.scroll = Some(widgets.scroll.clone());

        // Scroll listener for lazy-load: when the user scrolls within
        // 2× page-size of the top, ask the parent for an older page.
        // The Input::NearTop handler debounces via `loading_older`.
        {
            let scroll = widgets.scroll.clone();
            let input = sender.input_sender().clone();
            scroll.vadjustment().connect_value_changed(move |adj| {
                let value = adj.value();
                let page = adj.page_size();
                if value < page * 2.0 && adj.upper() > page * 2.0 {
                    let _ = input.send(ChatTabInput::NearTop);
                }
            });
        }

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: ChatTabInput, sender: ComponentSender<Self>) {
        match msg {
            ChatTabInput::SetMeta { name, kind } => {
                self.name = name;
                self.kind = kind;
            }
            ChatTabInput::Reset(rows) => {
                self.oldest_ts = rows.iter().map(|r| r.timestamp).min();
                self.reached_top = rows.len() < 50;
                self.loading_older = false;
                {
                    let mut guard = self.messages.guard();
                    guard.clear();
                    self.last_sender = None;
                    self.seen_message_ids.clear();
                    for row in &rows {
                        let show = !row.is_from_me
                            && self.kind != "dm"
                            && self.last_sender.as_deref()
                                != Some(row.sender_name.as_deref().unwrap_or(""));
                        self.last_sender = row.sender_name.clone();
                        self.seen_message_ids.insert(row.message_id.clone());
                        guard.push_back(MessageItem::from_row(row, show));
                    }
                }
                // Sticky bottom on chat open. The first idle tick is too
                // early — the listbox hasn't allocated its rows yet, so
                // upper() is still 0. Schedule a few in succession; the
                // last one wins after layout has settled.
                if let Some(scroll) = self.scroll.clone() {
                    let s1 = scroll.clone();
                    glib::idle_add_local_once(move || {
                        let adj = s1.vadjustment();
                        adj.set_value(adj.upper() - adj.page_size());
                    });
                    let s2 = scroll.clone();
                    glib::timeout_add_local_once(
                        std::time::Duration::from_millis(50),
                        move || {
                            let adj = s2.vadjustment();
                            adj.set_value(adj.upper() - adj.page_size());
                        },
                    );
                    glib::timeout_add_local_once(
                        std::time::Duration::from_millis(200),
                        move || {
                            let adj = scroll.vadjustment();
                            adj.set_value(adj.upper() - adj.page_size());
                        },
                    );
                }
            }
            ChatTabInput::Append(rows) => {
                tracing::info!(
                    chat = %self.chat_id,
                    count = rows.len(),
                    "ChatTab::Append"
                );
                // Sticky-bottom behaviour: only auto-scroll if the user
                // was already near the bottom before this delta. If they
                // scrolled up to read history, don't yank them back down.
                let was_at_bottom = self
                    .scroll
                    .as_ref()
                    .map(|s| {
                        let adj = s.vadjustment();
                        let bottom = adj.upper() - adj.page_size();
                        adj.value() >= bottom - 50.0
                    })
                    .unwrap_or(true);

                let new_rows: Vec<_> = rows
                    .into_iter()
                    .filter(|r| self.seen_message_ids.insert(r.message_id.clone()))
                    .collect();
                if new_rows.is_empty() {
                    return;
                }
                {
                    let mut guard = self.messages.guard();
                    for row in &new_rows {
                        let show = !row.is_from_me
                            && self.kind != "dm"
                            && self.last_sender.as_deref()
                                != Some(row.sender_name.as_deref().unwrap_or(""));
                        self.last_sender = row.sender_name.clone();
                        guard.push_back(MessageItem::from_row(row, show));
                    }
                }
                if was_at_bottom {
                    if let Some(scroll) = self.scroll.clone() {
                        glib::idle_add_local_once(move || {
                            let adj = scroll.vadjustment();
                            adj.set_value(adj.upper() - adj.page_size());
                        });
                    }
                }
            }
            ChatTabInput::Send => {
                let text = self.composer_buffer.text().to_string();
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    return;
                }
                // Drop duplicate fires (Enter + button-click in same frame,
                // GTK signal weirdness). Same body within 1s = ignored.
                if let Some((prev, when)) = &self.last_send {
                    if prev == trimmed && when.elapsed() < std::time::Duration::from_secs(1) {
                        tracing::warn!(
                            chat = %self.chat_id,
                            "Send debounced (duplicate within 1s)"
                        );
                        self.composer_buffer.set_text("");
                        return;
                    }
                }
                self.last_send = Some((trimmed.to_string(), std::time::Instant::now()));
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
                let saved = self.scroll.as_ref().map(|s| s.vadjustment().value());
                {
                    let id_set: std::collections::HashSet<&String> = message_ids.iter().collect();
                    let mut guard = self.messages.guard();
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
                if let (Some(scroll), Some(v)) = (self.scroll.as_ref(), saved) {
                    let adj = scroll.vadjustment();
                    glib::idle_add_local_once(move || adj.set_value(v));
                }
            }
            ChatTabInput::MediaFailed(message_id) => {
                let saved = self.scroll.as_ref().map(|s| s.vadjustment().value());
                {
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
                if let (Some(scroll), Some(v)) = (self.scroll.as_ref(), saved) {
                    let adj = scroll.vadjustment();
                    glib::idle_add_local_once(move || adj.set_value(v));
                }
            }
            ChatTabInput::NearTop => {
                if self.loading_older || self.reached_top {
                    return;
                }
                let Some(before_ts) = self.oldest_ts else {
                    return;
                };
                self.loading_older = true;
                tracing::info!(
                    chat = %self.chat_id,
                    before_ts,
                    "ChatTab: requesting older page",
                );
                let _ = sender.output(ChatTabOutput::RequestLoadOlder {
                    chat_id: self.chat_id.clone(),
                    before_ts,
                });
            }
            ChatTabInput::PrependOlder {
                messages,
                reached_top,
            } => {
                self.loading_older = false;
                if reached_top || messages.len() < 50 {
                    self.reached_top = true;
                }
                if messages.is_empty() {
                    return;
                }
                // Preserve scroll position: capture (upper, value) before,
                // then after layout settles, set value = old_value +
                // (new_upper - old_upper). The user stays on the same
                // content while history grows above.
                let saved = self
                    .scroll
                    .as_ref()
                    .map(|s| (s.vadjustment().upper(), s.vadjustment().value()));

                let new_oldest = messages.iter().map(|r| r.timestamp).min();
                if let Some(t) = new_oldest {
                    self.oldest_ts = Some(match self.oldest_ts {
                        Some(prev) => prev.min(t),
                        None => t,
                    });
                }

                {
                    let mut guard = self.messages.guard();
                    // Insert oldest first (reversed iteration so push_front
                    // builds correct chronological order).
                    for row in messages.iter().rev() {
                        if !self.seen_message_ids.insert(row.message_id.clone()) {
                            continue;
                        }
                        guard.push_front(MessageItem::from_row(row, false));
                    }
                }

                if let (Some(scroll), Some((old_upper, old_value))) = (self.scroll.clone(), saved) {
                    glib::idle_add_local_once(move || {
                        let adj = scroll.vadjustment();
                        let new_upper = adj.upper();
                        let delta = new_upper - old_upper;
                        adj.set_value(old_value + delta);
                    });
                }
            }
            ChatTabInput::RequestMediaDownload(id) => {
                // remove+insert rebuilds the row, which the listbox treats
                // as "content changed → re-allocate", and any height
                // difference jumps the scroll. Capture the vadjustment
                // before the mutation and restore it after the next idle
                // tick so the user stays put.
                let saved = self
                    .scroll
                    .as_ref()
                    .map(|s| s.vadjustment().value());
                {
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
                }
                if let (Some(scroll), Some(v)) = (self.scroll.as_ref(), saved) {
                    let adj = scroll.vadjustment();
                    glib::idle_add_local_once(move || adj.set_value(v));
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
