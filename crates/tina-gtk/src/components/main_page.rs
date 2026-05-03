// In-app page: sidebar + multi-chat tab area, with the GNOME HIG-canonical
// `AdwOverlaySplitView` driving responsive collapse on narrow widths and
// `AdwTabView` driving the multi-chat business case (clicks open new tabs,
// drag-out detaches into a fresh window).
//
// One quirk worth flagging: with multiple tabs open, only the *focused* tab
// gets `MessagesAppended` push deltas from the worker — the others stay at
// the snapshot they were loaded with. Switching tabs re-points the worker.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use gtk::gdk;
use gtk::glib;
use relm4::factory::FactoryVecDeque;
use relm4::prelude::*;
use relm4::Controller;
use tina_db::{ChatRow, MessageRow};

use crate::components::chat_row::{ChatRowFactory, ChatRowItem};
use crate::components::chat_tab::{ChatTab, ChatTabInit, ChatTabInput, ChatTabOutput};
use crate::service::ServiceHandle;

pub struct MainInit {
    pub service: ServiceHandle,
}

#[derive(Debug)]
pub enum MainInput {
    SetIdentity {
        account_id: String,
        phone: Option<String>,
    },
    ChatsUpserted(Vec<ChatRow>),
    ChatOpened {
        chat_id: Option<String>,
        name: String,
        kind: String,
        messages: Vec<MessageRow>,
    },
    MessagesAppended {
        chat_id: String,
        messages: Vec<MessageRow>,
    },
    /// Default activation (left-click on a sidebar row). Replaces the
    /// currently-selected tab so single-tab usage feels like a normal
    /// messenger; explicit "Open in new tab" preserves multi-tab.
    OpenInCurrent(String),
    /// Force a new tab even if one is already open elsewhere.
    OpenInNewTab(String),
    TabSelected(Option<String>),
    TabClosed(String),
    SendFromTab {
        chat_id: String,
        text: String,
    },
    SetRepairing(bool),
    RepairProgress {
        stage: String,
        current: i64,
        total: i64,
        indeterminate: bool,
    },
    Repair,
    Logout,
    SearchChanged(String),
    MediaReady {
        message_ids: Vec<String>,
        path: String,
        mimetype: Option<String>,
    },
    MediaFailed {
        message_id: String,
    },
    /// Bubble in some tab clicked "Tap to download".
    RequestMediaDownload(String),
    /// Tab scrolled near the top and is requesting an older page.
    RequestLoadOlder { chat_id: String, before_ts: i64 },
    /// Older messages came back from the worker; route to the tab.
    OlderMessagesLoaded {
        chat_id: String,
        messages: Vec<MessageRow>,
        reached_top: bool,
    },
    /// A profile picture finished downloading; refresh sidebar/headerbar
    /// for any chat that resolves to this JID.
    AvatarReady { jid: String, path: String },
}

#[derive(Debug)]
pub enum MainOutput {
    OpenChatNew(String),
    FocusChat(Option<String>),
    SendText { chat_id: String, text: String },
    RequestRepair,
    RequestLogout,
    RequestMediaDownload(String),
    RequestLoadOlder { chat_id: String, before_ts: i64 },
    RequestFetchAvatar(String),
}

pub struct MainPage {
    #[allow(dead_code)]
    service: ServiceHandle,
    chats: FactoryVecDeque<ChatRowFactory>,
    /// chat_id -> (controller, AdwTabPage). Lookup table for "is this chat
    /// already open?" + reverse lookup from page selection back to chat_id.
    open_tabs: HashMap<String, (Controller<ChatTab>, adw::TabPage)>,
    /// chat_id -> (display_name, kind). Used to render the headerbar title
    /// based on the currently-selected tab without round-tripping the
    /// child component.
    chat_meta: HashMap<String, (String, String)>,
    /// AdwTabView is the central widget — kept here so we can manipulate
    /// pages outside the view! macro (insert/select/close).
    tab_view: adw::TabView,
    /// AdwTabBar lives inside the headerbar's title slot (Builder/Console
    /// style). Owning it as a field is cheaper than recreating it inside
    /// the view! macro.
    tab_bar: adw::TabBar,
    repairing: bool,
    repair_stage: String,
    repair_current: i64,
    repair_total: i64,
    repair_indeterminate: bool,
    phone: Option<String>,
    search: String,
    /// Title shown in the content headerbar (matches the selected tab).
    current_chat_name: String,
    current_chat_kind: String,
    /// Number of open chat tabs. Drives whether the headerbar shows
    /// avatar + name centred (single tab) or the tab bar (multi).
    tab_count: usize,
    /// JID of the currently-selected chat, used to filter incoming
    /// AvatarReady events for the headerbar.
    current_chat_id: Option<String>,
    /// Local cache path of the headerbar avatar (when downloaded).
    current_chat_avatar: Option<String>,
    /// JIDs we've already issued FetchAvatar for in this session, to
    /// avoid spamming the worker on every ChatsUpserted batch.
    avatar_requested: std::collections::HashSet<String>,
}

