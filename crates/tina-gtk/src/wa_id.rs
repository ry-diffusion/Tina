// Re-export of the typed JID module that lives in tina-core. Kept
// at this path so widgets keep importing `crate::wa_id::WaIdentity`
// while the canonical definition crosses the IPC boundary in
// tina-core. Display helpers that need GTK-only crates (phone
// formatting via the `phonenumber` crate) layer on top.

pub use tina_core::{WaContact, WaIdentity};

/// Phone-aware display: same fallback chain as `WaIdentity::display_short`
/// but routes Phone variants through `format::format_jid_or_phone` so
/// users see `+55 61 9…` rather than `5561…`.
pub fn display(id: &WaIdentity) -> String {
    match id {
        WaIdentity::Phone(_) | WaIdentity::Hosted(_) => {
            crate::format::format_jid_or_phone(id.user())
        }
        _ => id.display_short(),
    }
}

/// Phone-aware contact display. Falls through to the explicit
/// display_name → phone display → primary's `display`.
pub fn display_contact(c: &WaContact) -> String {
    if let Some(n) = c.display_name.as_deref() {
        return n.to_string();
    }
    if let Some(jid) = c.phone_jid() {
        return display(jid);
    }
    display(&c.primary)
}
