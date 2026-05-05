// Sidebar of the in-app page: profile button (delegated to `ProfileMenu`),
// search entry, virtualised chat list (relm4's `TypedListView` over
// `gtk::ListView` — same pattern paper-plane uses, with cleaner row
// state plumbing than the raw factory + qdata approach), and the
// repair progress bar at the bottom.

use std::cell::{Cell, RefCell};
use crate::fl;
use std::rc::Rc;

use adw::prelude::*;
use relm4::prelude::*;
use relm4::typed_view::list::TypedListView;

use crate::components::chat_row::{ChatRowItem, install_context_menu_sender};
use crate::components::profile_menu::ProfileMenu;

use super::messages::{ChatFilter, SidebarInit, SidebarInput, SidebarOutput};
use super::model::Sidebar;
use super::status_row::StatusAuthorItem;

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
                    set_title: &fl!("app-title"),
                    #[watch]
                    set_subtitle: &model.status_subtitle(),
                },
            },

            // Thin pulsing sliver pinned under the headerbar. Visible
            // while we're indeterminate (Connecting, or repair waiting
            // for its first stage). The pulse animation is driven by
            // a glib timer started in `init()` — see `pulse_bar`.
            #[name(headerbar_pulse_bar)]
            add_top_bar = &gtk::ProgressBar {
                add_css_class: "osd",
                set_pulse_step: 0.08,
                #[watch]
                set_visible: model.status_bar_visible() && model.status_bar_pulsing(),
            },

            // Determinate sliver — only when we have a real fraction.
            // Kept as a separate widget so we don't fight the pulse:
            // calling `set_fraction()` cancels activity-mode in GTK,
            // and `pulse()` cancels the fraction.
            add_top_bar = &gtk::ProgressBar {
                add_css_class: "osd",
                #[watch]
                set_visible: model.status_bar_visible() && !model.status_bar_pulsing(),
                #[watch]
                set_fraction: model.status_bar_fraction().unwrap_or(0.0),
            },

            #[wrap(Some)]
            set_content = &gtk::Box {
                set_orientation: gtk::Orientation::Vertical,

                // Filter tab strip — All / Groups / Channels / Status.
                // `linked` collapses the toggle buttons into a single
                // segmented control. `set_active(true)` on init for
                // `All` makes it the default; the toggled handler
                // ignores fires from buttons that are turning OFF
                // (released by the new selection) so we don't get a
                // double-fire.
                gtk::Box {
                    set_orientation: gtk::Orientation::Horizontal,
                    set_homogeneous: true,
                    set_margin_top: 6,
                    set_margin_bottom: 6,
                    set_margin_start: 12,
                    set_margin_end: 12,
                    add_css_class: "linked",

                    #[name(filter_btn_all)]
                    gtk::ToggleButton {
                        set_label: &fl!("sidebar-filter-all"),
                        set_active: true,
                        connect_toggled[sender] => move |b| {
                            if b.is_active() {
                                sender.input(SidebarInput::SetChatFilter(ChatFilter::All));
                            }
                        },
                    },
                    #[name(filter_btn_groups)]
                    gtk::ToggleButton {
                        set_label: &fl!("sidebar-filter-groups"),
                        set_group: Some(&filter_btn_all),
                        connect_toggled[sender] => move |b| {
                            if b.is_active() {
                                sender.input(SidebarInput::SetChatFilter(ChatFilter::Groups));
                            }
                        },
                    },
                    #[name(filter_btn_channels)]
                    gtk::ToggleButton {
                        set_label: &fl!("sidebar-filter-channels"),
                        set_group: Some(&filter_btn_all),
                        connect_toggled[sender] => move |b| {
                            if b.is_active() {
                                sender.input(SidebarInput::SetChatFilter(ChatFilter::Channels));
                            }
                        },
                    },
                    #[name(filter_btn_status)]
                    gtk::ToggleButton {
                        set_label: &fl!("sidebar-filter-status"),
                        set_group: Some(&filter_btn_all),
                        connect_toggled[sender] => move |b| {
                            if b.is_active() {
                                sender.input(SidebarInput::SetChatFilter(ChatFilter::Status));
                            }
                        },
                    },
                },

                gtk::SearchEntry {
                    set_margin_top: 6,
                    set_margin_bottom: 6,
                    set_margin_start: 12,
                    set_margin_end: 12,
                    set_placeholder_text: Some(&fl!("sidebar-search")),
                    connect_search_changed[sender] => move |se| {
                        sender.input(SidebarInput::SearchChanged(se.text().to_string()));
                    },
                },

                // Stack switches between the regular chat list and the
                // status authors list based on the active tab. Both
                // views are scrolled, so the Stack lives directly
                // inside the sidebar Box with `vexpand: true` to take
                // all leftover space above the repair affordance.
                gtk::Stack {
                    set_vexpand: true,
                    set_transition_type: gtk::StackTransitionType::Crossfade,

                    #[name(scroll)]
                    add_named[Some("chats")] = &gtk::ScrolledWindow {
                        set_vexpand: true,
                        set_hscrollbar_policy: gtk::PolicyType::Never,

                        #[local_ref]
                        list_view -> gtk::ListView {
                            add_css_class: "navigation-sidebar",
                            set_single_click_activate: true,
                        },
                    },

                    // Status page is itself a Stack so we can swap to
                    // an empty-state placeholder when the author list
                    // is empty (no recent posts from contacts). Saves
                    // the user from staring at a blank list.
                    add_named[Some("status")] = &gtk::Stack {
                        add_named[Some("list")] = &gtk::ScrolledWindow {
                            set_vexpand: true,
                            set_hscrollbar_policy: gtk::PolicyType::Never,

                            #[local_ref]
                            status_list_view -> gtk::ListView {
                                add_css_class: "navigation-sidebar",
                                set_single_click_activate: true,
                            },
                        },

                        add_named[Some("empty")] = &adw::StatusPage {
                            set_icon_name: Some("loop-symbolic"),
                            set_title: &fl!("sidebar-no-status"),
                            set_description: Some(
                                fl!("sidebar-no-status-description").as_str()
                            ),
                            set_vexpand: true,
                        },

                        #[watch]
                        set_visible_child_name: if model.status_list.len() == 0 {
                            "empty"
                        } else {
                            "list"
                        },
                    },

                    #[watch]
                    set_visible_child_name: if model.chat_filter.get()
                        == ChatFilter::Status
                    {
                        "status"
                    } else {
                        "chats"
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
        _root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let mut list: TypedListView<ChatRowItem, gtk::SingleSelection> =
            TypedListView::with_sorting();
        let status_list: TypedListView<StatusAuthorItem, gtk::SingleSelection> =
            TypedListView::with_sorting();
        // Status list shouldn't auto-select either — same reason as
        // the chat list (sort churn drags the view).
        status_list.selection_model.set_autoselect(false);
        status_list.selection_model.set_can_unselect(true);
        status_list
            .selection_model
            .set_selected(gtk::INVALID_LIST_POSITION);

        let search_query: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
        let chat_filter: Rc<Cell<ChatFilter>> = Rc::new(Cell::new(ChatFilter::All));
        wire_search_filter(&mut list, search_query.clone(), chat_filter.clone());

        // No auto-selection: without this the SingleSelection picks the
        // first item the moment one arrives, and the ListView promptly
        // scrolls to keep "the selection" visible — every re-sort
        // during the initial load drags the view further down.
        list.selection_model.set_autoselect(false);
        list.selection_model.set_can_unselect(true);
        list.selection_model.set_selected(gtk::INVALID_LIST_POSITION);

        // Spread sort work across GTK frames instead of blocking the
        // main thread on a full re-sort every time items_changed fires.
        // The chain is: selection_model → FilterListModel → SortListModel.
        if let Some(sort_model) = list
            .selection_model
            .model()
            .and_downcast::<gtk::FilterListModel>()
            .and_then(|fm| fm.model())
            .and_downcast::<gtk::SortListModel>()
        {
            sort_model.set_incremental(true);
        }

        wire_activate(&list.view, sender.input_sender());
        wire_status_activate(&status_list.view, sender.input_sender());
        install_context_menu_sender(sender.input_sender().clone());

        let profile = ProfileMenu::builder()
            .launch(())
            .forward(sender.input_sender(), SidebarInput::FromProfile);

        let model = Sidebar {
            list,
            status_list,
            search_query,
            chat_filter,
            scroll: None,
            profile,
            repairing: false,
            repair_stage: String::new(),
            repair_current: 0,
            repair_total: 0,
            repair_indeterminate: true,
            connection: crate::app::ConnectionStatus::Connecting,
            history_sync_progress: None,
            history_sync_type: String::new(),
            user_jid: None,
            avatars: init.avatars,
            chats: init.chats,
            pending_avatar_fetches: std::collections::VecDeque::new(),
            in_flight_avatar_count: 0,
        };

        let list_view = &model.list.view;
        let status_list_view = &model.status_list.view;
        let widgets = view_output!();
        let mut model = model;
        model.scroll = Some(widgets.scroll.clone());

        // Drive the indeterminate sliver. We can't lean on
        // `set_pulse_step` alone — `pulse()` has to be called
        // periodically to actually animate. Weak ref so the timer
        // self-terminates when the headerbar is destroyed.
        {
            use gtk::glib;
            let weak = widgets.headerbar_pulse_bar.downgrade();
            glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                let Some(bar) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                if bar.is_visible() {
                    bar.pulse();
                }
                glib::ControlFlow::Continue
            });
        }
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: SidebarInput, sender: ComponentSender<Self>) {
        self.dispatch(msg, sender);
    }
}

