// CSS shipped alongside the bubble factory. Loaded once at app start via
// `relm4::set_global_css`.
//
// Visual language ported from Fractal's `_room_history.scss` and
// `_session_view.scss`: rows have soft hover/selection backgrounds with
// rounded corners (matching the menu radius), the day-divider is a
// dim, bold pill flanked by content margins, and the cozy/collapsed
// avatar gutter aligns with the rest of the timeline. Reply quotes
// are rendered as the Fractal `%nested-effect` (left-accent border +
// reduced opacity).

pub const MESSAGE_ROW_CSS: &str = r#"
/* ── Per-row container ──────────────────────────────────────────── */
.message-row {
    padding: 0;
    /* Fractal's `.room-history-row` uses 8px horizontal + 2px vertical
     * padding inside a row that owns the `border-radius`. The hover
     * affordance is the row background, not an inner element. */
}
.message-row .message-box {
    margin-left: 6px;
    margin-right: 6px;
    padding: 4px 8px;
    border-radius: 9px;
    transition: linear 120ms background-color;
}
.message-row:hover .message-box,
.message-row:focus-within .message-box {
    background-color: alpha(@theme_fg_color, 0.06);
    transition: none;
}
.message-row.has-open-popup .message-box {
    background-color: alpha(@theme_fg_color, 0.10);
}
/* Cozy: first message in a sender's run — full padding so the avatar
 * column has room to breathe. Collapsed: zero top padding so a run
 * reads as one block. Mirrors Fractal's `.has-avatar` margin-top. */
.message-cozy {
    margin-top: 4px;
}
.message-collapsed {
    margin-top: 0;
}
.message-cozy-header {
    min-height: 1.4em;
}
.message-cozy-avatar {
    margin-left: 4px;
    margin-right: 12px;
}

/* ── Day divider pill ───────────────────────────────────────────── */
/* Fractal's `divider-row` is a bold, dim caption that sits centered
 * between two flanking separators. We render the same shape with a
 * single label inside a centered box; a future iteration can add the
 * separator lines via `gtk::Separator` siblings if visual parity is
 * desired down the line. */
.message-day-divider {
    margin: 14px 24px 6px 24px;
}
.message-day-divider-label {
    font-size: 0.85em;
    font-weight: 600;
    opacity: 0.55;
    padding: 2px 12px;
    border-radius: 9999px;
    background-color: alpha(@theme_fg_color, 0.06);
}

/* ── Hover-only timestamp on collapsed runs ─────────────────────── */
.message-collapsed-timestamp {
    opacity: 0;
    font-size: 0.7em;
    color: alpha(@theme_fg_color, 0.7);
    transition: opacity 120ms ease-out;
}
.message-row:hover .message-collapsed-timestamp,
.message-row:focus-within .message-collapsed-timestamp {
    opacity: 1;
}

/* ── Content ────────────────────────────────────────────────────── */
.message-content {
    margin-top: 1px;
    margin-bottom: 1px;
}
.message-picture {
    border-radius: 9px;
}

/* Blurhash-style placeholder: the proto thumbnail is tiny (often
 * <150 px on the long axis), blown up via ContentFit::Cover. The
 * blur softens the pixelation into a preview matching WhatsApp. */
.message-thumbnail-blur {
    filter: blur(12px);
}

.message-sticker {
    min-width: 128px;
    min-height: 128px;
}

/* ── Reply (nested-effect from Fractal) ─────────────────────────── */
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
    border-left: 2px solid @accent_color;
    padding: 2px 8px;
    border-radius: 3px;
    font-size: 0.9em;
    opacity: 0.85;
}
.message-reply-author {
    font-weight: 600;
    color: @accent_color;
}
.message-reply-preview {
    color: alpha(@theme_fg_color, 0.7);
}

/* ── Jump highlight (transient when navigating to a quoted message) */
.message-jump-highlight {
    background-color: alpha(@accent_color, 0.18);
    transition: background-color 600ms ease-out;
}

/* ── Delivery status (from_me only) ─────────────────────────────── */
/* WhatsApp-style check marks. Read-receipt blue close to spec
 * (#34b7f1) so the visual mapping is obvious at a glance. */
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
