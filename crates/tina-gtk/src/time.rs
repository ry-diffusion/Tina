// Timestamp formatting helpers, ported from the Slint frontend so display
// matches between the two during the cutover.
use chrono::{DateTime, Datelike, Local, Utc};

pub fn format_chat_timestamp(timestamp: i64) -> String {
    if timestamp <= 0 {
        return String::new();
    }
    let dt = DateTime::<Utc>::from_timestamp(timestamp, 0).unwrap_or_else(Utc::now);
    let local: DateTime<Local> = dt.into();
    let now = Local::now();
    if local.date_naive() == now.date_naive() {
        local.format("%H:%M").to_string()
    } else if local.year() == now.year() {
        local.format("%d/%m").to_string()
    } else {
        local.format("%d/%m/%y").to_string()
    }
}

/// `HH:MM` only — used by collapsed rows' hover-timestamp gutter,
/// which has 56px to play with. The full-date variants are too wide
/// for that gutter (`04/05 22:20` doesn't fit), and the date is
/// already conveyed by the day-divider pill above the run.
pub fn format_short_time(timestamp: i64) -> String {
    if timestamp <= 0 {
        return String::new();
    }
    let dt = DateTime::<Utc>::from_timestamp(timestamp, 0).unwrap_or_else(Utc::now);
    let local: DateTime<Local> = dt.into();
    local.format("%H:%M").to_string()
}

/// Date label shown by the chat-thread day divider when the day flips
/// between two consecutive messages. Mirrors WhatsApp / Fractal's
/// "Today / Yesterday / weekday / full date" cascade so the user gets
/// the loosest pretty form available.
pub fn format_day_divider(timestamp: i64) -> String {
    if timestamp <= 0 {
        return String::new();
    }
    let dt = DateTime::<Utc>::from_timestamp(timestamp, 0).unwrap_or_else(Utc::now);
    let local: DateTime<Local> = dt.into();
    let now = Local::now();
    let today = now.date_naive();
    let day = local.date_naive();
    if day == today {
        "Today".to_string()
    } else if day == today.pred_opt().unwrap_or(today) {
        "Yesterday".to_string()
    } else if (today - day).num_days() < 7 && day < today {
        local.format("%A").to_string()
    } else if local.year() == now.year() {
        local.format("%A, %d %B").to_string()
    } else {
        local.format("%d %B %Y").to_string()
    }
}

/// Local date for grouping/divider decisions. Returns a stable key
/// (`YYYY-MM-DD`) so callers can compare two timestamps for "are these
/// in the same local day" without dragging chrono types around.
pub fn local_day_key(timestamp: i64) -> String {
    if timestamp <= 0 {
        return String::new();
    }
    let dt = DateTime::<Utc>::from_timestamp(timestamp, 0).unwrap_or_else(Utc::now);
    let local: DateTime<Local> = dt.into();
    local.format("%Y-%m-%d").to_string()
}

pub fn format_message_time(timestamp: i64) -> String {
    if timestamp <= 0 {
        return String::new();
    }
    let dt = DateTime::<Utc>::from_timestamp(timestamp, 0).unwrap_or_else(Utc::now);
    let local: DateTime<Local> = dt.into();
    let now = Local::now();
    if local.date_naive() == now.date_naive() {
        local.format("%H:%M").to_string()
    } else if local.year() == now.year() {
        local.format("%d/%m %H:%M").to_string()
    } else {
        local.format("%d/%m/%y %H:%M").to_string()
    }
}
