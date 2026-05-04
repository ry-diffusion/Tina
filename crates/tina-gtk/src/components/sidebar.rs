// Sidebar of the in-app page: profile button (delegated to `ProfileMenu`),
// search entry, virtualised chat list (relm4's `TypedListView` over
// `gtk::ListView` — same pattern paper-plane uses, with cleaner row
// state plumbing than the raw factory + qdata approach), and the
// repair progress bar at the bottom.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use relm4::Controller;
use relm4::prelude::*;
use relm4::typed_view::list::TypedListView;
use tina_db::ChatRow;

use crate::components::chat_row::{ChatRowItem, install_context_menu_sender};
use crate::components::profile_menu::{ProfileMenu, ProfileMenuInput, ProfileMenuOutput};
use crate::inventory::AvatarInventory;

pub struct SidebarInit {
    pub avatars: AvatarInventory,
}

#[derive(Debug)]
pub enum SidebarInput {
    SetIdentity {
        phone: Option<String>,
        jid: Option<String>,
        push_name: Option<String>,
    },
    ChatsUpserted(Vec<ChatRow>),
    SearchChanged(String),
    SetRepairing(bool),
    RepairProgress {
        stage: String,
        current: i64,
        total: i64,
        indeterminate: bool,
    },
    AvatarReady {
        jid: String,
        path: String,
    },
    /// ListView's `activate` signal fired with the row position (in the
    /// post-filter, post-sort visible model).
    RowActivated(u32),
    /// Right-click context menu picked "Open".
    OpenChatRequested(String),
    /// Right-click context menu picked "Open in new tab".
    OpenInNewTabRequested(String),
    /// Right-click context menu picked "Pin" or "Unpin".
    PinChatRequested {
        chat_id: String,
        pinned: bool,
    },
    /// The set of chat_ids currently open as tabs in the chat area.
    /// Drives the "active" highlight + sort-to-top behaviour.
    SetActiveChats(Vec<String>),
    /// Forwarded from the profile menu child.
    FromProfile(ProfileMenuOutput),
}

#[derive(Debug)]
pub enum SidebarOutput {
    OpenInCurrent(String),
    OpenInNewTab(String),
    RequestRepair,
    RequestLogout,
    RequestFetchAvatar(String),
    SetChatPinned {
        chat_id: String,
        pinned: bool,
    },
}

pub struct Sidebar {
    /// Typed wrapper around `gtk::ListView` + `gio::ListStore` +
    /// sort/filter models. We talk to it through the wrapper's typed
    /// API; the unsafe boxing/unboxing of the row data happens inside
    /// the relm4 abstraction.
    list: TypedListView<ChatRowItem, gtk::SingleSelection>,
    /// Search query backing the (only) filter we register on `list`.
    /// Mutated on `SearchChanged`; the filter closure reads through it.
    search_query: Rc<RefCell<String>>,
    /// Stashed for the scroll-pinning snap. Captured from the view!
    /// macro at init time. While the user is parked at the top, every
    /// `ChatsUpserted` batch nudges the viewport back to 0 so the
    /// SortListModel's reorders don't drift the list to the bottom
    /// over the course of a sync. Once they scroll away, we leave
    /// them where they are.
    scroll: Option<gtk::ScrolledWindow>,
    profile: Controller<ProfileMenu>,
    repairing: bool,
    repair_stage: String,
    repair_current: i64,
    repair_total: i64,
    repair_indeterminate: bool,
    user_jid: Option<String>,
    avatars: AvatarInventory,
}

#[relm4::component(pub)]
impl SimpleComponent for Sidebar {
    type Init = SidebarInit;
    type Input = SidebarInput;
    type Output = SidebarOutput;

