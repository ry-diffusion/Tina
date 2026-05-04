// Typed WhatsApp identifier. Wraps the raw `<user>@<server>` JID
// strings whatsmeow hands us so the UI never has to reason about
// "is this a phone, a LID, a channel?" via string matching. Display
// strings are formatted on read; the raw form is preserved for
// equality and for round-tripping back through IPC.
//
// References:
//   * `types.JID` in whatsmeow (`go.mau.fi/whatsmeow/types/jid.go`)
//   * `tulir/whatsmeow` deepwiki — `parseMessageSource` treats
//     newsletter senders specially (chat == sender for channels).
//
// Coverage matches `tina-db::ChatKind::infer_from_jid` 1:1 — when
// you add a server here, mirror it there.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WaIdentity {
    /// E.164 phone-rooted JID (`<digits>@s.whatsapp.net`). The display
    /// form runs the user-part through `format_jid_or_phone` so
    /// `5561…` becomes `+55 61 …`.
    Phone(String),
    /// Linked-identity JID (`<digits>@lid`). Used for users who
    /// haven't shared their phone with us; the display form is the
    /// raw LID prefixed with `lid:` so it's obviously a non-phone
    /// identifier.
    Lid(String),
    /// Group chat (`<digits>@g.us`).
    Group(String),
    /// Newsletter / channel (`<digits>@newsletter`).
    Newsletter(String),
    /// `status@broadcast` — the special pseudo-chat aggregating
    /// everyone's status posts.
    Status,
    /// Generic broadcast list (`<digits>@broadcast` without
    /// `status@` prefix).
    Broadcast(String),
    /// Business / hosted server (`<digits>@hosted`).
    Hosted(String),
    /// Server we don't recognise. Carries the raw string so the
    /// caller can still log / round-trip it.
    Unknown(String),
}

impl WaIdentity {
    /// Parse from the raw `<user>@<server>` form.
    pub fn parse(raw: &str) -> Self {
        if raw == "status@broadcast" {
            return WaIdentity::Status;
        }
        let Some((_user, server)) = raw.rsplit_once('@') else {
            return WaIdentity::Unknown(raw.to_string());
        };
        match server {
            "s.whatsapp.net" | "c.us" => WaIdentity::Phone(raw.to_string()),
            "lid" => WaIdentity::Lid(raw.to_string()),
            "g.us" => WaIdentity::Group(raw.to_string()),
            "newsletter" => WaIdentity::Newsletter(raw.to_string()),
            "broadcast" => WaIdentity::Broadcast(raw.to_string()),
            "hosted" => WaIdentity::Hosted(raw.to_string()),
            _ => WaIdentity::Unknown(raw.to_string()),
        }
    }

    /// Server-side `kind` string matching `tina_db::ChatKind`.
    /// `dm` covers both Phone and Lid because DMs in tina.db are
    /// stored with a single kind regardless of which identifier
    /// type the user is reachable by.
    pub fn kind(&self) -> &'static str {
        match self {
            WaIdentity::Phone(_) | WaIdentity::Lid(_) => "dm",
            WaIdentity::Group(_) => "group",
            WaIdentity::Newsletter(_) => "newsletter",
            WaIdentity::Status => "status",
            WaIdentity::Broadcast(_) => "broadcast",
            WaIdentity::Hosted(_) => "dm",
            WaIdentity::Unknown(_) => "unknown",
        }
    }

    /// The original `<user>@<server>` string. Use this when
    /// round-tripping through IPC or comparing against keys stored
    /// in the database.
    pub fn raw(&self) -> &str {
        match self {
            WaIdentity::Phone(s)
            | WaIdentity::Lid(s)
            | WaIdentity::Group(s)
            | WaIdentity::Newsletter(s)
            | WaIdentity::Broadcast(s)
            | WaIdentity::Hosted(s)
            | WaIdentity::Unknown(s) => s,
            WaIdentity::Status => "status@broadcast",
        }
    }

    /// Just the `<user>` half of the JID, before the `@server`.
    /// Useful for matching against contact short-IDs.
    pub fn user(&self) -> &str {
        match self {
            WaIdentity::Status => "status",
            other => other
                .raw()
                .rsplit_once('@')
                .map(|(u, _)| u)
                .unwrap_or(other.raw()),
        }
    }

    /// Human-readable label. Phones render formatted; LIDs and
    /// channels fall back to readable shortened forms so the user
    /// never sees `120363…@newsletter` blasted into a name field.
    pub fn display(&self) -> String {
        match self {
            WaIdentity::Phone(_) => crate::format::format_jid_or_phone(self.user()),
            WaIdentity::Lid(_) => format!("lid:{}", short_id(self.user())),
            WaIdentity::Group(_) => format!("Group #{}", short_id(self.user())),
            WaIdentity::Newsletter(_) => format!("Channel #{}", short_id(self.user())),
            WaIdentity::Broadcast(_) => "Broadcast list".to_string(),
            WaIdentity::Status => "Status".to_string(),
            WaIdentity::Hosted(_) => crate::format::format_jid_or_phone(self.user()),
            WaIdentity::Unknown(s) => s.clone(),
        }
    }
}

impl fmt::Display for WaIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display())
    }
}

/// Take the trailing 6 chars of a long numeric ID. Group/newsletter
/// JIDs are 18 digits which doesn't fit anywhere; the last six are
/// distinctive enough to differentiate channels in passing.
fn short_id(user: &str) -> String {
    if user.len() <= 8 {
        return user.to_string();
    }
    let tail: String = user.chars().rev().take(6).collect::<String>().chars().rev().collect();
    tail
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_servers() {
        assert!(matches!(
            WaIdentity::parse("5561999999999@s.whatsapp.net"),
            WaIdentity::Phone(_)
        ));
        assert!(matches!(
            WaIdentity::parse("220280752451716@lid"),
            WaIdentity::Lid(_)
        ));
        assert!(matches!(
            WaIdentity::parse("120363194378500802@newsletter"),
            WaIdentity::Newsletter(_)
        ));
        assert!(matches!(
            WaIdentity::parse("123-456@g.us"),
            WaIdentity::Group(_)
        ));
        assert!(matches!(WaIdentity::parse("status@broadcast"), WaIdentity::Status));
        assert!(matches!(
            WaIdentity::parse("garbage"),
            WaIdentity::Unknown(_)
        ));
    }

    #[test]
    fn newsletter_display_uses_short_id() {
        let id = WaIdentity::parse("120363194378500802@newsletter");
        assert_eq!(id.display(), "Channel #500802");
    }

    #[test]
    fn lid_display_marks_kind() {
        let id = WaIdentity::parse("220280752451716@lid");
        assert_eq!(id.display(), "lid:451716");
    }

    #[test]
    fn raw_round_trips() {
        let raw = "120363194378500802@newsletter";
        assert_eq!(WaIdentity::parse(raw).raw(), raw);
    }
}