/// Search predicate. The closure reads through a shared
/// Rc<RefCell<String>>; mutating the query + calling
/// `notify_filter_changed(0)` re-evaluates against every row.
fn wire_search_filter(
    list: &mut TypedListView<ChatRowItem, gtk::SingleSelection>,
    query: Rc<RefCell<String>>,
    filter: Rc<Cell<ChatFilter>>,
) {
    list.add_filter(move |item: &ChatRowItem| {
        // Tab filter first — cheaper than the lowercase/contains
        // search string match, and short-circuits early on tabs that
        // hide whole categories (e.g. Status under "All").
        if !filter.get().matches(&item.kind) {
            return false;
        }
        let needle = query.borrow();
        if needle.is_empty() {
            return true;
        }
        item.name.to_lowercase().contains(needle.as_str())
            || item.preview.to_lowercase().contains(needle.as_str())
    });
}

/// Activation: emit RowActivated with the visible (post-sort,
/// post-filter) position so we can resolve back to a chat_id.
fn wire_activate(view: &gtk::ListView, input: &relm4::Sender<SidebarInput>) {
    let s = input.clone();
    view.connect_activate(move |_, pos| {
        let _ = s.send(SidebarInput::RowActivated(pos));
    });
}

fn wire_status_activate(view: &gtk::ListView, input: &relm4::Sender<SidebarInput>) {
    let s = input.clone();
    view.connect_activate(move |_, pos| {
        let _ = s.send(SidebarInput::StatusAuthorActivated(pos));
    });
}
