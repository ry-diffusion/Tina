pub fn format_jid_or_phone(jid_or_phone: &str) -> String {
    // If it's a JID, extract the phone number part
    let num_str = jid_or_phone.split('@').next().unwrap_or(jid_or_phone);
    let base_num = num_str.split(':').next().unwrap_or(num_str);

    // Some JIDs are just IDs, some are phone numbers. We add a '+' to make it parseable
    let num_with_plus = if !base_num.starts_with('+') {
        format!("+{}", base_num)
    } else {
        base_num.to_string()
    };

    if let Ok(phone) = phonenumber::parse(None, &num_with_plus) {
        // If it's an older 8-digit Brazilian mobile number, phonenumber crate might consider it invalid
        // It will be 13 characters long including the '+' e.g. +556196862399
        if !phone.is_valid() && num_with_plus.starts_with("+55") && num_with_plus.len() == 13 {
            let (prefix, suffix) = num_with_plus.split_at(5);
            let with_9 = format!("{}9{}", prefix, suffix);
            if let Ok(phone_with_9) = phonenumber::parse(None, &with_9) {
                return phone_with_9
                    .format()
                    .mode(phonenumber::Mode::International)
                    .to_string();
            }
        }
        phone
            .format()
            .mode(phonenumber::Mode::International)
            .to_string()
    } else {
        base_num.to_string()
    }
}

pub fn base_jid(jid: &str) -> String {
    let mut parts = jid.split('@');
    let user_part = parts.next().unwrap_or(jid);
    let domain = parts.next().unwrap_or("s.whatsapp.net");
    let base_user = user_part.split(':').next().unwrap_or(user_part);
    format!("{}@{}", base_user, domain)
}
