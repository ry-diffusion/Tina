// `@`-mention autocomplete popover for the chat composer.
//
// Inspired by fractal's `CompletionPopover` (matrix client) and
// dissent's `chatkit/components/autocomplete`. We don't have a
// chatkit equivalent in Rust, so this owns its filter / key nav /
// row rendering by hand. The widget is intentionally not a relm4
// component — it's a small wrapper around a `gtk::Popover` that
// the `ChatTab` constructs once per tab and feeds a candidate list
// through.
//
// Wiring (see `chat_tab::component`):
//   - Anchored to the composer `gtk::Entry`.
//   - Watches `notify::text` and `notify::cursor-position` on the
//     entry's `EntryBuffer` to find the `@<query>` token at the
//     cursor.
//   - Keyboard: ↑/↓ navigate, Enter/Tab insert, Escape inhibits
//     the popover for the current word.
//   - Selection replaces the `@<query>` substring with `@<digits> `
//     in the entry buffer and pushes the JID up the channel so
//     `ChatTab` can record it on `pending_mentions`.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use relm4::Sender;
use tina_db::MentionCandidate;

use crate::inventory::AvatarInventory;

use super::chat_tab::ChatTabInput;

const MAX_VISIBLE_ROWS: usize = 8;

#[derive(Clone)]
pub struct MentionPopover {
    inner: Rc<Inner>,
}

struct Inner {
    popover: gtk::Popover,
    list: gtk::ListBox,
    /// Entry the popover autocompletes for. Held weakly through
    /// `popover.set_parent` already; we keep a clone here for the
    /// buffer-rewrite path on row activation.
    entry: gtk::Entry,
    /// Avatar inventory shared with the rest of the GTK tree —
    /// the popover renders the same cached textures the bubbles
    /// use, so rows don't trigger fresh fetches.
    avatars: AvatarInventory,
    /// Live candidate list. Replaced wholesale on each
    /// `MentionCandidatesLoaded` from the worker.
    candidates: RefCell<Vec<MentionCandidate>>,
    /// Channel back to the owning `ChatTab` — we send a
    /// `MentionInserted { jid }` after rewriting the buffer so the
    /// tab can stash the JID in `pending_mentions`.
    sender: Sender<ChatTabInput>,
    /// Current `@<query>` window: `(start_byte, end_byte)` in the
    /// entry's text. `None` when the popover is hidden.
    current_word: RefCell<Option<(usize, usize)>>,
    /// User pressed Esc on the current word — keep the popover
    /// suppressed until the cursor leaves the `@<…>` token.
    inhibit: RefCell<bool>,
}

impl MentionPopover {
    pub fn new(
        entry: &gtk::Entry,
        avatars: AvatarInventory,
        sender: Sender<ChatTabInput>,
    ) -> Self {
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Browse)
            .build();
        list.add_css_class("mention-popover");
        // The popover doesn't own keyboard focus — the entry
        // does. If we let the ListBox/Popover steal focus on
        // popup, the user's next keystroke goes nowhere visible
        // (the entry stops receiving characters). Disabling
        // can-focus on both keeps the cursor in the entry while
        // still showing a selection highlight.
        list.set_can_focus(false);

        let scrolled = gtk::ScrolledWindow::builder()
            .min_content_width(320)
            .max_content_width(420)
            .min_content_height(60)
            .max_content_height(280)
            // Without `propagate_natural_width`, the ScrolledWindow
            // reports `min_content_width` (320) as its natural
            // width. That's still bigger than what the popover was
            // ending up with — GTK seems to choose the smaller of
            // the natural request and some entry-anchor heuristic.
            // Forcing it on lets the row's true width drive the
            // popover; combined with `width_request` below we get a
            // hard floor.
            .propagate_natural_width(true)
            .propagate_natural_height(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&list)
            .build();
        // Hard floor on width — the popover's auto-sizing under an
        // anchored Entry was collapsing rows down to "avatar … …"
        // because something in the chain (likely the popover itself
        // when pointing-to has a 1px rect) was treating the natural
        // width as the entry's allocation. `width_request` is a
        // strict minimum the layout cannot ignore.
        scrolled.set_width_request(360);

