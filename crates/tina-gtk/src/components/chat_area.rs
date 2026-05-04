// Right side of the in-app page: the multi-tab, split-capable chat surface.
//
// Two `AdwTabView`s sit in a horizontal `gtk::Paned` so the user can have
// up to two independent groups of chat tabs side-by-side (VSCode "editor
// groups", but capped at 2 — pure libadwaita has no native N-way split).
// Each pane is a self-contained `AdwToolbarView` with its own headerbar
// and a `Stack { single | multi }` title widget: when the pane has only
// one tab it shows a centred avatar+name; with two or more it shows the
// pane's `AdwTabBar`. Pane 1 is hidden until the user moves a tab into
// it, so single-pane mode looks identical to a one-tab-view chat.
//
// One quirk worth flagging: every open tab gets `MessagesAppended` push
// deltas from the worker, regardless of which pane it sits in — but only
// chats present in the worker's open-set are emitted in the first place.
// Closed tabs stay at the snapshot they were loaded with until the user
// reopens them.

use std::collections::HashMap;

use adw::prelude::*;
use gtk::glib;
use relm4::Controller;
use relm4::prelude::*;
use tina_db::MessageRow;

use crate::components::chat_tab::{ChatTab, ChatTabInit, ChatTabInput, ChatTabOutput};
use crate::inventory::{AvatarInventory, MediaInventory};

pub struct ChatAreaInit {
    pub avatars: AvatarInventory,
    pub media: MediaInventory,
}

#[derive(Debug)]
pub enum ChatAreaInput {
    /// User picked a chat from the sidebar — reuse the focused pane's
    /// selected tab if one's open, else open a fresh tab in that pane.
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
    AvatarReady {
        jid: String,
        path: String,
    },
    /// A pane's selected tab changed; route StickToBottom + update headerbar.
    PaneTabSelected {
        pane: usize,
        chat_id: Option<String>,
    },
    /// AdwTabView signalled close-page; finalize teardown.
    TabClosed {
        pane: usize,
        chat_id: String,
    },
    /// User pressed the "move to other split" button on pane `from`.
    /// Transfers the pane's currently-selected tab to the opposite pane,
    /// creating the split if it wasn't visible.
    MoveTabToOtherPane(usize),
    /// User clicked into a pane — make it the routing target for new
    /// chats. Selecting a tab inside a pane already does this via
    /// PaneTabSelected, but a click in an empty pane needs its own path.
    PaneFocused(usize),
    /// Adaptive: window narrowed below the split threshold. Move every
    /// pane 1 tab back into pane 0 so they don't end up stranded in a
    /// hidden pane the user can't reach without widening again.
    AutoMergePane1,
    /// Forwarded from a ChatTab.
    SendFromTab {
        chat_id: String,
        text: String,
    },
    /// Forwarded from a ChatTab.
    RequestMediaDownload(String),
    /// Forwarded from a ChatTab.
    RequestLoadOlder {
        chat_id: String,
        before_ts: i64,
    },
    /// Forwarded from a ChatTab — sender-avatar fetch.
    RequestFetchAvatar(String),
    /// Identity arrived (or changed). Stored for new tabs + forwarded
    /// to existing ones so from_me rows pick up the user avatar.
    SetUserJid(Option<String>),
}

#[derive(Debug)]
pub enum ChatAreaOutput {
    ToggleSidebar(bool),
    /// Ask the worker to fetch metadata + first page for `chat_id`. Comes
    /// back as `ChatOpened` via the parent.
    OpenChatNew(String),
    SendText {
        chat_id: String,
        text: String,
    },
    /// A chat was closed in the UI — parent must tell the worker so it
    /// stops emitting `MessagesAppended` for it.
    CloseChat(String),
    RequestMediaDownload(String),
    RequestLoadOlder {
        chat_id: String,
        before_ts: i64,
    },
    RequestFetchAvatar(String),
    /// The set of chat_ids currently open in tabs (across both panes).
    /// Emitted whenever a tab opens or closes so the sidebar can
    /// highlight + sort-to-top the active chats.
    ActiveTabsChanged(Vec<String>),
}

