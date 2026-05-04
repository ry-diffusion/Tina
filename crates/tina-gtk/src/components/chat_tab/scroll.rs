// Sticky-bottom autoscroll, ported from gotkit's `autoscroll.Window`.
//
// `connect_changed` fires when the adjustment's upper changes (i.e. new
// content was laid out into the listbox). If we were at the bottom, we
// jump back to the new bottom via idle_add — running through one extra
// frame matches dissent's behaviour and lets GTK finish allocating the
// new row before we set `value()`.
//
// `connect_value_changed` fires for both user scrolls AND our own
// set_value calls. The `updated_value` flag set in `wire_changed` lets
// us ignore the immediate echo from the relayout path; only genuine
// user input flips `bottomed` to false.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use relm4::Sender;

use super::messages::ChatTabInput;

pub fn wire_changed(
    scroll: &gtk::ScrolledWindow,
    bottomed: Rc<Cell<bool>>,
    updated_value: Rc<Cell<bool>>,
) {
    let scroll = scroll.clone();
    scroll.vadjustment().connect_changed(move |adj| {
        updated_value.set(true);
        if bottomed.get() {
            let adj = adj.clone();
            glib::idle_add_local_once(move || {
                let target = adj.upper() - adj.page_size();
                if target >= 0.0 {
                    adj.set_value(target);
                }
            });
        }
    });
}

/// Lazy-load on near-top, prune on near-bottom, and update the
/// bottomed flag based on user scroll position. We skip the
/// bottomed update on the first event after a relayout (signaled
/// by `updated_value`), since GTK can briefly clamp value before
/// re-allocating the new content.
pub fn wire_value_changed(
    scroll: &gtk::ScrolledWindow,
    input: Sender<ChatTabInput>,
    bottomed: Rc<Cell<bool>>,
    updated_value: Rc<Cell<bool>>,
) {
    let scroll = scroll.clone();
    scroll.vadjustment().connect_value_changed(move |adj| {
        let value = adj.value();
        let page = adj.page_size();
        let upper = adj.upper();
        let bottom_value = upper - page;

        if updated_value.replace(false) {
            // Came from a relayout — don't reinterpret as the
            // user scrolling away.
        } else {
            bottomed.set(bottom_value < 0.0 || value >= bottom_value);
        }

        if value < page * 2.0 && upper > page * 2.0 {
            let _ = input.send(ChatTabInput::NearTop);
        }
        if value >= bottom_value - 50.0 {
            let _ = input.send(ChatTabInput::NearBottom);
        }
    });
}

/// Force-scroll-to-bottom helper used when the user switches into a
/// tab. Schedules both an idle and a 50ms timeout so we catch the
/// layout pass.
pub fn force_to_bottom(scroll: &gtk::ScrolledWindow) {
    let s1 = scroll.clone();
    glib::idle_add_local_once(move || {
        let adj = s1.vadjustment();
        let target = adj.upper() - adj.page_size();
        if target >= 0.0 {
            adj.set_value(target);
        }
    });
    let s2 = scroll.clone();
    glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
        let adj = s2.vadjustment();
        let target = adj.upper() - adj.page_size();
        if target >= 0.0 {
            adj.set_value(target);
        }
    });
}
