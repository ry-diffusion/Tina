// Display-string helpers used by the bubble's view bindings.

use tina_db::MessageRow;

pub fn glib_markup_escape(s: &str) -> String {
    gtk::glib::markup_escape_text(s).to_string()
}

pub fn build_media_summary(row: &MessageRow) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let (Some(w), Some(h)) = (row.media_width, row.media_height)
        && w > 0 && h > 0 {
            parts.push(format!("{w}×{h}"));
        }
    if let Some(secs) = row.media_duration_secs
        && secs > 0 {
            parts.push(format!("{}:{:02}", secs / 60, secs % 60));
        }
    if let Some(bytes) = row.media_size_bytes
        && bytes > 0 {
            parts.push(format_size(bytes));
        }
    if let Some(name) = row.media_filename.as_deref()
        && !name.is_empty() {
            parts.push(name.to_string());
        }
    parts.join(" · ")
}

pub fn format_size(bytes: i64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
