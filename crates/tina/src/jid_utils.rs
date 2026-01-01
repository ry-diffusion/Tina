/// Utilities for parsing and formatting WhatsApp JIDs
use slint::SharedString;

/// Format a JID for display in the UI
/// - If it's a phone number JID (e.g., "5511999999999@s.whatsapp.net"), format the phone number
/// - If it only has LID, show "(LID)"
/// - Otherwise, show the raw JID
pub fn format_jid_for_display(jid: &str) -> SharedString {
    // Check if it's a user JID format (phone@s.whatsapp.net)
    if jid.contains("@s.whatsapp.net") {
        if let Some(phone) = jid.split('@').next() {
            // Check if it's a phone number (all digits)
            if phone.chars().all(|c| c.is_ascii_digit()) && !phone.is_empty() {
                return SharedString::from(format_phone_number(phone));
            }
        }
    }

    // Check if it's a group JID
    if jid.contains("@g.us") {
        // For groups, we'll rely on the group subject being set
        return SharedString::from(jid);
    }

    // Check if it's a LID format (starts with 2: or has specific LID pattern)
    if jid.starts_with("2:") || (!jid.contains('@') && jid.contains(':')) {
        return SharedString::from("(LID)");
    }

    // Check if it's a user LID (contains @lid)
    if jid.contains("@lid") {
        return SharedString::from("(U)");
    }

    // Default: return the JID as-is
    SharedString::from(jid)
}

/// Format a phone number string for better readability
/// Example: "5511999999999" -> "+55 11 99999-9999"
fn format_phone_number(phone: &str) -> String {
    if phone.is_empty() {
        return phone.to_string();
    }

    // Try to format Brazilian phone numbers
    if phone.starts_with("55") && phone.len() >= 12 {
        // Format: +55 11 99999-9999 or +55 11 9999-9999
        let country = &phone[0..2];
        let area = &phone[2..4];
        let rest = &phone[4..];

        if rest.len() == 9 {
            // Mobile with 9 digits
            let part1 = &rest[0..5];
            let part2 = &rest[5..];
            return format!("+{} {} {}-{}", country, area, part1, part2);
        } else if rest.len() == 8 {
            // Landline with 8 digits
            let part1 = &rest[0..4];
            let part2 = &rest[4..];
            return format!("+{} {} {}-{}", country, area, part1, part2);
        }
    }

    // For other countries or if formatting fails, just add + prefix if it looks like an international number
    if phone.len() > 10 {
        return format!("+{}", phone);
    }

    phone.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_brazilian_mobile() {
        assert_eq!(format_phone_number("5511999999999"), "+55 11 99999-9999");
    }

    #[test]
    fn test_format_brazilian_landline() {
        assert_eq!(format_phone_number("551133334444"), "+55 11 3333-4444");
    }

    #[test]
    fn test_format_jid_phone() {
        let result = format_jid_for_display("5511999999999@s.whatsapp.net");
        assert_eq!(result.as_str(), "+55 11 99999-9999");
    }

    #[test]
    fn test_format_jid_lid() {
        let result = format_jid_for_display("2:abc123");
        assert_eq!(result.as_str(), "(LID)");
    }

    #[test]
    fn test_format_jid_user_lid() {
        let result = format_jid_for_display("abc123@lid");
        assert_eq!(result.as_str(), "(U)");
    }
}