    view! {
        #[root]
        adw::ToolbarView {
            add_top_bar = &adw::HeaderBar {
                pack_start = model.profile.widget(),

                #[wrap(Some)]
                set_title_widget = &adw::WindowTitle {
                    set_title: "Tina",
                },
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
                        sender.input(SidebarInput::SearchChanged(se.text().to_string()));
                    },
                },

                #[name(scroll)]
                gtk::ScrolledWindow {
                    set_vexpand: true,
                    set_hscrollbar_policy: gtk::PolicyType::Never,

                    #[local_ref]
                    list_view -> gtk::ListView {
                        add_css_class: "navigation-sidebar",
                        set_single_click_activate: true,
                    },
                },

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
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // `with_sorting` builds the SortListModel from `ChatRowItem`'s
        // `Ord` (active → pinned → newest → alpha). Filtering goes on
        // top via add_filter.
        let mut list: TypedListView<ChatRowItem, gtk::SingleSelection> =
            TypedListView::with_sorting();

        // Search predicate. The closure reads through a shared
        // Rc<RefCell<String>>; mutating the query + calling
        // `notify_filter_changed(0)` re-evaluates against every row.
        let search_query: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
        {
            let q = search_query.clone();
            list.add_filter(move |item: &ChatRowItem| {
                let needle = q.borrow();
                if needle.is_empty() {
                    return true;
                }
                item.name.to_lowercase().contains(needle.as_str())
                    || item.preview.to_lowercase().contains(needle.as_str())
            });
        }

        // No auto-selection: without this the SingleSelection picks
        // the first item the moment one arrives, and the ListView
        // promptly scrolls to keep "the selection" visible — every
        // re-sort during the initial load drags the view further down.
        list.selection_model.set_autoselect(false);
        list.selection_model.set_can_unselect(true);
        list.selection_model.set_selected(gtk::INVALID_LIST_POSITION);

        // Activation: emit RowActivated with the visible (post-sort,
        // post-filter) position so we can resolve back to a chat_id.
        {
            let s = sender.input_sender().clone();
            list.view.connect_activate(move |_, pos| {
                let _ = s.send(SidebarInput::RowActivated(pos));
            });
        }

        // Wire the per-row context menu's static sender so its closures
        // can dispatch back into us without per-row sender clones.
        install_context_menu_sender(sender.input_sender().clone());

        let profile = ProfileMenu::builder()
            .launch(())
            .forward(sender.input_sender(), SidebarInput::FromProfile);

        let model = Sidebar {
            list,
            search_query,
            scroll: None,
            profile,
            repairing: false,
            repair_stage: String::new(),
            repair_current: 0,
            repair_total: 0,
            repair_indeterminate: true,
            user_jid: None,
            avatars: init.avatars,
        };

        let list_view = &model.list.view;
        let widgets = view_output!();
        // Stash the ScrolledWindow ref for the one-shot scroll-to-top
        // we run after the first ChatsUpserted batch lands.
        let mut model = model;
        model.scroll = Some(widgets.scroll.clone());
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: SidebarInput, sender: ComponentSender<Self>) {
        match msg {
            SidebarInput::SetIdentity {
                phone,
                jid,
                push_name,
            } => {
                self.user_jid = jid.clone();
                if let Some(j) = jid.as_deref() {
                    if !j.is_empty() {
                        if let Some(p) = self.avatars.get(j) {
                            let _ =
                                self.profile.sender().send(ProfileMenuInput::SetAvatar(p));
                        } else if self.avatars.needs_fetch(j) {
                            let _ = sender
                                .output(SidebarOutput::RequestFetchAvatar(j.to_string()));
                        }
                    }
                }
                let _ = self.profile.sender().send(ProfileMenuInput::SetIdentity {
                    phone,
                    jid,
                    push_name,
                });
            }
            SidebarInput::ChatsUpserted(mut rows) => {
                for r in &mut rows {
                    if r.avatar_path.is_none() {
                        if let Some(p) = self.avatars.get(&r.chat_id) {
                            r.avatar_path = Some(p);
                        }
                    }
                    if r.avatar_path.is_none() && self.avatars.needs_fetch(&r.chat_id) {
                        let _ =
                            sender.output(SidebarOutput::RequestFetchAvatar(r.chat_id.clone()));
                    }
                }
                // Snapshot whether the user is parked at the top BEFORE
                // applying the batch — once items_changed fires the
                // SortListModel can reorder rows and drift the value
                // arbitrarily, so the post-apply value is no longer a
                // reliable signal of user intent.
                let was_at_top = self
                    .scroll
                    .as_ref()
                    .map(|s| s.vadjustment().value() < 4.0)
                    .unwrap_or(true);
                self.apply_chats_upserted(rows);
                if was_at_top {
                    if let Some(scroll) = self.scroll.clone() {
                        // Defer to idle so the layout has settled
                        // before we set the value — calling set_value(0)
                        // before the upper has been recomputed is a no-op.
                        gtk::glib::idle_add_local_once(move || {
                            scroll.vadjustment().set_value(0.0);
                        });
                    }
                }
            }
            SidebarInput::SearchChanged(text) => {
                *self.search_query.borrow_mut() = text.to_lowercase();
                self.list.notify_filter_changed(0);
            }
            SidebarInput::SetRepairing(r) => {
                self.repairing = r;
                if !r {
                    self.repair_current = 0;
                    self.repair_total = 0;
                    self.repair_stage.clear();
                }
                let _ = self.profile.sender().send(ProfileMenuInput::SetRepairing(r));
            }
            SidebarInput::RepairProgress {
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
            SidebarInput::AvatarReady { jid, path } => {
                self.avatars.put(jid.clone(), path.clone());
                if let Some(pos) = self.find_chat_position(&jid) {
                    let prev = self.list.get(pos).map(|i| i.borrow().clone());
                    if let Some(mut prev) = prev {
                        prev.avatar_path = Some(path.clone());
                        self.replace_at(pos, prev);
                    }
                }
                if self.user_jid.as_deref() == Some(jid.as_str()) {
                    let _ = self.profile.sender().send(ProfileMenuInput::SetAvatar(path));
                }
            }
            SidebarInput::RowActivated(pos) => {
                if let Some(item) = self.list.get_visible(pos) {
                    let id = item.borrow().chat_id.clone();
                    let _ = sender.output(SidebarOutput::OpenInCurrent(id));
                }
            }
            SidebarInput::OpenChatRequested(id) => {
                let _ = sender.output(SidebarOutput::OpenInCurrent(id));
            }
            SidebarInput::OpenInNewTabRequested(id) => {
                let _ = sender.output(SidebarOutput::OpenInNewTab(id));
            }
            SidebarInput::PinChatRequested { chat_id, pinned } => {
                let _ = sender.output(SidebarOutput::SetChatPinned {
                    chat_id,
                    pinned,
                });
            }
            SidebarInput::SetActiveChats(ids) => {
                let new_active: std::collections::HashSet<String> =
                    ids.into_iter().collect();
                let total = self.list.len();
                for pos in 0..total {
                    let updated = self.list.get(pos).and_then(|item| {
                        let cur = item.borrow().clone();
                        let now_active = new_active.contains(&cur.chat_id);
                        if cur.is_active != now_active {
                            let mut next = cur.clone();
                            next.is_active = now_active;
                            Some(next)
                        } else {
                            None
                        }
                    });
                    if let Some(next) = updated {
                        self.replace_at(pos, next);
                    }
                }
            }
            SidebarInput::FromProfile(out) => match out {
                ProfileMenuOutput::Repair => {
                    let _ = sender.output(SidebarOutput::RequestRepair);
                }
                ProfileMenuOutput::Logout => {
                    let _ = sender.output(SidebarOutput::RequestLogout);
                }
            },
        }
    }
}

impl Sidebar {
    /// Linear search through the base store for a chat by id. The list
    /// is small enough (a few hundred chats max in practice) that the
    /// O(n) cost is invisible — and we already iterate the store on
    /// every `ChatsUpserted`, so adding a separate `chat_id → pos`
    /// index would just be more state to keep coherent.
    fn find_chat_position(&self, chat_id: &str) -> Option<u32> {
        let total = self.list.len();
        for pos in 0..total {
            if let Some(item) = self.list.get(pos) {
                if item.borrow().chat_id == chat_id {
                    return Some(pos);
                }
            }
        }
        None
    }

    /// Replace the item at `pos` with `item`. Triggers `items_changed`
    /// internally → the SortListModel re-evaluates the row's position
    /// and the bound widget rebinds, picking up the new fields.
    fn replace_at(&mut self, pos: u32, item: ChatRowItem) {
        self.list.remove(pos);
        self.list.insert(pos, item);
    }

    fn apply_chats_upserted(&mut self, rows: Vec<ChatRow>) {
        for row in &rows {
            let mut item = ChatRowItem::from_row(row);
            if let Some(pos) = self.find_chat_position(&item.chat_id) {
                if let Some(prev) = self.list.get(pos) {
                    // `is_active` is owned by the chat area (via
                    // `SetActiveChats`); a fresh `from_row` always
                    // reports `false`, so without this carry-over a
                    // new message landing for an open chat would
                    // silently strip its highlight.
                    item.is_active = prev.borrow().is_active;
                }
                self.replace_at(pos, item);
            } else {
                self.list.append(item);
            }
        }
    }
}
