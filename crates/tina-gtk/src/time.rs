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
