// One side of the split. Owns the widgets needed to render its own
// headerbar (Stack { single | multi }) plus the AdwTabView underneath.

use adw::prelude::*;
use crate::fl;
use gtk::glib;
use relm4::prelude::*;

use super::messages::{ChatAreaInput, ChatAreaOutput};
use super::model::ChatArea;

pub(super) struct Pane {
    pub(super) tab_view: adw::TabView,
    pub(super) toolbar_view: adw::ToolbarView,
    pub(super) header: adw::HeaderBar,
    pub(super) stack: gtk::Stack,
    pub(super) avatar: adw::Avatar,
    pub(super) title: adw::WindowTitle,
    /// Move-to-other-split button. Disabled when this pane has no tab
    /// to move (an empty pane can't split anywhere). Hidden when the
    /// chat area is narrow (split is unavailable in compact layout).
    pub(super) split_btn: gtk::Button,
    /// Sidebar toggle (only present on pane 0). Hidden when narrow,
    /// since AdwNavigationSplitView already provides a back button via
    /// the navigation page header.
    pub(super) toggle_btn: Option<gtk::ToggleButton>,
    /// Mirrors the currently-selected tab's chat_id so the single-tab
    /// header can render the right avatar+name.
    pub(super) current_chat_id: Option<String>,
    pub(super) current_chat_name: String,
    pub(super) current_chat_avatar: Option<String>,
    pub(super) current_chat_kind: String,
}

pub(super) fn build_pane(idx: usize, sender: &ComponentSender<ChatArea>) -> Pane {
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

    let toggle_btn = build_toggle_btn(idx, &header, sender);
    let split_btn = build_split_btn(idx, &header, sender);

    // Pane 1 starts hidden — no window controls until refresh_pane_visibility
    // says otherwise. Pane 0 keeps the default (true) for the single-pane case.
    if idx == 1 {
        header.set_show_end_title_buttons(false);
    }

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&header);
    toolbar_view.set_content(Some(&tab_view));

    wire_pane_focus(idx, &toolbar_view, sender);
    wire_tab_signals(idx, &tab_view, sender);

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

fn build_toggle_btn(
    idx: usize,
    header: &adw::HeaderBar,
    sender: &ComponentSender<ChatArea>,
) -> Option<gtk::ToggleButton> {
    if idx != 0 {
        return None;
    }
    let toggle = gtk::ToggleButton::builder()
        .icon_name("sidebar-show-symbolic")
        .active(true)
        .tooltip_text(&fl!("pane-toggle-sidebar"))
        .build();
    let s = sender.output_sender().clone();
    toggle.connect_toggled(move |btn| {
        let _ = s.send(ChatAreaOutput::ToggleSidebar(btn.is_active()));
    });
    header.pack_start(&toggle);
    Some(toggle)
}

fn build_split_btn(
    idx: usize,
    header: &adw::HeaderBar,
    sender: &ComponentSender<ChatArea>,
) -> gtk::Button {
    // "Move to other split" — same icon both panes; the model figures out
    // which way is "other" based on which pane fired the event.
    let split_btn = gtk::Button::builder()
        .icon_name("view-dual-symbolic")
        .tooltip_text(&if idx == 0 {
            fl!("pane-move-right")
        } else {
            fl!("pane-move-left")
        })
        .build();
    let s = sender.input_sender().clone();
    split_btn.connect_clicked(move |_| {
        let _ = s.send(ChatAreaInput::MoveTabToOtherPane(idx));
    });
    header.pack_end(&split_btn);
    split_btn
}

/// Click anywhere in the pane (including empty area when no tabs) →
/// make this pane the routing target for sidebar clicks. Capture-phase
/// so we hear the click even if a child consumes it.
fn wire_pane_focus(
    idx: usize,
    toolbar_view: &adw::ToolbarView,
    sender: &ComponentSender<ChatArea>,
) {
    let s = sender.input_sender().clone();
    let click = gtk::GestureClick::new();
    click.set_propagation_phase(gtk::PropagationPhase::Capture);
    click.set_button(0);
    click.connect_pressed(move |_, _, _, _| {
        let _ = s.send(ChatAreaInput::PaneFocused(idx));
    });
    toolbar_view.add_controller(click);
}

fn wire_tab_signals(idx: usize, tab_view: &adw::TabView, sender: &ComponentSender<ChatArea>) {
    {
        let s = sender.input_sender().clone();
        tab_view.connect_close_page(move |_view, page| {
            if let Some(chat_id) = page.keyword().map(|s| s.to_string()) {
                let _ = s.send(ChatAreaInput::TabClosed { pane: idx, chat_id });
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
}
