// Typed WhatsApp identifier. Wraps the raw `<user>@<server>` JID
// strings whatsmeow hands us so the rest of the workspace never has
// to reason about "is this a phone, a LID, a channel?" via string
// matching. Lives in `tina-core` because it crosses the IPC wire —
// every `chat_jid` / `sender_jid` field on an event or command is a
// `WaIdentity` once it lands in Rust, and serde round-trips it
// through the original raw form so the Go side stays untouched.
//
// References:
//   * `types.JID` in whatsmeow (`go.mau.fi/whatsmeow/types/jid.go`)
//   * `tulir/whatsmeow` deepwiki — `parseMessageSource` treats
//     newsletter senders specially (chat == sender for channels).

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WaIdentity {
    /// E.164 phone-rooted JID (`<digits>@s.whatsapp.net`). Display
    /// runs the user-part through E.164 formatting in the GTK
    /// crate; here we just carry the raw string.
    Phone(String),
    /// Linked-identity JID (`<digits>@lid`). Used for users who
    /// haven't shared their phone with us.
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

    /// True for `WaIdentity` variants whatsmeow exposes a metadata
    /// endpoint for (`GetNewsletterInfo` / `GetGroupInfo`). The UI
    /// uses this to decide whether `Cmd::RefreshChat` makes sense.
    pub fn needs_metadata_refresh(&self) -> bool {
        matches!(self, WaIdentity::Newsletter(_) | WaIdentity::Group(_))
    }

    /// True when the parsed identity is a recognised JID (anything
    /// other than `Unknown`). Use as a guard for "is this string a
    /// raw JID we shouldn't be showing as a name?".
    pub fn is_known(&self) -> bool {
        !matches!(self, WaIdentity::Unknown(_))
    }

    /// Treat a `name` candidate as "still unresolved" so the caller
    /// can decide to fire a refresh. Empty strings and raw JIDs
    /// (parsed to a non-`Unknown` identity) both qualify.
    pub fn looks_like_unresolved_name(name: &str) -> bool {
        let trimmed = name.trim();
        trimmed.is_empty() || WaIdentity::parse(trimmed).is_known()
    }

    /// Sentinel for "the wire string was empty" — parses to
    /// `Unknown("")`. Used by message ingestion when `sender_jid`
    /// is missing (from_me rows have no sender).
    pub fn is_empty_unknown(&self) -> bool {
        matches!(self, WaIdentity::Unknown(s) if s.is_empty())
    }

    /// Generic display label that doesn't depend on phone-formatting
    /// helpers (kept here in tina-core). Phones come back as the raw
    /// digit prefix (UI crate may still re-format them with the
    /// phonenumber crate); LIDs / channels render with a short hash
    /// suffix so the user never sees the full opaque ID.
    ///
    /// `tina-gtk` augments this for phones via its `format` module —
    /// see `tina-gtk::format::format_jid_or_phone`.
    pub fn display_short(&self) -> String {
        match self {
            WaIdentity::Phone(_) | WaIdentity::Hosted(_) => self.user().to_string(),
            WaIdentity::Lid(_) => format!("lid:{}", short_id(self.user())),
            WaIdentity::Group(_) => format!("Group #{}", short_id(self.user())),
            WaIdentity::Newsletter(_) => format!("Channel #{}", short_id(self.user())),
            WaIdentity::Broadcast(_) => "Broadcast list".to_string(),
            WaIdentity::Status => "Status".to_string(),
            WaIdentity::Unknown(s) => s.clone(),
        }
    }
}

impl fmt::Display for WaIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.raw())
    }
}

impl Serialize for WaIdentity {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.raw())
    }
}

impl<'de> Deserialize<'de> for WaIdentity {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(WaIdentity::parse(&s))
    }
}

/// A WhatsApp participant the user can talk to: the primary JID
/// (whatever form the protocol gave us first) plus an optional alt
/// JID (the linked phone-number or LID counterpart) plus a resolved
/// display name. `WaIdentity` alone tags a single string;
/// `WaContact` tags a *person* who's reachable through more than one
/// identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaContact {
    pub primary: WaIdentity,
    pub alt: Option<WaIdentity>,
    pub display_name: Option<String>,
}

