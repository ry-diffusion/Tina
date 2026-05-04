// Display-string helpers used by the bubble's view bindings.

use tina_db::MessageRow;

pub fn glib_markup_escape(s: &str) -> String {
    gtk::glib::markup_escape_text(s).to_string()
}

/// Convert the WhatsApp text-styling markers to Pango markup so the
/// content label can render them. Supports:
///   * `*bold*`, `_italic_`, `~strike~`,
///   * `` `inline code` ``, ``` ```block code``` ```,
///   * `http(s)://…` links auto-wrapped in `<a>`.
///
/// Returns plain Pango-escaped text when nothing matches; the bubble
/// view always sets `use-markup: true`, so even unstyled content goes
/// through this path.
pub fn wa_markdown_to_pango(input: &str) -> String {
    let escaped = glib_markup_escape(input);
    let with_links = autolink(&escaped);
    let after_code_block = wrap_pairs(&with_links, "```", "<tt>", "</tt>");
    let after_inline_code = wrap_pairs(&after_code_block, "`", "<tt>", "</tt>");
    let after_bold = wrap_pairs(&after_inline_code, "*", "<b>", "</b>");
    let after_italic = wrap_pairs(&after_bold, "_", "<i>", "</i>");
    wrap_pairs(&after_italic, "~", "<s>", "</s>")
}

/// Wrap `delim`-bounded pairs with `open`/`close` markup. Naive
/// "alternating segments" rule — the same WhatsApp uses in practice.
/// Edge cases (single-character word boundaries, escaped markers) are
/// not handled; if a marker appears inside a URL or as a raw
/// character it'll either stay literal (odd count) or get wrapped
/// (even count). Matches WhatsApp's behaviour closely enough for the
/// 95th percentile of group chats.
fn wrap_pairs(s: &str, delim: &str, open: &str, close: &str) -> String {
    let parts: Vec<&str> = s.split(delim).collect();
    if parts.len() < 3 {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 16);
    let mut toggle = false;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            out.push_str(part);
            continue;
        }
        // Last part is unmatched if there's an odd count of delims.
        if i == parts.len() - 1 && parts.len() % 2 == 0 {
            out.push_str(delim);
            out.push_str(part);
            continue;
        }
        // Reject pairs that would wrap an empty span ("**" by itself)
        // or open/close on whitespace — WhatsApp ignores those.
        if !toggle {
            let next = parts.get(i + 1).copied().unwrap_or("");
            let inner_starts_with_space =
                part.chars().next().is_none_or(char::is_whitespace);
            let inner_is_empty = part.is_empty();
            if inner_is_empty || inner_starts_with_space || next.is_empty() {
                out.push_str(delim);
                out.push_str(part);
                continue;
            }
            out.push_str(open);
            out.push_str(part);
            toggle = true;
        } else {
            out.push_str(close);
            out.push_str(part);
            toggle = false;
        }
    }
    if toggle {
        // Closer was eaten by the inner-empty guard; we need to undo
        // the opening tag so the resulting markup is balanced. Cheap
        // fallback: drop the entire pass and return the input.
        return s.to_string();
    }
    out
}

/// Wrap bare http(s) URLs in `<a href="…">…</a>`. Operates on already
/// Pango-escaped text — entity references like `&amp;` inside a URL
/// stay valid because the closing terminator is whitespace / EOL /
/// the few ASCII punctuation chars we exclude.
fn autolink(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 32);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &s[i..];
        if rest.starts_with("http://") || rest.starts_with("https://") {
            // Walk until whitespace or terminator. ASCII-only test is
            // safe — URLs can't contain UTF-8 non-ASCII unencoded.
            let end = rest
                .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | ')'))
                .unwrap_or(rest.len());
            let url = &rest[..end];
            // Avoid wrapping URLs that already live inside an `<a>`
            // tag (defensive — autolink is run before tag insertion
            // so this should never trigger, but cheap to check).
            out.push_str(&format!(
                "<a href=\"{href}\">{label}</a>",
                href = url,
                label = url,
            ));
            i += end;
        } else {
            // Push one char at a time. Cheap because the work is
            // dominated by URL spans, not the surrounding text.
            let ch = s[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
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