        let popover = gtk::Popover::builder()
            .autohide(false)
            .has_arrow(false)
            .position(gtk::PositionType::Top)
            .child(&scrolled)
            .build();
        popover.set_can_focus(false);
        popover.set_parent(entry);
        popover.set_width_request(360);
        // Anchor the popover near the entry's left edge instead
        // of the default centre — when the entry is wide the
        // popover otherwise floats far to the right of the `@`
        // the user just typed (visible in the bug screenshot).
        popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(0, 0, 1, 1)));

        let inner = Rc::new(Inner {
            popover,
            list,
            entry: entry.clone(),
            avatars,
            candidates: RefCell::new(Vec::new()),
            sender,
            current_word: RefCell::new(None),
            inhibit: RefCell::new(false),
        });

        wire_entry(&inner);
        wire_keys(&inner);
        wire_list_activation(&inner);

        Self { inner }
    }

    /// Replace the candidate list (the popover repaints on the
    /// next text-change tick). Called when a fresh
    /// `MentionCandidatesLoaded` arrives from the worker.
    pub fn set_candidates(&self, candidates: Vec<MentionCandidate>) {
        *self.inner.candidates.borrow_mut() = candidates;
        // Re-run filter against the current text so an in-flight
        // word picks up the new list immediately rather than
        // waiting for the next keystroke.
        self.inner.update_completion();
    }
}

fn wire_entry(inner: &Rc<Inner>) {
    let buffer = inner.entry.buffer();
    {
        let inner = inner.clone();
        buffer.connect_notify_local(Some("text"), move |_, _| {
            inner.update_completion();
        });
    }
    {
        let inner_for_closure = inner.clone();
        inner
            .entry
            .connect_notify_local(Some("cursor-position"), move |_, _| {
                inner_for_closure.update_completion();
            });
    }
    // Hide the popover when the entry loses focus — without this,
    // the popover is visually orphaned when the user clicks into
    // another widget (its `autohide=false` keeps it open).
    {
        let inner_for_closure = inner.clone();
        inner.entry.connect_has_focus_notify(move |entry| {
            if !entry.has_focus() {
                inner_for_closure.popover.popdown();
            }
        });
    }
}

