// CSS for the chat-row "active" indicator. Loaded once at app start.

pub const CSS_TAB_OPEN: &str = "tina-tab-open";

/// Active rows (chats currently open in a tab) get a small accent
/// ring around the avatar — the same indicator paper-plane uses for
/// its `.selected-avatar` state. Subtle, doesn't fight with GTK's
/// own `:selected` row styling, and stays out of the way of pinned/
/// unread badge real estate on the right edge.
pub const CHAT_ROW_CSS: &str = "
.tina-tab-open avatar {
  outline: 2px solid @accent_color;
  outline-offset: 2px;
}

/* Story ring around avatars on the Status authors list — green
 * accent matches WhatsApp's brand colour for unviewed status posts.
 * `box-shadow` follows the widget's `border-radius`, but the OUTER
 * AdwAvatar widget node has no radius set (only the inner image
 * does). Forcing 9999px on the outer node makes box-shadow render
 * as a circular halo. The two stacked shadows are: an inner halo
 * matching the window background (creates the gap) + an outer ring
 * in the brand green. */
.tina-status-ring {
  border-radius: 9999px;
  box-shadow:
      0 0 0 2px @window_bg_color,
      0 0 0 4px #25d366;
}

/* Headerbar slim-down — matches Dissent's 42px convention and
 * Paper-Plane's tighter title padding. Default Adwaita tops out
 * around 46–50 px; trimming the windowhandle to ~38 px feels like
 * a desktop messenger without cramming buttons against the edge.
 * Applies globally to every AdwHeaderBar in the app (sidebar Tina,
 * each chat pane, lightbox, stories). */
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
  /* Pull the title closer to the bar so it visually centres
   * inside the slimmer height; Paper-Plane's
   * `.two-line-window-title` does the same with a -1px margin. */
  margin: -1px 0;
}
/* Buttons inherit the bar height; cap them so they don't push the
 * bar back to 46 px on themes that style buttons taller. The
 * `.flat` variant covers AdwSplitButton's chevron half. */
headerbar button,
headerbar button.image-button,
headerbar button.toggle,
headerbar button.flat {
  min-height: 28px;
  min-width: 28px;
  padding: 2px 6px;
}
/* Title labels — `.title` is the standard Adwaita class. Trimming
 * the line-height keeps the label from inflating the windowhandle
 * back up. */
headerbar .title {
  font-size: 0.95em;
  padding: 0;
}

/* Unread-message pill, WhatsApp-style. The `.tina-unread-badge`
 * class replaces the old `accent caption-heading` Label so the
 * badge is visually distinct (rounded green pill, white text)
 * instead of bleeding into the dim preview text. */
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
