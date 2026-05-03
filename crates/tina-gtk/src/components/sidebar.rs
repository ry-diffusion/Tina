// Sidebar of the in-app page: profile button (delegated to `ProfileMenu`),
// search entry, scrolling chat list, and the repair progress bar at the
// bottom. Owns the `chats` FactoryVecDeque; identity state lives in the
// child profile menu.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use gtk::gdk;
use relm4::Controller;
use relm4::factory::FactoryVecDeque;
use relm4::prelude::*;
use tina_db::ChatRow;

use crate::components::chat_row::{ChatRowFactory, ChatRowItem};
use crate::components::profile_menu::{ProfileMenu, ProfileMenuInput, ProfileMenuOutput};

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
    /// Internal: a row was activated (left click / Enter).
    RowActivated(String),
    /// Internal: right-click context menu picked "Open in new tab".
    OpenInNewTabRequested(String),
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
}

pub struct Sidebar {
    chats: FactoryVecDeque<ChatRowFactory>,
    profile: Controller<ProfileMenu>,
    repairing: bool,
    repair_stage: String,
    repair_current: i64,
    repair_total: i64,
    repair_indeterminate: bool,
    /// Currently-signed-in user's JID, used to filter `AvatarReady` events
    /// for the profile menu (the profile child stores it too, but caching
    /// here avoids a sync round-trip).
    user_jid: Option<String>,
    #[allow(dead_code)]
    search: String,
    /// JIDs we've already issued FetchAvatar for in this session.
    avatar_requested: std::collections::HashSet<String>,
}

#[relm4::component(pub)]
impl SimpleComponent for Sidebar {
    type Init = ();
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

                gtk::ScrolledWindow {
                    set_vexpand: true,
                    set_hscrollbar_policy: gtk::PolicyType::Never,

                    #[local_ref]
                    chat_listbox -> gtk::ListBox {
                        add_css_class: "navigation-sidebar",
                        set_selection_mode: gtk::SelectionMode::Single,
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
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let chats = FactoryVecDeque::<ChatRowFactory>::builder()
            .launch(gtk::ListBox::default())
            .detach();

        let profile = ProfileMenu::builder()
            .launch(())
            .forward(sender.input_sender(), SidebarInput::FromProfile);

        wire_chat_list_clicks(chats.widget(), sender.input_sender().clone());

        let model = Sidebar {
            chats,
            profile,
            repairing: false,
            repair_stage: String::new(),
            repair_current: 0,
            repair_total: 0,
            repair_indeterminate: true,
            user_jid: None,
            search: String::new(),
            avatar_requested: std::collections::HashSet::new(),
        };

        let chat_listbox = model.chats.widget();
        let widgets = view_output!();

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
                let _ = self.profile.sender().send(ProfileMenuInput::SetIdentity {
                    phone,
                    jid,
                    push_name,
                });
            }
            SidebarInput::ChatsUpserted(rows) => {
                for r in &rows {
                    if r.avatar_path.is_some() {
                        continue;
                    }
                    if self.avatar_requested.insert(r.chat_id.clone()) {
                        let _ =
                            sender.output(SidebarOutput::RequestFetchAvatar(r.chat_id.clone()));
                    }
                }
                self.apply_chats_upserted(rows);
            }
            SidebarInput::SearchChanged(text) => {
                self.search = text.to_lowercase();
                // Filtering via gtk::ListBox::set_filter_func loses state
                // when FactoryVecDeque rebuilds rows. Skipped for now.
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
                let indices: Vec<usize> = self
                    .chats
                    .guard()
                    .iter()
                    .enumerate()
                    .filter_map(|(i, f)| if f.item.chat_id == jid { Some(i) } else { None })
                    .collect();
                if !indices.is_empty() {
                    let mut guard = self.chats.guard();
                    for idx in indices {
                        if let Some(slot) = guard.get_mut(idx) {
                            slot.item.avatar_path = Some(path.clone());
                        }
                    }
                }
                if self.user_jid.as_deref() == Some(jid.as_str()) {
                    let _ = self.profile.sender().send(ProfileMenuInput::SetAvatar(path));
                }
            }
            SidebarInput::RowActivated(id) => {
                let _ = sender.output(SidebarOutput::OpenInCurrent(id));
            }
            SidebarInput::OpenInNewTabRequested(id) => {
                let _ = sender.output(SidebarOutput::OpenInNewTab(id));
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
    /// Upsert by chat_id, then resort by (pinned desc, last_ts desc).
    fn apply_chats_upserted(&mut self, rows: Vec<ChatRow>) {
        let mut guard = self.chats.guard();

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
}

/// Wires left-click activation and a right-click "Open / Open in new tab"
/// context menu onto the chat list. Pure imperative GTK plumbing — extracted
/// out of `init` to keep the component readable.
fn wire_chat_list_clicks(
    listbox: &gtk::ListBox,
    input: relm4::Sender<SidebarInput>,
) {
    // Left click → default activation. ListBoxRow alone only fires
    // `activate` on keyboard Enter; mouse clicks land on the parent
    // ListBox as `row-activated`. We pull chat_id from `widget_name`
    // (set by the factory).
    {
        let input = input.clone();
        listbox.connect_row_activated(move |_listbox, row| {
            let id = row.widget_name().to_string();
            if !id.is_empty() {
                let _ = input.send(SidebarInput::RowActivated(id));
            }
        });
    }

    // Right-click → context menu. The popover is a single shared widget
    // reparented (well, repointed) on each click; `target` carries the
    // chat_id between the gesture press and the button click.
    let target: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let popover = gtk::Popover::new();
    popover.set_has_arrow(false);
    popover.set_position(gtk::PositionType::Bottom);

    let menu = gtk::Box::new(gtk::Orientation::Vertical, 2);
    menu.set_margin_top(4);
    menu.set_margin_bottom(4);
    menu.set_margin_start(4);
    menu.set_margin_end(4);

    for (label, mk_msg) in [
        (
            "Open",
            Box::new(SidebarInput::RowActivated)
                as Box<dyn Fn(String) -> SidebarInput + 'static>,
        ),
        (
            "Open in new tab",
            Box::new(SidebarInput::OpenInNewTabRequested)
                as Box<dyn Fn(String) -> SidebarInput + 'static>,
        ),
    ] {
        let btn = gtk::Button::with_label(label);
        btn.add_css_class("flat");
        let input = input.clone();
        let target = target.clone();
        let pop = popover.clone();
        btn.connect_clicked(move |_| {
            if let Some(id) = target.borrow().clone() {
                let _ = input.send(mk_msg(id));
            }
            pop.popdown();
        });
        menu.append(&btn);
    }
    popover.set_child(Some(&menu));
    popover.set_parent(listbox);

    let listbox_clone = listbox.clone();
    let pop = popover.clone();
    let right_click = gtk::GestureClick::new();
    right_click.set_button(gdk::BUTTON_SECONDARY);
    right_click.connect_pressed(move |_g, _n, x, y| {
        if let Some(row) = listbox_clone.row_at_y(y as i32) {
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
    listbox.add_controller(right_click);
}
