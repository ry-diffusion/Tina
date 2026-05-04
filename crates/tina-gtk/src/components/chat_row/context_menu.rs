// Right-click context menu (Open / Open in new tab / Pin) attached to
// every chat row. Communicates with the sidebar via a process-wide
// `OnceLock` sender — a single sidebar instance per app makes the
// global safe.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;

use adw::prelude::*;
use relm4::Sender;

use crate::components::sidebar::SidebarInput;

#[derive(Clone, Debug)]
pub struct RowMenuTarget {
    pub chat_id: String,
    pub pinned: bool,
}

/// Sender registered once at sidebar init; `attach_context_menu`'s
/// click handlers read from it. `OnceLock` is enough for our case
/// since the app only ever has one sidebar.
static CONTEXT_MENU_SENDER: OnceLock<Sender<SidebarInput>> = OnceLock::new();

/// Wire the per-row sender so right-click context menus can dispatch
/// `SidebarInput`s. Idempotent — call once on sidebar startup.
pub fn install_context_menu_sender(sender: Sender<SidebarInput>) {
    let _ = CONTEXT_MENU_SENDER.set(sender);
}

pub fn attach_context_menu(root: &gtk::Box, target: Rc<RefCell<Option<RowMenuTarget>>>) {
    let popover = gtk::Popover::builder()
        .has_arrow(false)
        .position(gtk::PositionType::Bottom)
        // `menu` styles the popover like a GtkPopoverMenu — flat
        // background, tighter row padding, no extra margins. Matches
        // Nautilus's right-click menu look.
        .css_classes(["menu"])
        .build();

    let menu = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .build();

    menu.append(&menu_row(
        "Open",
        Box::new(SidebarInput::OpenChatRequested),
        target.clone(),
        &popover,
    ));
    menu.append(&menu_row(
        "Open in New Tab",
        Box::new(SidebarInput::OpenInNewTabRequested),
        target.clone(),
        &popover,
    ));

    let (pin_btn, pin_label) = build_pin_button(target.clone(), &popover);
    menu.append(&pin_btn);

    popover.set_child(Some(&menu));
    popover.set_parent(root);

    attach_gesture(root, &popover, target, pin_label);
}

/// A flat button shaped like a `GtkPopoverMenu` row — left-aligned
/// label, single line, no border. Wrapping each button in a Box so we
/// can swap content without rebuilding the row.
fn menu_row(
    label: &str,
    mk_msg: Box<dyn Fn(String) -> SidebarInput + 'static>,
    target: Rc<RefCell<Option<RowMenuTarget>>>,
    popover: &gtk::Popover,
) -> gtk::Button {
    let row_label = gtk::Label::builder()
        .label(label)
        .xalign(0.0)
        .hexpand(true)
        .build();
    let btn = gtk::Button::builder()
        .css_classes(["flat"])
        .child(&row_label)
        .build();
    let pop = popover.clone();
    btn.connect_clicked(move |_| {
        if let (Some(t), Some(sender)) = (target.borrow().clone(), CONTEXT_MENU_SENDER.get()) {
            let _ = sender.send(mk_msg(t.chat_id));
        }
        pop.popdown();
    });
    btn
}

fn build_pin_button(
    target: Rc<RefCell<Option<RowMenuTarget>>>,
    popover: &gtk::Popover,
) -> (gtk::Button, gtk::Label) {
    let pin_label = gtk::Label::builder()
        .label("Pin")
        .xalign(0.0)
        .hexpand(true)
        .build();
    let pin_btn = gtk::Button::builder()
        .css_classes(["flat"])
        .child(&pin_label)
        .build();
    let pop = popover.clone();
    pin_btn.connect_clicked(move |_| {
        if let (Some(t), Some(sender)) = (target.borrow().clone(), CONTEXT_MENU_SENDER.get()) {
            let _ = sender.send(SidebarInput::PinChatRequested {
                chat_id: t.chat_id,
                pinned: !t.pinned,
            });
        }
        pop.popdown();
    });
    (pin_btn, pin_label)
}

fn attach_gesture(
    root: &gtk::Box,
    popover: &gtk::Popover,
    target: Rc<RefCell<Option<RowMenuTarget>>>,
    pin_label: gtk::Label,
) {
    let gesture = gtk::GestureClick::new();
    gesture.set_button(gtk::gdk::BUTTON_SECONDARY);
    let pop = popover.clone();
    gesture.connect_pressed(move |_, _, x, y| {
        if let Some(t) = target.borrow().clone() {
            // Update the inner label rather than calling
            // `Button::set_label` — the latter swaps in a brand-new
            // child label and drops our xalign/hexpand styling.
            pin_label.set_label(if t.pinned { "Unpin" } else { "Pin" });
        }
        let rect = gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1);
        pop.set_pointing_to(Some(&rect));
        pop.popup();
    });
    root.add_controller(gesture);
}
