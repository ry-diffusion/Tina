// Right side of the in-app page: the multi-tab chat surface.
//
// Owns the `AdwTabView` + `AdwTabBar` that drive the multi-chat business
// case (clicks open new tabs, drag-out detaches into a fresh window) and
// the `open_tabs` map of `ChatTab` controllers. The headerbar swaps
// between a centred "single tab" title (avatar + name) and the tab bar
// when more than one tab is open.
//
// One quirk worth flagging: with multiple tabs open, EVERY open tab gets
// `MessagesAppended` push deltas from the worker — but only chats present
// in the worker's open-set are emitted in the first place. Closed tabs
// stay at the snapshot they were loaded with until the user opens them.

use std::collections::HashMap;

use adw::prelude::*;
use gtk::glib;
use relm4::Controller;
use relm4::prelude::*;
use tina_db::MessageRow;

use crate::components::chat_tab::{ChatTab, ChatTabInit, ChatTabInput, ChatTabOutput};

#[derive(Debug)]
pub enum ChatAreaInput {
    /// User picked a chat from the sidebar — reuse the selected tab if
    /// one's open, else open a fresh one. Drives the browser-style
    /// "click bookmark, opens here" behaviour.
    OpenInCurrent(String),
    /// User explicitly asked for a new tab (right-click menu).
    OpenInNewTab(String),
    ChatOpened {
        chat_id: String,
        name: String,
        kind: String,
        messages: Vec<MessageRow>,
    },
    MessagesAppended {
        chat_id: String,
        messages: Vec<MessageRow>,
    },
    OlderMessagesLoaded {
        chat_id: String,
        messages: Vec<MessageRow>,
        reached_top: bool,
    },
    MediaReady {
        message_ids: Vec<String>,
        path: String,
        mimetype: Option<String>,
    },
    MediaFailed {
        message_id: String,
    },
    /// Avatar arrived for some JID — update the headerbar if it matches
    /// the currently-focused chat.
    AvatarReady {
        jid: String,
        path: String,
    },
    /// Internal: AdwTabView signalled tab selection changed.
    TabSelected(Option<String>),
    /// Internal: AdwTabView signalled close-page; finalize teardown.
    TabClosed(String),
    /// Forwarded from a ChatTab.
    SendFromTab { chat_id: String, text: String },
    /// Forwarded from a ChatTab.
    RequestMediaDownload(String),
    /// Forwarded from a ChatTab.
    RequestLoadOlder { chat_id: String, before_ts: i64 },
}

#[derive(Debug)]
pub enum ChatAreaOutput {
    ToggleSidebar(bool),
    /// Ask the worker to fetch metadata + first page for `chat_id`. Comes
    /// back as `ChatOpened` via the parent.
    OpenChatNew(String),
    SendText { chat_id: String, text: String },
    /// A chat was closed in the UI — parent must tell the worker so it
    /// stops emitting `MessagesAppended` for it.
    CloseChat(String),
    RequestMediaDownload(String),
    RequestLoadOlder { chat_id: String, before_ts: i64 },
    RequestFetchAvatar(String),
}

pub struct ChatArea {
    /// chat_id -> (controller, AdwTabPage). Lookup table for "is this chat
    /// already open?" + reverse lookup from page selection back to chat_id.
    open_tabs: HashMap<String, (Controller<ChatTab>, adw::TabPage)>,
    /// chat_id -> (display_name, kind). Used to render the headerbar title
    /// based on the currently-selected tab without round-tripping the
    /// child component.
    chat_meta: HashMap<String, (String, String)>,
    tab_view: adw::TabView,
    tab_bar: adw::TabBar,
    /// Title shown in the content headerbar (matches the selected tab).
    current_chat_name: String,
    #[allow(dead_code)]
    current_chat_kind: String,
    tab_count: usize,
    /// JID of the currently-selected chat, used to filter incoming
    /// AvatarReady events for the headerbar.
    current_chat_id: Option<String>,
    /// Local cache path of the headerbar avatar (when downloaded).
    current_chat_avatar: Option<String>,
}

#[relm4::component(pub)]
impl SimpleComponent for ChatArea {
    type Init = ();
    type Input = ChatAreaInput;
    type Output = ChatAreaOutput;