impl WaContact {
    pub fn from_jid(primary: WaIdentity) -> Self {
        Self {
            primary,
            alt: None,
            display_name: None,
        }
    }

    pub fn with_alt(mut self, alt: WaIdentity) -> Self {
        if alt != self.primary {
            self.alt = Some(alt);
        }
        self
    }

    pub fn with_display(mut self, name: impl Into<String>) -> Self {
        let s = name.into();
        if !s.is_empty() {
            self.display_name = Some(s);
        }
        self
    }

    /// Best-effort phone form. Looks at `primary` first, falls back
    /// to `alt`. Used by avatar fetches that only work against
    /// `s.whatsapp.net` JIDs.
    pub fn phone_jid(&self) -> Option<&WaIdentity> {
        if matches!(self.primary, WaIdentity::Phone(_) | WaIdentity::Hosted(_)) {
            return Some(&self.primary);
        }
        match self.alt.as_ref() {
            Some(jid) if matches!(jid, WaIdentity::Phone(_) | WaIdentity::Hosted(_)) => Some(jid),
            _ => None,
        }
    }

    pub fn lid_jid(&self) -> Option<&WaIdentity> {
        if matches!(self.primary, WaIdentity::Lid(_)) {
            return Some(&self.primary);
        }
        match self.alt.as_ref() {
            Some(jid) if matches!(jid, WaIdentity::Lid(_)) => Some(jid),
            _ => None,
        }
    }

    /// What to show in the UI. Falls back through the resolution
    /// chain: explicit display name → phone display → primary's
    /// own `display_short()`. Never returns the raw JID.
    pub fn display(&self) -> String {
        if let Some(n) = self.display_name.as_deref() {
            return n.to_string();
        }
        if let Some(jid) = self.phone_jid() {
            return jid.display_short();
        }
        self.primary.display_short()
    }
}

fn short_id(user: &str) -> String {
    if user.len() <= 8 {
        return user.to_string();
    }
    user.chars()
        .rev()
        .take(6)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
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
        assert!(matches!(WaIdentity::parse("123-456@g.us"), WaIdentity::Group(_)));
        assert!(matches!(
            WaIdentity::parse("status@broadcast"),
            WaIdentity::Status
        ));
        assert!(matches!(WaIdentity::parse("garbage"), WaIdentity::Unknown(_)));
    }

    #[test]
    fn raw_round_trips() {
        for raw in [
            "5561@s.whatsapp.net",
            "220280@lid",
            "120363@newsletter",
            "123@g.us",
            "status@broadcast",
            "x@hosted",
            "weird",
        ] {
            assert_eq!(WaIdentity::parse(raw).raw(), raw);
        }
    }

    #[test]
    fn serde_round_trips_as_string() {
        let id = WaIdentity::parse("120363@newsletter");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, r#""120363@newsletter""#);
        let back: WaIdentity = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn refresh_eligibility() {
        assert!(WaIdentity::parse("120363@newsletter").needs_metadata_refresh());
        assert!(WaIdentity::parse("123-456@g.us").needs_metadata_refresh());
        assert!(!WaIdentity::parse("5561@s.whatsapp.net").needs_metadata_refresh());
        assert!(!WaIdentity::parse("status@broadcast").needs_metadata_refresh());
    }

    #[test]
    fn unresolved_name_predicate() {
        assert!(WaIdentity::looks_like_unresolved_name(""));
        assert!(WaIdentity::looks_like_unresolved_name("   "));
        assert!(WaIdentity::looks_like_unresolved_name(
            "120363194378500802@newsletter"
        ));
        assert!(!WaIdentity::looks_like_unresolved_name("Canaltech HQ"));
    }

    #[test]
    fn contact_resolves_phone_alt() {
        let primary = WaIdentity::parse("220280752451716@lid");
        let alt = WaIdentity::parse("556196862399@s.whatsapp.net");
        let c = WaContact::from_jid(primary)
            .with_alt(alt.clone())
            .with_display("Moizes");
        assert_eq!(c.display(), "Moizes");
        assert_eq!(c.phone_jid(), Some(&alt));
    }

    #[test]
    fn contact_dedupes_alt_equal_primary() {
        let same = WaIdentity::parse("5561@s.whatsapp.net");
        let c = WaContact::from_jid(same.clone()).with_alt(same);
        assert!(c.alt.is_none());
    }
}
