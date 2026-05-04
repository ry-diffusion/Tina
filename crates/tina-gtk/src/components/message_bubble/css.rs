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
"#;
