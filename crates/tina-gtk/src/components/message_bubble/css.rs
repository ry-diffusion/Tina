// CSS shipped alongside the bubble factory. Loaded once at app start via
// `relm4::set_global_css`.

pub const MESSAGE_ROW_CSS: &str = r#"
.message-row {
    padding: 0;
}
.message-box {
    border: 2px solid transparent;
    transition: linear 150ms background-color;
    padding: 2px 4px;
}
.message-row:hover .message-box {
    background-color: alpha(@theme_fg_color, 0.06);
    transition: none;
}
.message-row:focus .message-box {
    background-color: alpha(@theme_fg_color, 0.10);
    transition: none;
}
.message-cozy {
    padding-top: 6px;
}
.message-collapsed {
    padding-top: 0;
}
.message-cozy-header {
    min-height: 1.4em;
}
.message-cozy-avatar {
    margin-left: 8px;
    margin-right: 12px;
}
.message-collapsed-timestamp {
    opacity: 0;
    font-size: 0.7em;
    color: alpha(@theme_fg_color, 0.7);
}
.message-row:hover .message-collapsed-timestamp,
.message-row:focus .message-collapsed-timestamp {
    opacity: 1;
}
.message-content {
    margin-top: 1px;
    margin-bottom: 1px;
}
.message-picture {
    border-radius: 6px;
}
.message-reply-button {
    padding: 0;
    margin-bottom: 2px;
    margin-top: 1px;
    background: transparent;
    border: none;
    box-shadow: none;
    min-height: 0;
}
.message-reply-button:hover .message-reply-box {
    background-color: alpha(@theme_fg_color, 0.06);
}
.message-reply-box {
    border-left: 2px solid alpha(@theme_fg_color, 0.4);
    padding: 2px 8px;
    border-radius: 3px;
    font-size: 0.9em;
}
.message-reply-author {
    font-weight: 600;
    color: @accent_color;
}
.message-reply-preview {
    color: alpha(@theme_fg_color, 0.7);
}
.message-jump-highlight {
    background-color: alpha(@accent_color, 0.18);
    transition: background-color 600ms ease-out;
}

/* Delivery-status indicator next to outgoing bubbles. Only renders
 * for from_me rows; the icon swaps via #[watch] in `factory.rs`,
 * the colour comes from these classes. Keeping the read-receipt
 * blue close to WhatsApp's spec (#34b7f1) so the visual mapping is
 * obvious at a glance. */
.tina-delivery-status {
    margin-left: 6px;
    -gtk-icon-size: 14px;
    transition: color 200ms ease-out;
}
.tina-status-pending {
    color: alpha(@theme_fg_color, 0.45);
}
.tina-status-sent {
    color: alpha(@theme_fg_color, 0.55);
}
.tina-status-delivered {
    color: alpha(@theme_fg_color, 0.65);
}
.tina-status-read {
    color: #34b7f1;
}
.tina-status-failed {
    color: @error_color;
}
"#;