/// One side of the split. Owns the widgets needed to render its own
/// headerbar (Stack { single | multi }) plus the AdwTabView underneath.
struct Pane {
    tab_view: adw::TabView,
    toolbar_view: adw::ToolbarView,
    header: adw::HeaderBar,
    stack: gtk::Stack,
    avatar: adw::Avatar,
    title: adw::WindowTitle,
    /// Move-to-other-split button. Disabled when this pane has no tab
    /// to move (an empty pane can't split anywhere). Hidden when the
    /// chat area is narrow (split is unavailable in compact layout).
    split_btn: gtk::Button,
    /// Sidebar toggle (only present on pane 0). Hidden when narrow,
    /// since AdwNavigationSplitView already provides a back button via
    /// the navigation page header.
    toggle_btn: Option<gtk::ToggleButton>,
    /// Mirrors the currently-selected tab's chat_id so the single-tab
    /// header can render the right avatar+name.
    current_chat_id: Option<String>,
    current_chat_name: String,
    current_chat_avatar: Option<String>,
    current_chat_kind: String,
}

pub struct ChatArea {
    /// Two panes; pane 1's `toolbar_view` is hidden when empty so the
    /// Paned collapses visually to a single pane.
    panes: [Pane; 2],
    /// chat_id -> (controller, page, pane_idx).
    open_tabs: HashMap<String, (Controller<ChatTab>, adw::TabPage, usize)>,
    chat_meta: HashMap<String, (String, String)>,
    paned: gtk::Paned,
    /// Revealer wrapping pane 1, used to slide the second split in/out
    /// instead of toggling visibility instantly.
    pane1_revealer: gtk::Revealer,
    /// Pane that receives "open in current" clicks and new chats. Updated
    /// whenever a tab is selected in a pane (selection ⇒ implicit focus).
    focused_pane: usize,
    avatars: AvatarInventory,
    media: MediaInventory,
    user_jid: Option<String>,
}

#[relm4::component(pub)]
impl SimpleComponent for ChatArea {
    type Init = ChatAreaInit;
    type Input = ChatAreaInput;
    type Output = ChatAreaOutput;

