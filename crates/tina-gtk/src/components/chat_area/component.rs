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
use relm4::prelude::*;

use super::messages::{ChatAreaInit, ChatAreaInput, ChatAreaOutput};
use super::model::ChatArea;
use super::pane::build_pane;

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

        install_breakpoint(&root, &pane0, &pane1, &pane1_revealer, &sender);

        let model = ChatArea {
            panes: [pane0, pane1],
            open_tabs: HashMap::new(),
            chat_meta: HashMap::new(),
            paned: widgets.paned.clone(),
            pane1_revealer,
            focused_pane: 0,
            avatars: init.avatars,
            media: init.media,
            chats: init.chats,
            user_jid: None,
        };
        model.refresh_pane_visibility();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: ChatAreaInput, sender: ComponentSender<Self>) {
        self.dispatch(msg, sender);
    }
}

/// Adaptive narrow mode:
///   1. Hide the toggle-sidebar button on pane 0's header — the
///      AdwNavigationPage already exposes a Back button, so it's
///      redundant.
///   2. Hide both panes' split-move buttons — split layout is
///      unavailable in compact width.
///   3. Collapse the Revealer (pane 1 slides out).
///   4. On apply, also auto-merge any pane 1 tabs back into pane 0 so
///      they aren't stranded in an inaccessible pane.
fn install_breakpoint(
    root: &adw::BreakpointBin,
    pane0: &super::pane::Pane,
    pane1: &super::pane::Pane,
    pane1_revealer: &gtk::Revealer,
    sender: &ComponentSender<ChatArea>,
) {
    let bp = adw::Breakpoint::new(
        adw::BreakpointCondition::parse("max-width: 700sp")
            .expect("hardcoded breakpoint condition is well-formed"),
    );
    bp.add_setter(pane1_revealer, "reveal-child", Some(&false.to_value()));
    if let Some(toggle) = &pane0.toggle_btn {
        bp.add_setter(toggle, "visible", Some(&false.to_value()));
    }
    bp.add_setter(&pane0.split_btn, "visible", Some(&false.to_value()));
    bp.add_setter(&pane1.split_btn, "visible", Some(&false.to_value()));
    let s = sender.input_sender().clone();
    bp.connect_apply(move |_| {
        let _ = s.send(ChatAreaInput::AutoMergePane1);
    });
    root.add_breakpoint(bp);
}
