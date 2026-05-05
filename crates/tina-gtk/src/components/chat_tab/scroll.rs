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
            // Defer one tick — same pattern as Fractal's
            // `room_history::mod::scroll_down` (`idle_add_local_once`
            // → `listview.scroll_to(n_items - 1, …)`). Lets GTK
            // finish the layout pass that sized the new row before
            // we read `upper`/`page_size`, otherwise the target lands
            // on a stale upper and the row peeks above the bottom
            // edge.
            let adj = adj.clone();
            glib::idle_add_local_once(move || {
                let target = adj.upper() - adj.page_size();
                if target < 0.0 {
                    return;
                }
                // Skip the set_value if we're already there. GTK
                // emits value-changed even for no-op writes, which
                // would propagate through `wire_value_changed` and
                // potentially flip `bottomed` in edge cases (the
                // first event after a relayout briefly clamps value
                // before re-allocating). Avoiding the write avoids
                // that race entirely.
                if (adj.value() - target).abs() > 0.5 {
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
    scroll_lock: Rc<Cell<bool>>,
) {
    let scroll = scroll.clone();
    scroll.vadjustment().connect_value_changed(move |adj| {
        // Strong lock held during programmatic mutations
        // (`update_item_at` remove + insert + restore). Bails out
        // entirely so no NearTop / NearBottom / bottomed flip leaks
        // through the multi-event cascade.
        if scroll_lock.get() {
            return;
        }
        let value = adj.value();
        let page = adj.page_size();
        let upper = adj.upper();
        let bottom_value = upper - page;

        // `updated_value` is set by `wire_changed` whenever the
        // adjustment's upper changes — which happens BOTH for live
        // user content (new messages, scrollback pages) AND for
        // programmatic `items_changed` events when we mutate a row
        // in place (download status flip, avatar arrival, etc).
        // GTK can briefly clamp `value` to 0 during the relayout
        // window before re-allocating into the new size, and that
        // brief value=0 used to dispatch `NearTop` → which paged
        // older history → which left the user pinned at the top.
        // Suppress ALL the threshold-based dispatches when this
        // value-change came from a relayout, not the user.
        let is_relayout = updated_value.replace(false);
        if is_relayout {
            return;
        }
        bottomed.set(bottom_value < 0.0 || value >= bottom_value);

        if value < page * 2.0 && upper > page * 2.0 {
            let _ = input.send(ChatTabInput::NearTop);
        }
        if value >= bottom_value - 50.0 {
            let _ = input.send(ChatTabInput::NearBottom);
        }
        // Symmetric of NearTop's `value < page * 2.0` zone: when the
        // user is within two viewports of the tail, fire a fetch for
        // newer rows. The handler short-circuits if `reached_bottom`
        // is already true (factory tail is the DB tail) so this is
        // cheap to fire on every value-changed during a downward
        // scroll.
        if value > bottom_value - page * 2.0 && upper > page * 2.0 {
            let _ = input.send(ChatTabInput::NearBottomFetch);
        }
    });
}

/// Force-scroll-to-bottom helper used when the user switches into a
/// tab. Schedules both an idle and a 50ms timeout so we catch the
/// layout pass.
pub fn force_to_bottom(scroll: &gtk::ScrolledWindow) {
    let snap = |adj: &gtk::Adjustment| {
        let target = adj.upper() - adj.page_size();
        if target < 0.0 {
            return;
        }
        if (adj.value() - target).abs() > 0.5 {
            adj.set_value(target);
        }
    };
    let s1 = scroll.clone();
    glib::idle_add_local_once(move || snap(&s1.vadjustment()));
    let s2 = scroll.clone();
    // Belt-and-suspenders 50 ms timeout — covers the case where the
    // initial idle pass ran before the new rows finished allocating
    // (GTK fires `changed` before `upper-notify` settles). 50 ms is
    // enough that GTK has done at least one layout cycle on every
    // realistic display refresh rate (60 Hz → 16.7 ms / frame).
    glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
        snap(&s2.vadjustment())
    });
}