fn wire_keys(inner: &Rc<Inner>) {
    let key_ctl = gtk::EventControllerKey::new();
    key_ctl.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let inner = inner.clone();
        key_ctl.connect_key_pressed(move |_, key, _, modifier| {
            // Bail on any non-trivial modifier — we don't want to
            // intercept Ctrl-Enter (newline) or Shift-Tab.
            if modifier
                .difference(gtk::gdk::ModifierType::LOCK_MASK)
                .bits()
                != 0
            {
                return glib::Propagation::Proceed;
            }
            if !inner.popover.is_visible() {
                return glib::Propagation::Proceed;
            }
            match key {
                gtk::gdk::Key::Up | gtk::gdk::Key::KP_Up => {
                    inner.move_selection(-1);
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Down | gtk::gdk::Key::KP_Down => {
                    inner.move_selection(1);
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Return
                | gtk::gdk::Key::KP_Enter
                | gtk::gdk::Key::ISO_Enter
                | gtk::gdk::Key::Tab
                | gtk::gdk::Key::KP_Tab => {
                    inner.activate_selected();
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Escape => {
                    inner.inhibit_until_word_changes();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
    }
    inner.entry.add_controller(key_ctl);
}

fn wire_list_activation(inner: &Rc<Inner>) {
    let inner_act = inner.clone();
    inner.list.connect_row_activated(move |_, row| {
        // Stash the chosen index by reading the row's name (we set
        // it to the digit string on row build); cleaner than
        // walking widget hierarchy to extract the candidate.
        let Some(name) = row.widget_name().to_string().strip_prefix("mention:").map(String::from)
        else {
            return;
        };
        inner_act.activate_jid(&name);
    });
}

impl Inner {
    /// Re-derive `current_word`, filter candidates, repopulate the
    /// list, and pop the popover up/down accordingly.
    fn update_completion(self: &Rc<Self>) {
        let text = self.entry.text().to_string();
        let cursor = self.entry.position();
        let cursor_byte = char_offset_to_byte(&text, cursor as usize);

        let word = find_at_word(&text, cursor_byte);
        if word.is_none() {
            // Cursor moved out of the `@<…>` window — clear the
            // inhibit flag so a fresh `@` re-opens the popover.
            *self.inhibit.borrow_mut() = false;
            *self.current_word.borrow_mut() = None;
            self.popover.popdown();
            return;
        }
        if *self.inhibit.borrow() {
            return;
        }
        let (start, end) = word.unwrap();
        *self.current_word.borrow_mut() = Some((start, end));

        let query = &text[start + 1..end]; // skip the `@`
        let cands = self.candidates.borrow();
        let filtered = filter_candidates(&cands, query);
        if filtered.is_empty() {
            drop(cands);
            self.popover.popdown();
            return;
        }
        self.repopulate(&filtered);
        drop(cands);
        if !self.popover.is_visible() {
            self.popover.popup();
        }
        // Always select the first row when the filter changes —
        // otherwise the previous selection might be stale (out of
        // bounds or pointing at a row that was filtered out). We
        // deliberately do NOT call `grab_focus` on the row: that
        // would move keyboard focus off the entry, and any
        // subsequent characters the user types would land on the
        // ListBox (or be silently dropped) instead of being
        // inserted into the composer. The key controller on the
        // entry already handles ↑/↓/Enter/Tab/Esc while the entry
        // keeps focus.
        if let Some(first) = self.list.row_at_index(0) {
            self.list.select_row(Some(&first));
        }
    }

    fn repopulate(&self, candidates: &[&MentionCandidate]) {
        // Clean slate — small list, no point diff-ing.
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        for c in candidates.iter().take(MAX_VISIBLE_ROWS) {
            let row = build_row(c, &self.avatars);
            self.list.append(&row);
        }
    }

    fn move_selection(self: &Rc<Self>, delta: i32) {
        let count = self.list_row_count();
        if count == 0 {
            return;
        }
        let cur = self
            .list
            .selected_row()
            .map(|r| r.index())
            .unwrap_or(0);
        let mut new_idx = cur + delta;
        if new_idx < 0 {
            new_idx = 0;
        }
        if new_idx >= count {
            new_idx = count - 1;
        }
        // Just select — don't grab focus (see the comment in
        // `update_completion`: focus must stay on the entry so
        // the user can keep typing).
        if let Some(row) = self.list.row_at_index(new_idx) {
            self.list.select_row(Some(&row));
        }
    }

    fn list_row_count(&self) -> i32 {
        let mut n = 0;
        let mut child = self.list.first_child();
        while let Some(c) = child {
            n += 1;
            child = c.next_sibling();
        }
        n
    }

    fn activate_selected(self: &Rc<Self>) {
        let Some(row) = self.list.selected_row() else {
            return;
        };
        let Some(jid) = row
            .widget_name()
            .to_string()
            .strip_prefix("mention:")
            .map(String::from)
        else {
            return;
        };
        self.activate_jid(&jid);
    }

    fn activate_jid(self: &Rc<Self>, jid: &str) {
        let Some((start, end)) = *self.current_word.borrow() else {
            return;
        };
        let digits = jid.split('@').next().unwrap_or(jid).to_string();

        // Splice `@<digits> ` over `[start..end]` in the entry text.
        let text = self.entry.text().to_string();
        let mut new_text = String::with_capacity(text.len());
        new_text.push_str(&text[..start]);
        new_text.push('@');
        new_text.push_str(&digits);
        new_text.push(' ');
        new_text.push_str(&text[end..]);

        // Drive the buffer + cursor in one atomic-ish update so
        // the next `notify::text` sees the post-replace state and
        // doesn't reopen the popover for the same word.
        self.entry.set_text(&new_text);
        let new_cursor_bytes = start + 1 + digits.len() + 1;
        let new_cursor_chars = byte_offset_to_char(&new_text, new_cursor_bytes);
        self.entry.set_position(new_cursor_chars as i32);
        self.popover.popdown();
        *self.current_word.borrow_mut() = None;

        let _ = self.sender.send(ChatTabInput::MentionInserted {
            jid: jid.to_string(),
        });
    }

    fn inhibit_until_word_changes(self: &Rc<Self>) {
        *self.inhibit.borrow_mut() = true;
        self.popover.popdown();
    }
}

fn build_row(c: &MentionCandidate, avatars: &AvatarInventory) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    // Encode the JID into the row's GTK widget name so the
    // activation signal can recover it without needing a sidecar
    // map. `mention:` prefix avoids collisions with other widget
    // names.
    row.set_widget_name(&format!("mention:{}", c.jid));
    row.add_css_class("mention-row");

    let hbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_start(8)
        .margin_end(8)
        .margin_top(4)
        .margin_bottom(4)
        .build();

    let avatar = adw::Avatar::builder()
        .size(28)
        .text(&c.display_name)
        .show_initials(true)
        .build();
    avatar.set_valign(gtk::Align::Center);
    if let Some(tex) = avatars.load_texture(c.avatar_path.as_deref()) {
        avatar.set_custom_image(Some(&tex.upcast::<gtk::gdk::Paintable>()));
    }
    hbox.append(&avatar);

    // The labels Box must consume the remaining horizontal space.
    // Without `hexpand` the children's natural width is the only
    // signal GTK has — and an ellipsizing Label's natural width
    // is `…` alone. That's the bug seen in the screenshot: every
    // row collapsed to "avatar  …  …".
    let labels = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    // No `max_width_chars` here — when an ellipsizing label has a
    // max set, GTK reports its NATURAL width as that count; the
    // ScrolledWindow + popover chain then sized rows to "0 chars +
    // ellipsis" because the popover's layout pass picked the row's
    // natural-width-with-ellipsis as a sizing hint. Width is now
    // gated by the popover's `width_request` instead, and the
    // ellipsis still fires when content would overflow that.
    let name = gtk::Label::builder()
        .label(&c.display_name)
        .xalign(0.0)
        .halign(gtk::Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .single_line_mode(true)
        .hexpand(true)
        .build();
    labels.append(&name);

    // Subtitle: only shown for contacts with a real phone-number
    // JID (`<digits>@s.whatsapp.net`). LID-only contacts
    // (`<digits>@lid`) don't have a publishable phone, so the raw
    // LID digits would just look like noise — we skip the subtitle
    // entirely. The phone is run through `format_jid_or_phone` so
    // it matches what the rest of the UI shows for the same
    // contact (e.g., `+55 56 1945 60393` rather than 12 raw digits).
    if let Some(formatted_phone) = phone_subtitle(c) {
        let phone = gtk::Label::builder()
            .label(&formatted_phone)
            .xalign(0.0)
            .halign(gtk::Align::Start)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .single_line_mode(true)
            .hexpand(true)
            .build();
        phone.add_css_class("dim-label");
        phone.add_css_class("caption");
        labels.append(&phone);
    }
    hbox.append(&labels);

    row.set_child(Some(&hbox));
    row
}

/// Phone subtitle for a candidate, or `None` if the contact only
/// has a LID JID (no associated phone number — the digits aren't a
/// real number, just an opaque identifier).
fn phone_subtitle(c: &MentionCandidate) -> Option<String> {
    // The JID's server tells us the kind. `@s.whatsapp.net` and
    // `@c.us` are phone-number JIDs; `@lid` is the opaque-identifier
    // form WhatsApp introduced for privacy. Anything else (e.g.,
    // `@newsletter`, `@g.us`) wouldn't show up in the mention picker
    // anyway, but the explicit allow-list guards the format helper
    // against treating LID digits as a phone.
    let server = c.jid.split('@').nth(1).unwrap_or("");
    if !matches!(server, "s.whatsapp.net" | "c.us") {
        return None;
    }
    if c.phone.is_empty() {
        return None;
    }
    Some(crate::format::format_jid_or_phone(&c.phone))
}

/// Find the `@<word>` window the cursor is sitting inside.
/// Returns the byte range of the `@` start (inclusive) through the
/// end of the word (exclusive). The token spans contiguous
/// alphanumeric / `_` / `.` / `-` chars after the `@`. Returns
/// `None` when the cursor isn't inside such a token.
///
/// The returned `end` is always `>= start + 1` so callers can
/// safely slice `text[start + 1..end]` for the query — even when
/// the cursor sits exactly on the `@` (an empty query, all
/// candidates).
fn find_at_word(text: &str, cursor_byte: usize) -> Option<(usize, usize)> {
    let cursor_byte = cursor_byte.min(text.len());
    let bytes = text.as_bytes();

    // Walk backwards from cursor to find the `@`.
    let mut start = cursor_byte;
    while start > 0 {
        let prev_byte = bytes[start - 1];
        if prev_byte == b'@' {
            start -= 1;
            break;
        }
        if !is_word_byte(prev_byte) {
            return None;
        }
        start -= 1;
    }
    if start >= text.len() || bytes[start] != b'@' {
        return None;
    }
    // Either at start of buffer or preceded by whitespace —
    // otherwise this is `email@host` and we should leave it alone.
    if start > 0 {
        let pred = bytes[start - 1];
        if !pred.is_ascii_whitespace() {
            return None;
        }
    }

    // Walk forwards from `start + 1` (just past the `@`). Starting
    // from `cursor_byte` instead would let `end` land on `start`
    // itself when the cursor is sitting exactly on the trigger,
    // and the caller then panics on `text[start + 1 .. end]`.
    // Anchoring at `start + 1` guarantees `end >= start + 1` and
    // matches what the user means: "completion for everything
    // after the `@`".
    let mut end = start + 1;
    while end < text.len() && is_word_byte(bytes[end]) {
        end += 1;
    }
    Some((start, end))
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')
}

fn filter_candidates<'a>(
    all: &'a [MentionCandidate],
    query: &str,
) -> Vec<&'a MentionCandidate> {
    if query.is_empty() {
        return all.iter().collect();
    }
    let q = query.to_lowercase();
    let mut hits: Vec<&MentionCandidate> = all
        .iter()
        .filter(|c| {
            c.display_name.to_lowercase().contains(&q)
                || c.phone.contains(&q)
                || c.jid.to_lowercase().contains(&q)
        })
        .collect();
    // Bias rows whose name *starts* with the query above ones
    // that merely contain it — matches what users expect when
    // typing the first couple characters of a name.
    hits.sort_by_key(|c| {
        let starts_name = !c.display_name.to_lowercase().starts_with(&q);
        let starts_phone = !c.phone.starts_with(&q);
        (starts_name && starts_phone, c.display_name.to_lowercase())
    });
    hits
}

fn char_offset_to_byte(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

fn byte_offset_to_char(text: &str, byte_idx: usize) -> usize {
    text.char_indices()
        .position(|(b, _)| b >= byte_idx)
        .unwrap_or_else(|| text.chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_word_handles_cursor_on_trigger() {
        // Repro for the panic seen in production: cursor-position
        // notify fires with cursor == 0 while text was already
        // updated to "@". Old code returned (0,0) and the slice
        // `text[1..0]` aborted the process.
        let got = find_at_word("@", 0).unwrap();
        assert_eq!(got, (0, 1));
        let got = find_at_word("@", 1).unwrap();
        assert_eq!(got, (0, 1));
    }

    #[test]
    fn find_word_after_whitespace() {
        let text = "hi @al";
        let got = find_at_word(text, text.len()).unwrap();
        assert_eq!(got, (3, 6));
        assert_eq!(&text[got.0 + 1..got.1], "al");
    }

    #[test]
    fn find_word_rejects_email_like() {
        assert!(find_at_word("user@host", 9).is_none());
    }

    #[test]
    fn find_word_at_start_of_buffer() {
        let got = find_at_word("@bob hi", 4).unwrap();
        assert_eq!(got, (0, 4));
    }
}
