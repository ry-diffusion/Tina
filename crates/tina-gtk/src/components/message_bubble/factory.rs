// Delivery-status helpers shared between the (now-virtualised) chat
// row factory in `message_row` and any future renderer. Kept in this
// module because `MessageItem` (the data model) lives next door —
// it's the natural home for status mapping that depends on whatever
// the wire protocol calls each step.

/// Maps wire-level status strings to symbolic icons. Names match
/// what `build.rs` bundles via relm4-icons-build — we don't fall
/// back on the host icon theme so the bubble looks identical on
/// minimal Adwaita / Breeze / etc. installations.
pub fn delivery_icon_name(status: &str) -> &'static str {
    match status {
        // Spinner-style icon while we're waiting for server ack.
        "pending" => "clock-loader-40-symbolic",
        // Server ack but peer device hasn't received yet — single
        // check (WhatsApp's "✓").
        "sent" | "server_ack" => "check-symbolic",
        // Peer device got it — double check ("✓✓"). Same icon as
        // `read`; the difference shows up via the CSS color class
        // (`tina-status-read` paints the read variant blue).
        "delivered" => "done-all-symbolic",
        // Peer opened the chat / pressed play — blue ✓✓.
        "read" | "played" => "done-all-symbolic",
        "failed" => "warning-symbolic",
        _ => "clock-loader-40-symbolic",
    }
}

pub fn delivery_css_class(status: &str) -> &'static str {
    match status {
        "read" | "played" => "tina-status-read",
        "delivered" => "tina-status-delivered",
        "pending" => "tina-status-pending",
        "failed" => "tina-status-failed",
        _ => "tina-status-sent",
    }
}