    view! {
        #[root]
        adw::ToolbarView {
            add_top_bar = &adw::HeaderBar {
                pack_start = &gtk::ToggleButton {
                    set_icon_name: "sidebar-show-symbolic",
                    set_active: true,
                    set_tooltip_text: Some("Toggle sidebar"),
                    connect_toggled[sender] => move |btn| {
                        let _ = sender.output(ChatAreaOutput::ToggleSidebar(btn.is_active()));
                    },
                },

                // Title widget: a Stack switches between a single-
                // chat layout (avatar + name centred) and the multi-
                // chat tab bar. Stack-switching is more reliable
                // than per-child set_visible bindings because
                // existing-widget references don't always re-evaluate
                // their #[watch] bindings inside relm4's view! macro.
                #[wrap(Some)]
                set_title_widget = &gtk::Stack {
                    #[watch]
                    set_visible_child_name: if model.tab_count >= 2 {
                        "multi"
                    } else {
                        "single"
                    },

                    add_named[Some("single")] = &gtk::Box {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_spacing: 8,
                        set_halign: gtk::Align::Center,
                        set_valign: gtk::Align::Center,

                        adw::Avatar {
                            set_size: 30,
                            set_show_initials: true,
                            #[watch]
                            set_text: Some(&model.current_chat_name),
                            #[watch]
                            set_custom_image: model.current_chat_avatar
                                .as_deref()
                                .and_then(|p| gtk::gdk::Texture::from_filename(p).ok())
                                .map(|t| t.upcast::<gtk::gdk::Paintable>())
                                .as_ref(),
                        },

                        adw::WindowTitle {
                            #[watch]
                            set_title: &model.current_chat_name,
                        },
                    },

                    add_named[Some("multi")] = &model.tab_bar.clone(),
                },
            },

            #[wrap(Some)]
            set_content = &model.tab_view.clone() -> adw::TabView {
                connect_close_page[sender] => move |_view, page| {
                    if let Some(chat_id) = page.keyword().map(|s| s.to_string()) {
                        sender.input(ChatAreaInput::TabClosed(chat_id));
                    }
                    glib::Propagation::Stop
                },
                connect_selected_page_notify[sender] => move |view| {
                    let id = view.selected_page()
                        .and_then(|p| p.keyword())
                        .map(|s| s.to_string())
                        .filter(|s| !s.is_empty());
                    sender.input(ChatAreaInput::TabSelected(id));
                },
            },
        }
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let tab_view = adw::TabView::new();
        let tab_bar = adw::TabBar::new();
        tab_bar.set_view(Some(&tab_view));
        tab_bar.set_autohide(false);
        tab_bar.set_expand_tabs(false);

        let model = ChatArea {
            open_tabs: HashMap::new(),
            chat_meta: HashMap::new(),
            tab_view,
            tab_bar,
            current_chat_name: String::new(),
            current_chat_kind: String::new(),
            tab_count: 0,
            current_chat_id: None,
            current_chat_avatar: None,
        };

        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: ChatAreaInput, sender: ComponentSender<Self>) {
        match msg {
            ChatAreaInput::OpenInCurrent(chat_id) => {
                if let Some((_, page)) = self.open_tabs.get(&chat_id) {
                    self.tab_view.set_selected_page(page);
                } else if self.open_tabs.is_empty() {
                    let _ = sender.output(ChatAreaOutput::OpenChatNew(chat_id));
                } else {
                    // Reuse the currently-selected tab: close it, then open
                    // the new chat. close_page() emits `close-page`; our
                    // signal handler returns Stop and dispatches TabClosed,
                    // which calls close_page_finish. Trying to call
                    // close_page_finish directly trips an assertion because
                    // page->closing isn't set yet.
                    if let Some(current) = self.tab_view.selected_page() {
                        self.tab_view.close_page(&current);
                    }
                    let _ = sender.output(ChatAreaOutput::OpenChatNew(chat_id));
                }
            }
            ChatAreaInput::OpenInNewTab(chat_id) => {
                if let Some((_, page)) = self.open_tabs.get(&chat_id) {
                    self.tab_view.set_selected_page(page);
                } else {
                    let _ = sender.output(ChatAreaOutput::OpenChatNew(chat_id));
                }
            }
            ChatAreaInput::ChatOpened {
                chat_id,
                name,
                kind,
                messages,
            } => {
                self.chat_meta
                    .insert(chat_id.clone(), (name.clone(), kind.clone()));
                if let Some((controller, page)) = self.open_tabs.get(&chat_id) {
                    let _ = controller.sender().send(ChatTabInput::SetMeta {
                        name: name.clone(),
                        kind: kind.clone(),
                    });
                    let _ = controller.sender().send(ChatTabInput::Reset(messages));
                    page.set_title(&name);
                } else {
                    let controller = ChatTab::builder()
                        .launch(ChatTabInit {
                            chat_id: chat_id.clone(),
                            name: name.clone(),
                            kind: kind.clone(),
                            initial: messages,
                        })
                        .forward(sender.input_sender(), |o| match o {
                            ChatTabOutput::Send { chat_id, text } => {
                                ChatAreaInput::SendFromTab { chat_id, text }
                            }
                            ChatTabOutput::Close { chat_id } => ChatAreaInput::TabClosed(chat_id),
                            ChatTabOutput::RequestMediaDownload(id) => {
                                ChatAreaInput::RequestMediaDownload(id)
                            }
                            ChatTabOutput::RequestLoadOlder { chat_id, before_ts } => {
                                ChatAreaInput::RequestLoadOlder { chat_id, before_ts }
                            }
                        });
                    let widget = controller.widget().clone();
                    let page = self.tab_view.append(&widget);
                    page.set_title(&name);
                    page.set_keyword(&chat_id);
                    self.tab_view.set_selected_page(&page);
                    self.open_tabs.insert(chat_id.clone(), (controller, page));
                }
                self.tab_count = self.open_tabs.len();
                self.current_chat_name = name;
                self.current_chat_kind = kind;
                self.current_chat_id = Some(chat_id.clone());
                // Worker fetcher is cached on disk — request unconditionally
                // and let it be a no-op when the file's already there.
                self.current_chat_avatar = None;
                let _ = sender.output(ChatAreaOutput::RequestFetchAvatar(chat_id));
            }
            ChatAreaInput::MessagesAppended { chat_id, messages } => {
                if let Some((controller, _)) = self.open_tabs.get(&chat_id) {
                    let _ = controller.sender().send(ChatTabInput::Append(messages));
                } else {
                    tracing::warn!(
                        chat = %chat_id,
                        "MessagesAppended received for chat with no open tab",
                    );
                }
            }
            ChatAreaInput::OlderMessagesLoaded {
                chat_id,
                messages,
                reached_top,
            } => {
                if let Some((controller, _)) = self.open_tabs.get(&chat_id) {
                    let _ = controller.sender().send(ChatTabInput::PrependOlder {
                        messages,
                        reached_top,
                    });
                }
            }
            ChatAreaInput::MediaReady {
                message_ids,
                path,
                mimetype,
            } => {
                for (_, (controller, _)) in self.open_tabs.iter() {
                    let _ = controller.sender().send(ChatTabInput::MediaReady {
                        message_ids: message_ids.clone(),
                        path: path.clone(),
                        mimetype: mimetype.clone(),
                    });
                }
            }
            ChatAreaInput::MediaFailed { message_id } => {
                for (_, (controller, _)) in self.open_tabs.iter() {
                    let _ = controller
                        .sender()
                        .send(ChatTabInput::MediaFailed(message_id.clone()));
                }
            }
            ChatAreaInput::AvatarReady { jid, path } => {
                if self.current_chat_id.as_deref() == Some(jid.as_str()) {
                    self.current_chat_avatar = Some(path);
                }
            }
            ChatAreaInput::TabSelected(chat_id) => {
                if let Some(id) = &chat_id {
                    if let Some((name, kind)) = self.chat_meta.get(id) {
                        self.current_chat_name = name.clone();
                        self.current_chat_kind = kind.clone();
                    }
                    if let Some((controller, _)) = self.open_tabs.get(id) {
                        let _ = controller.sender().send(ChatTabInput::StickToBottom);
                    }
                    self.current_chat_id = chat_id;
                } else if self.open_tabs.is_empty() {
                    self.current_chat_name.clear();
                    self.current_chat_kind.clear();
                    self.current_chat_id = None;
                }
                // Else: spurious selected-page-notify (fires immediately after
                // tab_view.append, before keyword is set). Keep current state.
            }
            ChatAreaInput::TabClosed(chat_id) => {
                if let Some((controller, page)) = self.open_tabs.remove(&chat_id) {
                    self.tab_view.close_page_finish(&page, true);
                    drop(controller);
                }
                self.chat_meta.remove(&chat_id);
                self.tab_count = self.open_tabs.len();
                if self.open_tabs.is_empty() {
                    self.current_chat_name.clear();
                    self.current_chat_kind.clear();
                    self.current_chat_id = None;
                }
                let _ = sender.output(ChatAreaOutput::CloseChat(chat_id));
            }
            ChatAreaInput::SendFromTab { chat_id, text } => {
                let _ = sender.output(ChatAreaOutput::SendText { chat_id, text });
            }
            ChatAreaInput::RequestMediaDownload(id) => {
                let _ = sender.output(ChatAreaOutput::RequestMediaDownload(id));
            }
            ChatAreaInput::RequestLoadOlder { chat_id, before_ts } => {
                let _ = sender.output(ChatAreaOutput::RequestLoadOlder { chat_id, before_ts });
            }
        }
    }
}

