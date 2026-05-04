// Sidebar of the in-app page: profile button (delegated to `ProfileMenu`),
// search entry, virtualised chat list (relm4's `TypedListView` over
// `gtk::ListView` — same pattern paper-plane uses, with cleaner row
// state plumbing than the raw factory + qdata approach), and the
// repair progress bar at the bottom.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use relm4::prelude::*;
use relm4::typed_view::list::TypedListView;

use crate::components::chat_row::{ChatRowItem, install_context_menu_sender};
use crate::components::profile_menu::ProfileMenu;

use super::messages::{SidebarInit, SidebarInput, SidebarOutput};
use super::model::Sidebar;

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
        _root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let mut list: TypedListView<ChatRowItem, gtk::SingleSelection> =
            TypedListView::with_sorting();

        let search_query: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
        wire_search_filter(&mut list, search_query.clone());

        // No auto-selection: without this the SingleSelection picks the
        // first item the moment one arrives, and the ListView promptly
        // scrolls to keep "the selection" visible — every re-sort
        // during the initial load drags the view further down.
        list.selection_model.set_autoselect(false);
        list.selection_model.set_can_unselect(true);
        list.selection_model.set_selected(gtk::INVALID_LIST_POSITION);

        wire_activate(&list.view, sender.input_sender());
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
        let mut model = model;
        model.scroll = Some(widgets.scroll.clone());
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
) {
    list.add_filter(move |item: &ChatRowItem| {
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