    view! {
        #[root]
        adw::BreakpointBin {
            set_width_request: 360,
            set_height_request: 200,

            #[wrap(Some)]
            #[name(paned)]
            set_child = &gtk::Paned {
                set_orientation: gtk::Orientation::Horizontal,
                set_resize_start_child: true,
                set_resize_end_child: true,
                set_shrink_start_child: false,
                set_shrink_end_child: false,
            },
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let pane0 = build_pane(0, &sender);
        let pane1 = build_pane(1, &sender);

        // Wrap pane 1 in a Revealer so toggling the split slides instead
        // of snapping. SlideLeft = the new content slides in from the
        // right edge (toward the centre divider), which is the natural
        // direction for a "right pane appearing".
        let pane1_revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideLeft)
            .transition_duration(250)
            .reveal_child(false)
            .child(&pane1.toolbar_view)
            .build();

        let widgets = view_output!();
        widgets.paned.set_start_child(Some(&pane0.toolbar_view));
        widgets.paned.set_end_child(Some(&pane1_revealer));

        // Adaptive narrow mode:
        //   1. Hide the toggle-sidebar button on pane 0's header — the
        //      AdwNavigationPage already exposes a Back button, so it's
        //      redundant.
        //   2. Hide both panes' split-move buttons — split layout is
        //      unavailable in compact width.
        //   3. Collapse the Revealer (pane 1 slides out).
        //   4. On apply, also auto-merge any pane 1 tabs back into pane
        //      0 so they aren't stranded in an inaccessible pane.
        let bp = adw::Breakpoint::new(
            adw::BreakpointCondition::parse("max-width: 700sp")
                .expect("hardcoded breakpoint condition is well-formed"),
        );
        bp.add_setter(&pane1_revealer, "reveal-child", Some(&false.to_value()));
        if let Some(toggle) = &pane0.toggle_btn {
            bp.add_setter(toggle, "visible", Some(&false.to_value()));
        }
        bp.add_setter(&pane0.split_btn, "visible", Some(&false.to_value()));
        bp.add_setter(&pane1.split_btn, "visible", Some(&false.to_value()));
        {
            let s = sender.input_sender().clone();
            bp.connect_apply(move |_| {
                let _ = s.send(ChatAreaInput::AutoMergePane1);
            });
        }
        root.add_breakpoint(bp);

        let model = ChatArea {
            panes: [pane0, pane1],
            open_tabs: HashMap::new(),
            chat_meta: HashMap::new(),
            paned: widgets.paned.clone(),
            pane1_revealer,
            focused_pane: 0,
            avatars: init.avatars,
            media: init.media,
            user_jid: None,
        };
        model.refresh_pane_visibility();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: ChatAreaInput, sender: ComponentSender<Self>) {
        match msg {
            ChatAreaInput::OpenInCurrent(chat_id) => {
                let pane_idx = self
                    .open_tabs
                    .get(&chat_id)
                    .map(|(_, _, p)| *p)
                    .unwrap_or(self.focused_pane);
                if let Some((_, page, _)) = self.open_tabs.get(&chat_id) {
                    self.panes[pane_idx].tab_view.set_selected_page(page);
                } else if self.pane_tab_count(self.focused_pane) == 0 {
                    let _ = sender.output(ChatAreaOutput::OpenChatNew(chat_id));
                } else {
                    if let Some(current) = self.panes[self.focused_pane].tab_view.selected_page() {
                        self.panes[self.focused_pane].tab_view.close_page(&current);
                    }
                    let _ = sender.output(ChatAreaOutput::OpenChatNew(chat_id));
                }
            }
            ChatAreaInput::OpenInNewTab(chat_id) => {
                if let Some((_, page, pane_idx)) = self.open_tabs.get(&chat_id) {
                    self.panes[*pane_idx].tab_view.set_selected_page(page);
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
                if let Some((controller, page, _)) = self.open_tabs.get(&chat_id) {
                    let _ = controller.sender().send(ChatTabInput::SetMeta {
                        name: name.clone(),
                        kind: kind.clone(),
                    });
                    let _ = controller.sender().send(ChatTabInput::Reset(messages));
                    page.set_title(&name);
                } else {
                    let target_pane = self.focused_pane;
                    let controller = ChatTab::builder()
                        .launch(ChatTabInit {
                            chat_id: chat_id.clone(),
                            name: name.clone(),
                            kind: kind.clone(),
                            initial: messages,
                            avatars: self.avatars.clone(),
                            media: self.media.clone(),
                            user_jid: self.user_jid.clone(),
                        })
                        .forward(sender.input_sender(), |o| match o {
                            ChatTabOutput::Send { chat_id, text } => {
                                ChatAreaInput::SendFromTab { chat_id, text }
                            }
                            ChatTabOutput::Close { chat_id } => {
                                // Routed through the focused pane on close-page.
                                ChatAreaInput::TabClosed {
                                    pane: 0,
                                    chat_id,
                                }
                            }
                            ChatTabOutput::RequestMediaDownload(id) => {
                                ChatAreaInput::RequestMediaDownload(id)
                            }
                            ChatTabOutput::RequestLoadOlder { chat_id, before_ts } => {
                                ChatAreaInput::RequestLoadOlder { chat_id, before_ts }
                            }
                            ChatTabOutput::RequestFetchAvatar(jid) => {
                                ChatAreaInput::RequestFetchAvatar(jid)
                            }
                        });
                    let widget = controller.widget().clone();
                    let page = self.panes[target_pane].tab_view.append(&widget);
                    page.set_title(&name);
                    page.set_keyword(&chat_id);
                    self.panes[target_pane].tab_view.set_selected_page(&page);
                    self.open_tabs
                        .insert(chat_id.clone(), (controller, page, target_pane));
                    // Populate the pane's single-mode header state directly.
                    // `selected_page_notify` fires on append() before we
                    // got a chance to set keyword(), so its callback
                    // can't recover the chat_id — we set it here instead.
                    let pane = &mut self.panes[target_pane];
                    pane.current_chat_id = Some(chat_id.clone());
                    pane.current_chat_name = name.clone();
                    pane.current_chat_kind = kind.clone();
                    pane.current_chat_avatar = self.avatars.get(&chat_id);
                    self.focused_pane = target_pane;
                }
                self.refresh_pane_visibility();
                self.refresh_pane_header(0);
                self.refresh_pane_header(1);
                self.broadcast_active_tabs(&sender);
                if self.avatars.get(&chat_id).is_none() && self.avatars.needs_fetch(&chat_id) {
                    let _ = sender.output(ChatAreaOutput::RequestFetchAvatar(chat_id));
                }
            }
            ChatAreaInput::MessagesAppended { chat_id, messages } => {
                if let Some((controller, _, _)) = self.open_tabs.get(&chat_id) {
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
                if let Some((controller, _, _)) = self.open_tabs.get(&chat_id) {
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
                self.media
                    .set_ready(&message_ids, &path, mimetype.as_deref());
                for (controller, _, _) in self.open_tabs.values() {
                    let _ = controller.sender().send(ChatTabInput::MediaReady {
                        message_ids: message_ids.clone(),
                        path: path.clone(),
                        mimetype: mimetype.clone(),
                    });
                }
            }
            ChatAreaInput::MediaFailed { message_id } => {
                self.media.set_failed(&message_id);
                for (controller, _, _) in self.open_tabs.values() {
                    let _ = controller
                        .sender()
                        .send(ChatTabInput::MediaFailed(message_id.clone()));
                }
            }
            ChatAreaInput::AvatarReady { jid, path } => {
                self.avatars.put(jid.clone(), path.clone());
                for pane in &mut self.panes {
                    if pane.current_chat_id.as_deref() == Some(jid.as_str()) {
                        pane.current_chat_avatar = Some(path.clone());
                    }
                }
                self.apply_pane_avatar(0);
                self.apply_pane_avatar(1);
                for (controller, _, _) in self.open_tabs.values() {
                    let _ = controller.sender().send(ChatTabInput::AvatarReady {
                        jid: jid.clone(),
                        path: path.clone(),
                    });
                }
            }
            ChatAreaInput::PaneTabSelected { pane, chat_id } => {
                self.focused_pane = pane;
                if let Some(id) = &chat_id {
                    if let Some((name, kind)) = self.chat_meta.get(id) {
                        self.panes[pane].current_chat_name = name.clone();
                        self.panes[pane].current_chat_kind = kind.clone();
                    }
                    self.panes[pane].current_chat_id = Some(id.clone());
                    self.panes[pane].current_chat_avatar = self.avatars.get(id);
                    if let Some((controller, _, _)) = self.open_tabs.get(id) {
                        let _ = controller.sender().send(ChatTabInput::StickToBottom);
                    }
                } else if self.pane_tab_count(pane) == 0 {
                    self.panes[pane].current_chat_id = None;
                    self.panes[pane].current_chat_name.clear();
                    self.panes[pane].current_chat_kind.clear();
                    self.panes[pane].current_chat_avatar = None;
                }
                self.refresh_pane_header(pane);
            }
            ChatAreaInput::TabClosed { pane: _, chat_id } => {
                if let Some((controller, page, pane_idx)) = self.open_tabs.remove(&chat_id) {
                    self.panes[pane_idx].tab_view.close_page_finish(&page, true);
                    drop(controller);
                }
                self.chat_meta.remove(&chat_id);
                self.refresh_pane_visibility();
                self.refresh_pane_header(0);
                self.refresh_pane_header(1);
                self.broadcast_active_tabs(&sender);
                let _ = sender.output(ChatAreaOutput::CloseChat(chat_id));
            }
            ChatAreaInput::PaneFocused(idx) => {
                self.focused_pane = idx;
            }
            ChatAreaInput::AutoMergePane1 => {
                // Drain pane 1 → pane 0 by transferring every page in
                // the natural order. After this, pane 1 is empty and
                // refresh_pane_visibility will collapse the revealer.
                while self.panes[1].tab_view.n_pages() > 0 {
                    let page = self.panes[1].tab_view.nth_page(0);
                    let chat_id = page.keyword().map(|s| s.to_string()).unwrap_or_default();
                    let dest = self.panes[0].tab_view.n_pages();
                    self.panes[1]
                        .tab_view
                        .transfer_page(&page, &self.panes[0].tab_view, dest);
                    if !chat_id.is_empty() {
                        if let Some(entry) = self.open_tabs.get_mut(&chat_id) {
                            entry.2 = 0;
                        }
                    }
                }
                self.focused_pane = 0;
                self.refresh_pane_visibility();
                self.refresh_pane_header(0);
                self.refresh_pane_header(1);
            }
            ChatAreaInput::MoveTabToOtherPane(from) => {
                let to = 1 - from;
                let Some(page) = self.panes[from].tab_view.selected_page() else {
                    return;
                };
                let Some(chat_id) = page.keyword().map(|s| s.to_string()) else {
                    return;
                };
                let pos = self.panes[to].tab_view.n_pages();
                self.panes[from]
                    .tab_view
                    .transfer_page(&page, &self.panes[to].tab_view, pos);
                if let Some(entry) = self.open_tabs.get_mut(&chat_id) {
                    entry.2 = to;
                }
                self.focused_pane = to;
                self.refresh_pane_visibility();
                self.refresh_pane_header(0);
                self.refresh_pane_header(1);
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
            ChatAreaInput::RequestFetchAvatar(jid) => {
                let _ = sender.output(ChatAreaOutput::RequestFetchAvatar(jid));
            }
            ChatAreaInput::SetUserJid(jid) => {
                self.user_jid = jid.clone();
                for (controller, _, _) in self.open_tabs.values() {
                    let _ = controller
                        .sender()
                        .send(ChatTabInput::SetUserJid(jid.clone()));
                }
            }
        }
    }
}

impl ChatArea {
    fn pane_tab_count(&self, idx: usize) -> i32 {
        self.panes[idx].tab_view.n_pages()
    }

    /// Snapshot the open chat_ids and emit them so the parent can
    /// forward to the sidebar (which highlights + sorts active chats
    /// to the top). Cheap; called only on tab open/close.
    fn broadcast_active_tabs(&self, sender: &ComponentSender<Self>) {
        let ids: Vec<String> = self.open_tabs.keys().cloned().collect();
        let _ = sender.output(ChatAreaOutput::ActiveTabsChanged(ids));
    }

    /// Reveal pane 1 only when it has tabs (so the Paned divider
    /// disappears too). Also keeps the window's close button visible on
    /// exactly one header — whichever pane is the rightmost-visible one
    /// — so the X never accidentally lives on a header that the user
    /// might mistake for "close this tab".
    fn refresh_pane_visibility(&self) {
        let p1_visible = self.pane_tab_count(1) > 0;
        self.panes[0].toolbar_view.set_visible(true);
        // Drive the Revealer instead of toolbar_view's visibility directly
        // — the Revealer slides the pane in/out and resizes the Paned
        // divider along with it.
        self.pane1_revealer.set_reveal_child(p1_visible);
        // Window controls live only on the rightmost visible pane.
        self.panes[0].header.set_show_end_title_buttons(!p1_visible);
        self.panes[1].header.set_show_end_title_buttons(p1_visible);
        // The split-move button needs at least one tab in the source pane.
        self.panes[0]
            .split_btn
            .set_sensitive(self.pane_tab_count(0) > 0);
        self.panes[1]
            .split_btn
            .set_sensitive(self.pane_tab_count(1) > 0);
        // When pane 1 just appeared, give it a sensible starting size.
        if p1_visible {
            let width = self.paned.width();
            if width > 0 && self.paned.position() <= 0 {
                self.paned.set_position(width / 2);
            }
        }
    }

    /// Pick the right child of the title Stack based on tab count, and
    /// repaint the single-tab avatar/name from the cached state.
    fn refresh_pane_header(&self, idx: usize) {
        let pane = &self.panes[idx];
        if self.pane_tab_count(idx) >= 2 {
            pane.stack.set_visible_child_name("multi");
        } else {
            pane.stack.set_visible_child_name("single");
            pane.title.set_title(&pane.current_chat_name);
            pane.avatar.set_text(Some(&pane.current_chat_name));
            self.apply_pane_avatar(idx);
        }
    }

    fn apply_pane_avatar(&self, idx: usize) {
        let pane = &self.panes[idx];
        let texture = pane
            .current_chat_avatar
            .as_deref()
            .and_then(|p| gtk::gdk::Texture::from_filename(p).ok())
            .map(|t| t.upcast::<gtk::gdk::Paintable>());
        pane.avatar.set_custom_image(texture.as_ref());
    }
}

fn build_pane(idx: usize, sender: &ComponentSender<ChatArea>) -> Pane {
    let tab_view = adw::TabView::new();
    let tab_bar = adw::TabBar::builder()
        .view(&tab_view)
        .autohide(false)
        .expand_tabs(false)
        .build();

    let avatar = adw::Avatar::builder()
        .size(30)
        .show_initials(true)
        .build();
    let title = adw::WindowTitle::new("", "");
    let single_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    single_box.append(&avatar);
    single_box.append(&title);

    let stack = gtk::Stack::new();
    stack.add_named(&single_box, Some("single"));
    stack.add_named(&tab_bar, Some("multi"));
    stack.set_visible_child_name("single");

    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&stack));

    let toggle_btn = if idx == 0 {
        let toggle = gtk::ToggleButton::builder()
            .icon_name("sidebar-show-symbolic")
            .active(true)
            .tooltip_text("Toggle sidebar")
            .build();
        let s = sender.output_sender().clone();
        toggle.connect_toggled(move |btn| {
            let _ = s.send(ChatAreaOutput::ToggleSidebar(btn.is_active()));
        });
        header.pack_start(&toggle);
        Some(toggle)
    } else {
        None
    };

    // "Move to other split" — same icon both panes; the model figures out
    // which way is "other" based on which pane fired the event.
    let split_btn = gtk::Button::builder()
        .icon_name("view-dual-symbolic")
        .tooltip_text(if idx == 0 {
            "Move tab to right split"
        } else {
            "Move tab to left split"
        })
        .build();
    let s = sender.input_sender().clone();
    split_btn.connect_clicked(move |_| {
        let _ = s.send(ChatAreaInput::MoveTabToOtherPane(idx));
    });
    header.pack_end(&split_btn);

    // Pane 1 starts hidden — no window controls until refresh_pane_visibility
    // says otherwise. Pane 0 keeps the default (true) for the single-pane case.
    if idx == 1 {
        header.set_show_end_title_buttons(false);
    }

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&header);
    toolbar_view.set_content(Some(&tab_view));

    // Click anywhere in the pane (including empty area when no tabs) →
    // make this pane the routing target for sidebar clicks. Capture-phase
    // so we hear the click even if a child consumes it.
    {
        let s = sender.input_sender().clone();
        let click = gtk::GestureClick::new();
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.set_button(0); // any button
        click.connect_pressed(move |_, _, _, _| {
            let _ = s.send(ChatAreaInput::PaneFocused(idx));
        });
        toolbar_view.add_controller(click);
    }

    {
        let s = sender.input_sender().clone();
        tab_view.connect_close_page(move |_view, page| {
            if let Some(chat_id) = page.keyword().map(|s| s.to_string()) {
                let _ = s.send(ChatAreaInput::TabClosed {
                    pane: idx,
                    chat_id,
                });
            }
            glib::Propagation::Stop
        });
    }
    {
        let s = sender.input_sender().clone();
        tab_view.connect_selected_page_notify(move |view| {
            let id = view
                .selected_page()
                .and_then(|p| p.keyword())
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty());
            let _ = s.send(ChatAreaInput::PaneTabSelected {
                pane: idx,
                chat_id: id,
            });
        });
    }

    Pane {
        tab_view,
        toolbar_view,
        header,
        stack,
        avatar,
        title,
        split_btn,
        toggle_btn,
        current_chat_id: None,
        current_chat_name: String::new(),
        current_chat_avatar: None,
        current_chat_kind: String::new(),
    }
}