/// Map raw chat kind strings to a human label for the header subtitle.
fn kind_label(kind: &str) -> &'static str {
    match kind {
        "dm" => "Direct Message",
        "group" => "Group",
        "newsletter" => "Channel",
        "broadcast" => "Broadcast",
        "status" => "Status",
        _ => "",
    }
}

#[relm4::component(pub)]
impl SimpleComponent for MainPage {
    type Init = MainInit;
    type Input = MainInput;
    type Output = MainOutput;

    view! {
        #[root]
        adw::OverlaySplitView {
            set_min_sidebar_width: 280.0,
            set_max_sidebar_width: 380.0,
            set_sidebar_width_fraction: 0.27,

            #[wrap(Some)]
            set_sidebar = &adw::ToolbarView {
                add_top_bar = &adw::HeaderBar {
                    // Paper-plane-style: the user's own avatar lives at the
                    // start of the sidebar headerbar; clicking opens a
                    // popover with profile info. Phone number moves into
                    // the popover so the title stays clean.
                    pack_start = &gtk::MenuButton {
                        add_css_class: "flat",
                        add_css_class: "circular",
                        set_tooltip_text: Some("Profile"),

                        #[wrap(Some)]
                        set_child = &adw::Avatar {
                            set_size: 28,
                            #[watch]
                            set_text: Some(model.phone.as_deref().unwrap_or("Tina")),
                            set_show_initials: true,
                        },

                        #[wrap(Some)]
                        set_popover = &gtk::Popover {
                            gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 12,
                                set_margin_top: 12,
                                set_margin_bottom: 12,
                                set_margin_start: 12,
                                set_margin_end: 12,
                                set_width_request: 240,

                                gtk::Box {
                                    set_orientation: gtk::Orientation::Horizontal,
                                    set_spacing: 12,
                                    set_halign: gtk::Align::Center,

                                    adw::Avatar {
                                        set_size: 56,
                                        #[watch]
                                        set_text: Some(model.phone.as_deref().unwrap_or("Tina")),
                                        set_show_initials: true,
                                    },
                                },

                                gtk::Label {
                                    set_label: "Tina",
                                    set_halign: gtk::Align::Center,
                                    add_css_class: "title-2",
                                },

                                gtk::Label {
                                    #[watch]
                                    set_label: model.phone.as_deref().unwrap_or("Not connected"),
                                    set_halign: gtk::Align::Center,
                                    set_selectable: true,
                                    add_css_class: "dim-label",
                                    add_css_class: "caption",
                                },

                                gtk::Separator {},

                                gtk::Button {
                                    set_label: "Repair (reconcile)",
                                    add_css_class: "flat",
                                    #[watch]
                                    set_sensitive: !model.repairing,
                                    connect_clicked => MainInput::Repair,
                                },

                                gtk::Button {
                                    set_label: "Log out",
                                    add_css_class: "flat",
                                    add_css_class: "destructive-action",
                                    connect_clicked => MainInput::Logout,
                                },
                            },
                        },
                    },

                    #[wrap(Some)]
                    set_title_widget = &adw::WindowTitle {
                        set_title: "Tina",
                        // Phone number lives in the profile popover now.
                    },

                    // The ⋮ menu used to live here but its actions all moved
                    // into the profile popover above; redundant.
                },

                #[wrap(Some)]
                set_content = &gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,

                    gtk::SearchEntry {
                        set_margin_top: 6,
                        set_margin_bottom: 6,
                        set_margin_start: 12,
                        set_margin_end: 12,
                        set_placeholder_text: Some("Search"),
                        connect_search_changed[sender] => move |se| {
                            sender.input(MainInput::SearchChanged(se.text().to_string()));
                        },
                    },

                    gtk::ScrolledWindow {
                        set_vexpand: true,
                        set_hscrollbar_policy: gtk::PolicyType::Never,

                        #[local_ref]
                        chat_listbox -> gtk::ListBox {
                            add_css_class: "navigation-sidebar",
                            set_selection_mode: gtk::SelectionMode::Single,
                        },
                    },

                    #[name(repair_bar)]
                    gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                        set_spacing: 4,
                        set_margin_top: 8,
                        set_margin_bottom: 8,
                        set_margin_start: 12,
                        set_margin_end: 12,
                        #[watch]
                        set_visible: model.repairing,

                        gtk::Label {
                            #[watch]
                            set_label: &model.repair_stage,
                            set_xalign: 0.0,
                            add_css_class: "caption",
                        },

                        gtk::ProgressBar {
                            #[watch]
                            set_pulse_step: 0.1,
                            #[watch]
                            set_fraction: if model.repair_total > 0 {
                                (model.repair_current as f64) / (model.repair_total as f64)
                            } else { 0.0 },
                            #[watch]
                            set_show_text: !model.repair_indeterminate && model.repair_total > 0,
                            #[watch]
                            set_text: Some(&format!("{} / {}", model.repair_current, model.repair_total)),
                        },
                    },
                },
            },

            #[wrap(Some)]
            set_content = &adw::ToolbarView {
                add_top_bar = &adw::HeaderBar {
                    pack_start = &gtk::ToggleButton {
                        set_icon_name: "sidebar-show-symbolic",
                        set_active: true,
                        set_tooltip_text: Some("Toggle sidebar"),
                        connect_toggled[root] => move |btn| {
                            // `root` = AdwOverlaySplitView (the macro #[root])
                            root.set_show_sidebar(btn.is_active());
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
                            sender.input(MainInput::TabClosed(chat_id));
                        }
                        glib::Propagation::Stop
                    },
                    connect_selected_page_notify[sender] => move |view| {
                        // `keyword()` is empty for a just-appended page
                        // whose set_keyword hasn't run yet. Skip these
                        // transient notifications; the real one with the
                        // chat_id arrives once we've finished configuring
                        // the page in the ChatOpened handler.
                        let id = view.selected_page()
                            .and_then(|p| p.keyword())
                            .map(|s| s.to_string())
                            .filter(|s| !s.is_empty());
                        sender.input(MainInput::TabSelected(id));
                    },
                },
            },
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let chats = FactoryVecDeque::<ChatRowFactory>::builder()
            .launch(gtk::ListBox::default())
            .detach();

        // Left click → default activation. ListBoxRow alone only fires
        // `activate` on keyboard Enter; mouse clicks land on the parent
        // ListBox as `row-activated`. We pull chat_id from `widget_name`
        // (set by the factory).
        {
            let input_sender = sender.input_sender().clone();
            chats
                .widget()
                .connect_row_activated(move |_listbox, row| {
                    let id = row.widget_name().to_string();
                    if !id.is_empty() {
                        let _ = input_sender.send(MainInput::OpenInCurrent(id));
                    }
                });
        }

        // Right-click → context menu with "Open" / "Open in new tab".
        // The popover is a single shared widget reparented (well, repointed)
        // on each click; `context_target` carries the chat_id between the
        // gesture press and the button click.
        let context_target: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let context_popover = gtk::Popover::new();
        context_popover.set_has_arrow(false);
        context_popover.set_position(gtk::PositionType::Bottom);
        {
            let menu_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
            menu_box.set_margin_top(4);
            menu_box.set_margin_bottom(4);
            menu_box.set_margin_start(4);
            menu_box.set_margin_end(4);

            let open_btn = gtk::Button::with_label("Open");
            open_btn.add_css_class("flat");
            {
                let s = sender.input_sender().clone();
                let target = context_target.clone();
                let pop = context_popover.clone();
                open_btn.connect_clicked(move |_| {
                    if let Some(id) = target.borrow().clone() {
                        let _ = s.send(MainInput::OpenInCurrent(id));
                    }
                    pop.popdown();
                });
            }
            menu_box.append(&open_btn);

            let new_tab_btn = gtk::Button::with_label("Open in new tab");
            new_tab_btn.add_css_class("flat");
            {
                let s = sender.input_sender().clone();
                let target = context_target.clone();
                let pop = context_popover.clone();
                new_tab_btn.connect_clicked(move |_| {
                    if let Some(id) = target.borrow().clone() {
                        let _ = s.send(MainInput::OpenInNewTab(id));
                    }
                    pop.popdown();
                });
            }
            menu_box.append(&new_tab_btn);

            context_popover.set_child(Some(&menu_box));
            context_popover.set_parent(chats.widget());
        }
        {
            let listbox = chats.widget().clone();
            let target = context_target.clone();
            let pop = context_popover.clone();
            let right_click = gtk::GestureClick::new();
            right_click.set_button(gdk::BUTTON_SECONDARY);
            right_click.connect_pressed(move |_g, _n, x, y| {
                if let Some(row) = listbox.row_at_y(y as i32) {
                    let id = row.widget_name().to_string();
                    if id.is_empty() {
                        return;
                    }
                    *target.borrow_mut() = Some(id);
                    let rect = gdk::Rectangle::new(x as i32, y as i32, 1, 1);
                    pop.set_pointing_to(Some(&rect));
                    pop.popup();
                }
            });
            chats.widget().add_controller(right_click);
        }

        let tab_view = adw::TabView::new();
        let tab_bar = adw::TabBar::new();
        tab_bar.set_view(Some(&tab_view));
        // Always-visible bar: even with one tab open, the tab IS the title
        // of the headerbar, so hiding it would also hide the chat name.
        tab_bar.set_autohide(false);
        tab_bar.set_expand_tabs(false);

        let model = MainPage {
            service: init.service,
            chats,
            open_tabs: HashMap::new(),
            chat_meta: HashMap::new(),
            tab_view,
            tab_bar,
            repairing: false,
            repair_stage: String::new(),
            repair_current: 0,
            repair_total: 0,
            repair_indeterminate: true,
            phone: None,
            search: String::new(),
            current_chat_name: String::new(),
            current_chat_kind: String::new(),
            tab_count: 0,
            current_chat_id: None,
            current_chat_avatar: None,
            avatar_requested: std::collections::HashSet::new(),
        };

        let chat_listbox = model.chats.widget();
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: MainInput, sender: ComponentSender<Self>) {
        match msg {
            MainInput::SetIdentity { phone, .. } => {
                self.phone = phone;
            }
            MainInput::ChatsUpserted(rows) => {
                // Trigger avatar fetches for any new chat ids we haven't
                // asked the worker about yet, before consuming `rows`.
                for r in &rows {
                    if r.avatar_path.is_some() {
                        continue;
                    }
                    if self.avatar_requested.insert(r.chat_id.clone()) {
                        let _ = sender
                            .output(MainOutput::RequestFetchAvatar(r.chat_id.clone()));
                    }
                }
                self.apply_chats_upserted(rows);
            }
            MainInput::SearchChanged(text) => {
                self.search = text.to_lowercase();
                self.refilter();
            }
            MainInput::OpenInCurrent(chat_id) => {
                if let Some((_, page)) = self.open_tabs.get(&chat_id) {
                    // Already open in some tab — just focus.
                    self.tab_view.set_selected_page(page);
                    let _ = sender.output(MainOutput::FocusChat(Some(chat_id)));
                } else if self.open_tabs.is_empty() {
                    // No tabs yet — open fresh.
                    let _ = sender.output(MainOutput::OpenChatNew(chat_id));
                } else {
                    // Reuse the currently-selected tab: close it, then open
                    // the new chat. Browser-style "click bookmark, opens
                    // here" behaviour.
                    // close_page() emits `close-page`; our signal handler
                    // returns Stop and dispatches MainInput::TabClosed,
                    // which then calls close_page_finish. Trying to call
                    // close_page_finish directly here trips an assertion
                    // because page->closing isn't set yet.
                    if let Some(current) = self.tab_view.selected_page() {
                        self.tab_view.close_page(&current);
                    }
                    let _ = sender.output(MainOutput::OpenChatNew(chat_id));
                }
            }
            MainInput::OpenInNewTab(chat_id) => {
                if let Some((_, page)) = self.open_tabs.get(&chat_id) {
                    self.tab_view.set_selected_page(page);
                    let _ = sender.output(MainOutput::FocusChat(Some(chat_id)));
                } else {
                    let _ = sender.output(MainOutput::OpenChatNew(chat_id));
                }
            }
            MainInput::ChatOpened {
                chat_id: Some(chat_id),
                name,
                kind,
                messages,
            } => {
                self.chat_meta
                    .insert(chat_id.clone(), (name.clone(), kind.clone()));
                if let Some((controller, page)) = self.open_tabs.get(&chat_id) {
                    // Existing tab → reset its content.
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
                                MainInput::SendFromTab { chat_id, text }
                            }
                            ChatTabOutput::Close { chat_id } => MainInput::TabClosed(chat_id),
                            ChatTabOutput::RequestMediaDownload(id) => {
                                MainInput::RequestMediaDownload(id)
                            }
                            ChatTabOutput::RequestLoadOlder { chat_id, before_ts } => {
                                MainInput::RequestLoadOlder { chat_id, before_ts }
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
                // Refresh the header for the now-selected chat.
                self.current_chat_name = name;
                self.current_chat_kind = kind;
                self.current_chat_id = Some(chat_id.clone());
                // Look up the cached avatar path from the sidebar's
                // factory; if missing, request a fresh fetch.
                self.current_chat_avatar = self
                    .chats
                    .guard()
                    .iter()
                    .find(|f| f.item.chat_id == chat_id)
                    .and_then(|f| f.item.avatar_path.clone());
                if self.current_chat_avatar.is_none()
                    && self.avatar_requested.insert(chat_id.clone())
                {
                    let _ =
                        sender.output(MainOutput::RequestFetchAvatar(chat_id.clone()));
                }
            }
            MainInput::ChatOpened { chat_id: None, .. } => {
                // Service told us "no chat open" — leave tabs as-is.
            }
            MainInput::MessagesAppended { chat_id, messages } => {
                tracing::info!(
                    chat = %chat_id,
                    count = messages.len(),
                    open_tabs = self.open_tabs.len(),
                    has_tab = self.open_tabs.contains_key(&chat_id),
                    "main: MessagesAppended → tab",
                );
                if let Some((controller, _)) = self.open_tabs.get(&chat_id) {
                    let _ = controller.sender().send(ChatTabInput::Append(messages));
                } else {
                    tracing::warn!(
                        chat = %chat_id,
                        "MessagesAppended received for chat with no open tab",
                    );
                }
            }
            MainInput::TabSelected(chat_id) => {
                if let Some(id) = &chat_id {
                    if let Some((name, kind)) = self.chat_meta.get(id) {
                        self.current_chat_name = name.clone();
                        self.current_chat_kind = kind.clone();
                    }
                    if let Some((controller, _)) = self.open_tabs.get(id) {
                        let _ = controller.sender().send(ChatTabInput::StickToBottom);
                    }
                } else if self.open_tabs.is_empty() {
                    self.current_chat_name.clear();
                    self.current_chat_kind.clear();
                } else {
                    // Spurious selected-page-notify (most often the one
                    // that fires immediately after `tab_view.append`,
                    // BEFORE we've set the new page's keyword). Keep the
                    // current title intact instead of blanking the
                    // headerbar — the keyword-bearing notify will arrive
                    // moments later.
                    return;
                }
                let _ = sender.output(MainOutput::FocusChat(chat_id));
            }
            MainInput::TabClosed(chat_id) => {
                if let Some((controller, page)) = self.open_tabs.remove(&chat_id) {
                    self.tab_view.close_page_finish(&page, true);
                    drop(controller);
                }
                self.chat_meta.remove(&chat_id);
                self.tab_count = self.open_tabs.len();
                if self.open_tabs.is_empty() {
                    self.current_chat_name.clear();
                    self.current_chat_kind.clear();
                    let _ = sender.output(MainOutput::FocusChat(None));
                }
            }
            MainInput::SendFromTab { chat_id, text } => {
                let _ = sender.output(MainOutput::SendText { chat_id, text });
            }
            MainInput::Repair => {
                let _ = sender.output(MainOutput::RequestRepair);
            }
            MainInput::Logout => {
                let _ = sender.output(MainOutput::RequestLogout);
            }
            MainInput::MediaReady {
                message_ids,
                path,
                mimetype,
            } => {
                // Broadcast: any tab that currently shows one of these
                // message_ids needs to re-render its bubble. Cheap because
                // each tab indexes by id internally.
                for (_, (controller, _)) in self.open_tabs.iter() {
                    let _ = controller.sender().send(ChatTabInput::MediaReady {
                        message_ids: message_ids.clone(),
                        path: path.clone(),
                        mimetype: mimetype.clone(),
                    });
                }
            }
            MainInput::MediaFailed { message_id } => {
                for (_, (controller, _)) in self.open_tabs.iter() {
                    let _ = controller
                        .sender()
                        .send(ChatTabInput::MediaFailed(message_id.clone()));
                }
            }
            MainInput::RequestMediaDownload(id) => {
                let _ = sender.output(MainOutput::RequestMediaDownload(id));
            }
            MainInput::RequestLoadOlder { chat_id, before_ts } => {
                let _ = sender.output(MainOutput::RequestLoadOlder { chat_id, before_ts });
            }
            MainInput::AvatarReady { jid, path } => {
                // Sidebar: patch any factory item whose chat_id resolves
                // to the same alias the worker just wrote. We use chat_id
                // = jid because the chat list and the worker use the same
                // canonical aliasing.
                let indices: Vec<usize> = self
                    .chats
                    .guard()
                    .iter()
                    .enumerate()
                    .filter_map(|(i, f)| {
                        if f.item.chat_id == jid {
                            Some(i)
                        } else {
                            None
                        }
                    })
                    .collect();
                if !indices.is_empty() {
                    let mut guard = self.chats.guard();
                    for idx in indices {
                        if let Some(slot) = guard.get_mut(idx) {
                            slot.item.avatar_path = Some(path.clone());
                        }
                    }
                }
                // Headerbar: refresh if the affected chat is the focused one.
                if self.current_chat_id.as_deref() == Some(jid.as_str()) {
                    self.current_chat_avatar = Some(path);
                }
            }
            MainInput::OlderMessagesLoaded {
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
            MainInput::SetRepairing(r) => {
                self.repairing = r;
                if !r {
                    self.repair_current = 0;
                    self.repair_total = 0;
                    self.repair_stage.clear();
                }
            }
            MainInput::RepairProgress {
                stage,
                current,
                total,
                indeterminate,
            } => {
                self.repair_stage = stage;
                self.repair_current = current;
                self.repair_total = total;
                self.repair_indeterminate = indeterminate;
            }
        }
    }
}

impl MainPage {
    /// Upsert by chat_id, then resort by (pinned desc, last_ts desc).
    fn apply_chats_upserted(&mut self, rows: Vec<ChatRow>) {
        let mut guard = self.chats.guard();

        // Build index of current items by chat_id.
        let mut existing: HashMap<String, usize> = HashMap::with_capacity(guard.len());
        for (idx, item) in guard.iter().enumerate() {
            existing.insert(item.item.chat_id.clone(), idx);
        }

        for row in &rows {
            let item = ChatRowItem::from_row(row);
            if let Some(&idx) = existing.get(&item.chat_id) {
                if let Some(slot) = guard.get_mut(idx) {
                    slot.item = item;
                }
            } else {
                guard.push_back(item);
            }
        }

        // Resort the whole list. Cheap enough for hundreds of chats; if it
        // ever shows up in profiles, switch to a sorted-insert.
        let mut all: Vec<ChatRowItem> = guard.iter().map(|f| f.item.clone()).collect();
        all.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then(b.last_ts.cmp(&a.last_ts))
                .then_with(|| a.name.as_str().cmp(b.name.as_str()))
        });
        guard.clear();
        for item in all {
            guard.push_back(item);
        }
    }

    fn refilter(&mut self) {
        // Filtering is implemented via gtk::ListBox::set_filter_func — but
        // since FactoryVecDeque rebuilds rows we'd lose state. Skipped for
        // the first cut; UI still works without it.
    }
}
