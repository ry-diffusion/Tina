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
";
