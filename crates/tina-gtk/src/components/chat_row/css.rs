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
";
