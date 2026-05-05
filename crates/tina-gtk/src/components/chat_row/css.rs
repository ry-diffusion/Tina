// CSS for the sidebar chat list. Inspired by Fractal's sidebar style
// (`_session_view.scss`): rows have soft hover/selection backgrounds
// inside a rounded corner pill, the active row uses the accent
// background, and the notification count is a tiny rounded chip
// rendered in the row's own color. Status authors keep the WhatsApp
// brand-green ring; pin and unread badges sit in the same right-side
// gutter.

pub const CSS_TAB_OPEN: &str = "tina-tab-open";

pub const CHAT_ROW_CSS: &str = "
/* ── Sidebar list rows ───────────────────────────────────────────── */
.navigation-sidebar > listview > row {
  margin: 0;
  padding: 0;
  border-radius: 0;
  background: none;
}
.navigation-sidebar > listview > row:hover,
.navigation-sidebar > listview > row:selected {
  background: none;
}

/* The Box that `chat_row::widgets` builds gets the actual visual
 * treatment — that's where margin/padding/border-radius live. Mirrors
 * Fractal's `sidebar-row > *:not(popover)` pattern: the outer row is
 * a passthrough; the inner element owns the rounded background.
 *
 * Targeted by the row's own widget hierarchy, which is a horizontal
 * gtk::Box (the `build_root` from widgets.rs). */
.navigation-sidebar > listview > row > box {
  margin: 2px 6px;
  padding: 8px 10px;
  border-radius: 9px;
  transition: background-color 120ms;
}
.navigation-sidebar > listview > row:hover > box {
  background-color: alpha(@theme_fg_color, 0.06);
}
.navigation-sidebar > listview > row:active > box {
  background-color: alpha(@theme_fg_color, 0.10);
}
.navigation-sidebar > listview > row:selected > box,
.navigation-sidebar > listview > row.selected > box {
  background-color: alpha(@accent_bg_color, 0.18);
}
.navigation-sidebar > listview > row:selected:hover > box {
  background-color: alpha(@accent_bg_color, 0.24);
}

/* Active chat (currently open in a tab). Subtle accent ring on the
 * avatar — same indicator paper-plane uses for `.selected-avatar`. */
.tina-tab-open avatar {
  outline: 2px solid @accent_color;
  outline-offset: 2px;
}

/* Story ring around avatars on the Status authors list — WhatsApp
 * brand green for unviewed status posts. Two stacked shadows: an
 * inner halo matching the window background (creates the gap) +
 * an outer ring in green. */
.tina-status-ring {
  border-radius: 9999px;
  box-shadow:
      0 0 0 2px @window_bg_color,
      0 0 0 4px #25d366;
}

/* ── Headerbar slim-down ─────────────────────────────────────────── */
headerbar {
  min-height: 38px;
  padding: 0 6px;
}
headerbar > windowhandle {
  min-height: 38px;
}
headerbar > windowhandle > box.start,
headerbar > windowhandle > box.end,
headerbar > windowhandle > box.title-box {
  min-height: 38px;
  padding-top: 0;
  padding-bottom: 0;
}
headerbar > windowhandle > box.title-box {
  margin: -1px 0;
}
headerbar button,
headerbar button.image-button,
headerbar button.toggle,
headerbar button.flat {
  min-height: 28px;
  min-width: 28px;
  padding: 2px 6px;
}
headerbar .title {
  font-size: 0.95em;
  padding: 0;
}

/* ── Unread-message pill ─────────────────────────────────────────── */
/* WhatsApp green pill, white text. Replaces the old `accent
 * caption-heading` Label so the badge is visually distinct from the
 * dim preview text. */
.tina-unread-badge {
  background-color: #25d366;
  color: white;
  border-radius: 9999px;
  padding: 1px 7px;
  min-width: 14px;
  font-size: 0.75em;
  font-weight: 700;
}
";
