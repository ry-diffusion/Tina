// One open chat — header strip with the contact's name, scrollable thread
// of message bubbles, and a single-line composer. The "active chat" gating
// (which thread receives push updates) is the parent's job; a tab just
// renders whatever it's been handed.

use adw::prelude::*;
use gtk::glib;
use relm4::factory::FactoryVecDeque;
use relm4::prelude::*;
use tina_db::MessageRow;

use crate::components::message_bubble::{MessageBubble, MessageBubbleInput, MessageItem};

/// Window in seconds within which two messages from the same sender are
/// rendered as a collapsed run (no avatar/header on the second). Mirrors
/// Dissent's 10-minute grouping window.
const COLLAPSE_WINDOW_SECS: i64 = 10 * 60;

fn sender_key(row: &MessageRow) -> String {
    if row.is_from_me {
        "\0me".to_string()
    } else {
        row.sender_name.clone().unwrap_or_default()
    }
}

/// Build a `MessageItem` from a row, hydrating avatar + media state from
/// the shared inventories and emitting fetch requests as needed.
/// `output` is the chat tab's output sender; we route through it because
/// `MessageItem` construction happens during `init`/`update`, both of
/// which already have access to the sender.
fn build_item(
    row: &MessageRow,
    is_collapsed: bool,
    avatars: &crate::inventory::AvatarInventory,
    media: &crate::inventory::MediaInventory,
    user_jid: Option<&str>,
    request_fetch_avatar: &mut impl FnMut(String),
) -> MessageItem {
    let mut item = MessageItem::from_row(row, is_collapsed);

    // For from_me messages the DB stores sender_contact_id=NULL (we
    // never auto-register a contact for the signed-in user), so the
    // JOIN can't resolve a sender_jid — override with the known
    // identity here so the avatar lookup matches against the same key
    // the inventory was populated with via SetIdentity.
    if item.from_me && item.sender_jid.is_none() {
        item.sender_jid = user_jid.map(|s| s.to_string());
    }

    // Avatar: prefer the live inventory over the JOIN'd snapshot.
    if let Some(jid) = item.sender_jid.clone() {
        if !jid.is_empty() {
            if let Some(p) = avatars.get(&jid) {
                item.sender_avatar_path = Some(p);
            } else if avatars.needs_fetch(&jid) {
                request_fetch_avatar(jid);
            }
        }
    }

    // Media: in-flight states + recent successes live here, not in the
    // DB. Override the row's snapshot when the inventory has fresher info.
    if let Some(state) = media.get(&item.id) {
        if state.path.is_some() {
            item.media_path = state.path;
        }
        if !state.status.is_empty() {
            item.media_status = state.status;
        }
        if item.media_mimetype.is_none() && state.mimetype.is_some() {
            item.media_mimetype = state.mimetype;
        }
    }

    item
}

/// Decide whether `row` should be rendered as a collapsed continuation
/// of the previous row in the thread. Updates the trailing-state cursor
/// in place so callers can iterate without manual bookkeeping.
fn collapse_against(
    row: &MessageRow,
    last_sender: &mut Option<String>,
    last_ts: &mut Option<i64>,
) -> bool {
    let key = sender_key(row);
    let collapsed = match (last_sender.as_deref(), *last_ts) {
        (Some(prev_sender), Some(prev_ts)) => {
            prev_sender == key && row.timestamp.saturating_sub(prev_ts) <= COLLAPSE_WINDOW_SECS
        }
        _ => false,
    };
    *last_sender = Some(key);
    *last_ts = Some(row.timestamp);
    collapsed
}

#[derive(Debug)]
pub enum ChatTabInput {
    SetMeta {
        name: String,
        kind: String,
    },
    Reset(Vec<MessageRow>),
    Append(Vec<MessageRow>),
    Send,
    MediaReady {
        message_ids: Vec<String>,
        path: String,
        mimetype: Option<String>,
    },
    MediaFailed(String),
    RequestMediaDownload(String),
    /// VAdjustment crossed the load-more threshold. Internal trigger.
    NearTop,
    /// User scrolled back to the bottom — opportunity to prune the top
    /// of the factory if it grew past the soft cap.
    NearBottom,
    /// Older page came back from the worker. `reached_top = true` means
    /// the worker returned fewer rows than requested → we've loaded the
    /// entire history; stop trying.
    PrependOlder {
        messages: Vec<MessageRow>,
        reached_top: bool,
    },
    /// User switched into this tab. Force sticky-bottom + a deferred
    /// scroll so the freshly-realised page lands on the latest message.
    StickToBottom,
    /// Worker resolved a profile picture — apply it to every message
    /// row whose sender JID matches.
    AvatarReady { jid: String, path: String },
    /// Identity arrived (or changed) — back-fill `sender_jid` on
    /// existing from_me rows and apply the cached avatar to them.
    SetUserJid(Option<String>),
}

#[derive(Debug)]
pub enum ChatTabOutput {
    Send { chat_id: String, text: String },
    Close { chat_id: String },
    RequestMediaDownload(String),
    RequestLoadOlder { chat_id: String, before_ts: i64 },
    /// Ask the worker to fetch a sender's profile picture. Deduped at
    /// the tab level so we only round-trip per JID once.
    RequestFetchAvatar(String),
}

pub struct ChatTabInit {
    pub chat_id: String,
    pub name: String,
    pub kind: String,
    pub initial: Vec<MessageRow>,
    pub avatars: crate::inventory::AvatarInventory,
    pub media: crate::inventory::MediaInventory,
    /// Signed-in user's JID, used to override `sender_jid` for `from_me`
    /// rows (which the DB stores with `sender_contact_id = NULL`, so the
    /// JOIN can't resolve a JID for them).
    pub user_jid: Option<String>,
}

pub struct ChatTab {
    chat_id: String,
    name: String,
    kind: String,
    messages: FactoryVecDeque<MessageBubble>,
    composer_buffer: gtk::EntryBuffer,
    avatars: crate::inventory::AvatarInventory,
    media: crate::inventory::MediaInventory,
    user_jid: Option<String>,
    scroll: Option<gtk::ScrolledWindow>,
    seen_message_ids: std::collections::HashSet<String>,
    last_send: Option<(String, std::time::Instant)>,
    oldest_ts: Option<i64>,
    loading_older: bool,
    reached_top: bool,
    pending_echoes: std::collections::HashMap<String, std::collections::VecDeque<String>>,
    /// Sticky-bottom state, ported from dissent's autoscroll.Window. When
    /// `true`, every `vadj.changed` (new content added → upper grew)
    /// re-scrolls to `upper - page_size`. Cleared when the user scrolls
    /// away from the bottom; re-set when they scroll back.
    bottomed: std::rc::Rc<std::cell::Cell<bool>>,
    /// Edge-detection flag matching dissent's `updatedValue`. The
    /// `changed` signal sets it; the deferred `value-changed` resolution
    /// reads it to distinguish "GTK relayout finished" from "user
    /// dragged the scrollbar".
    updated_value: std::rc::Rc<std::cell::Cell<bool>>,
}

#[relm4::component(pub)]
impl SimpleComponent for ChatTab {
    type Init = ChatTabInit;
    type Input = ChatTabInput;
    type Output = ChatTabOutput;

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,

            // The chat header (avatar + name + kind) lives in the parent
            // window's AdwHeaderBar — set by `MainPage` based on the
            // currently-selected tab — so each tab's content area starts
            // straight at the message thread.

            #[name(scroll)]
            gtk::ScrolledWindow {
                set_vexpand: true,
                set_hscrollbar_policy: gtk::PolicyType::Never,

                #[local_ref]
                messages_list -> gtk::ListBox {
                    set_selection_mode: gtk::SelectionMode::None,
                    add_css_class: "background",
                },
            },

            gtk::Separator {},

            gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,
                set_margin_top: 6,
                set_margin_bottom: 6,
                set_margin_start: 12,
                set_margin_end: 12,
                set_spacing: 6,

                gtk::Entry {
                    set_buffer: &model.composer_buffer,
                    set_hexpand: true,
                    set_placeholder_text: Some("Message…"),
                    connect_activate => ChatTabInput::Send,
                },

                gtk::Button {
                    // Bundled from relm4-icons (icon-development-kit set);
                    // see crates/tina-gtk/icons/ + icons.toml.
                    set_icon_name: "curved-arrow-left-symbolic",
                    set_tooltip_text: Some("Send"),
                    add_css_class: "suggested-action",
                    connect_clicked => ChatTabInput::Send,
                },
            },
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let mut messages = FactoryVecDeque::builder()
            .launch(gtk::ListBox::default())
            .forward(sender.input_sender(), |o| match o {
                crate::components::message_bubble::MessageBubbleOut::DownloadRequested(id) => {
                    ChatTabInput::RequestMediaDownload(id)
                }
            });

        // Seed with initial history. The collapse cursor is purely
        // local to this loop — for any subsequent Append/Send we re-read
        // the trailing item directly from the factory, keeping the
        // factory as the single source of truth.
        let mut last_sender: Option<String> = None;
        let mut last_ts: Option<i64> = None;
        let mut avatar_fetches: Vec<String> = Vec::new();
        {
            let mut guard = messages.guard();
            for row in &init.initial {
                let collapsed = collapse_against(row, &mut last_sender, &mut last_ts);
                let item = build_item(
                    row,
                    collapsed,
                    &init.avatars,
                    &init.media,
                    init.user_jid.as_deref(),
                    &mut |jid| avatar_fetches.push(jid),
                );
                guard.push_back(item);
            }
        }
        // Drain after the guard drops — Sender::send is cheap but we
        // don't want it to interleave with factory pushes.
        for jid in avatar_fetches {
            let _ = sender.output(ChatTabOutput::RequestFetchAvatar(jid));
        }

        let mut seen: std::collections::HashSet<String> =
            std::collections::HashSet::with_capacity(init.initial.len());
        for r in &init.initial {
            seen.insert(r.message_id.clone());
        }

        let oldest_ts = init.initial.iter().map(|r| r.timestamp).min();

        let bottomed = std::rc::Rc::new(std::cell::Cell::new(true));
        let updated_value = std::rc::Rc::new(std::cell::Cell::new(false));

        // The local seed cursors are dropped here — `factory_tail_cursor()`
        // is what every subsequent collapse decision queries.
        let _ = (last_sender, last_ts);

        let mut model = ChatTab {
            chat_id: init.chat_id,
            name: init.name,
            kind: init.kind,
            messages,
            composer_buffer: gtk::EntryBuffer::default(),
            avatars: init.avatars,
            media: init.media,
            user_jid: init.user_jid,
            scroll: None,
            seen_message_ids: seen,
            last_send: None,
            oldest_ts,
            loading_older: false,
            reached_top: false,
            pending_echoes: std::collections::HashMap::new(),
            bottomed: bottomed.clone(),
            updated_value: updated_value.clone(),
        };

        let messages_list = model.messages.widget();
        let widgets = view_output!();
        model.scroll = Some(widgets.scroll.clone());

        // ── Sticky-bottom autoscroll, ported from gotkit's autoscroll.Window
        //
        // `changed` fires when the adjustment's upper changes (i.e. new
        // content was laid out into the listbox). If we were at the
        // bottom, jump back to the new bottom via idle_add — running
        // through one extra frame matches dissent's behaviour and lets
        // GTK finish allocating the new row before we set value().
        //
        // `value-changed` fires for both user scrolls AND our own
        // set_value calls. The `updated_value` flag set above lets us
        // ignore the immediate echo from the relayout path; only
        // genuine user input flips `bottomed` to false.
        {
            let scroll = widgets.scroll.clone();
            let bottomed = bottomed.clone();
            let updated_value = updated_value.clone();
            scroll.vadjustment().connect_changed(move |adj| {
                updated_value.set(true);
                if bottomed.get() {
                    let adj = adj.clone();
                    glib::idle_add_local_once(move || {
                        let target = adj.upper() - adj.page_size();
                        if target >= 0.0 {
                            adj.set_value(target);
                        }
                    });
                }
            });
        }

        // Lazy-load on near-top, prune on near-bottom, and update the
        // bottomed flag based on user scroll position. We skip the
        // bottomed update on the first event after a relayout (signaled
        // by `updated_value`), since GTK can briefly clamp value before
        // re-allocating the new content.
        {
            let scroll = widgets.scroll.clone();
            let input = sender.input_sender().clone();
            let bottomed = bottomed.clone();
            let updated_value = updated_value.clone();
            scroll.vadjustment().connect_value_changed(move |adj| {
                let value = adj.value();
                let page = adj.page_size();
                let upper = adj.upper();
                let bottom_value = upper - page;

                if updated_value.replace(false) {
                    // Came from a relayout — don't reinterpret as the
                    // user scrolling away.
                } else {
                    bottomed.set(bottom_value < 0.0 || value >= bottom_value);
                }

                if value < page * 2.0 && upper > page * 2.0 {
                    let _ = input.send(ChatTabInput::NearTop);
                }
                if value >= bottom_value - 50.0 {
                    let _ = input.send(ChatTabInput::NearBottom);
                }
            });
        }

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: ChatTabInput, sender: ComponentSender<Self>) {
        match msg {
            ChatTabInput::SetMeta { name, kind } => {
                self.name = name;
                self.kind = kind;
            }
            ChatTabInput::Reset(rows) => {
                self.oldest_ts = rows.iter().map(|r| r.timestamp).min();
                self.reached_top = rows.len() < 50;
                self.loading_older = false;
                // Force sticky-bottom on every chat open. The
                // connect_changed handler we registered will catch each
                // upper-grew tick as the factory lays out the rows and
                // re-scroll to the bottom; no manual timeouts needed.
                self.bottomed.set(true);
                let mut avatar_fetches: Vec<String> = Vec::new();
                {
                    let mut guard = self.messages.guard();
                    guard.clear();
                    self.seen_message_ids.clear();
                    let mut cursor_sender: Option<String> = None;
                    let mut cursor_ts: Option<i64> = None;
                    for row in &rows {
                        let collapsed =
                            collapse_against(row, &mut cursor_sender, &mut cursor_ts);
                        self.seen_message_ids.insert(row.message_id.clone());
                        let item = build_item(
                            row,
                            collapsed,
                            &self.avatars,
                            &self.media,
                            self.user_jid.as_deref(),
                            &mut |jid| avatar_fetches.push(jid),
                        );
                        guard.push_back(item);
                    }
                }
                for jid in avatar_fetches {
                    let _ = sender.output(ChatTabOutput::RequestFetchAvatar(jid));
                }
            }
            ChatTabInput::Append(rows) => {
                tracing::info!(
                    chat = %self.chat_id,
                    count = rows.len(),
                    "ChatTab::Append"
                );

                // For each from_me row that confirms a pending optimistic
                // echo, REPLACE the local placeholder in-place (preserving
                // its `is_collapsed` flag) rather than remove+append. The
                // remove+append path silently broke the collapse seam
                // when the dropped local was the head of a from_me run:
                // the next local in the run stayed flagged collapsed,
                // ended up first-visible, and rendered without an avatar
                // — visually attaching to whoever spoke before.
                let mut avatar_fetches: Vec<String> = Vec::new();
                let mut confirmed_server_ids: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                let mut confirmed_local_ids: Vec<String> = Vec::new();
                {
                    let local_idx_state: std::collections::HashMap<String, (usize, bool)> = self
                        .messages
                        .guard()
                        .iter()
                        .enumerate()
                        .map(|(i, f)| (f.item.id.clone(), (i, f.item.is_collapsed)))
                        .collect();
                    let mut replacements: Vec<(usize, MessageRow, bool)> = Vec::new();
                    for r in &rows {
                        if !r.is_from_me {
                            continue;
                        }
                        let body = r.content.clone().unwrap_or_default();
                        if body.is_empty() {
                            continue;
                        }
                        let Some(queue) = self.pending_echoes.get_mut(&body) else {
                            continue;
                        };
                        let Some(local_id) = queue.pop_front() else {
                            continue;
                        };
                        if queue.is_empty() {
                            self.pending_echoes.remove(&body);
                        }
                        if let Some((idx, was_collapsed)) = local_idx_state.get(&local_id).copied()
                        {
                            replacements.push((idx, r.clone(), was_collapsed));
                            confirmed_server_ids.insert(r.message_id.clone());
                            confirmed_local_ids.push(local_id);
                        }
                        // If the local isn't in the factory anymore (e.g.
                        // Reset wiped it), let the server row fall through
                        // to the regular append path below.
                    }
                    // Replace in descending idx so the surviving indices
                    // stay valid as we mutate the factory.
                    replacements.sort_by_key(|(idx, _, _)| std::cmp::Reverse(*idx));
                    for (idx, row, was_collapsed) in replacements {
                        let item = build_item(
                            &row,
                            was_collapsed,
                            &self.avatars,
                            &self.media,
                            self.user_jid.as_deref(),
                            &mut |jid| avatar_fetches.push(jid),
                        );
                        let mut guard = self.messages.guard();
                        guard.remove(idx);
                        guard.insert(idx, item);
                    }
                }
                for id in &confirmed_local_ids {
                    self.seen_message_ids.remove(id);
                }
                for id in &confirmed_server_ids {
                    self.seen_message_ids.insert(id.clone());
                }

                let new_rows: Vec<_> = rows
                    .into_iter()
                    .filter(|r| !confirmed_server_ids.contains(&r.message_id))
                    .filter(|r| self.seen_message_ids.insert(r.message_id.clone()))
                    .collect();
                if new_rows.is_empty() {
                    for jid in avatar_fetches {
                        let _ = sender.output(ChatTabOutput::RequestFetchAvatar(jid));
                    }
                    return;
                }
                // Always seed the collapse cursor from the factory's
                // current trailing item — never from a stashed field.
                // Any other path (echo drop, pane move, tab switch, etc.)
                // is allowed to mutate the factory between Append calls,
                // and we'd silently misgroup messages if we trusted a
                // stale `last_sender`. The factory IS the source of
                // truth.
                let (mut cursor_sender, mut cursor_ts) = self.factory_tail_cursor();
                {
                    let mut guard = self.messages.guard();
                    for row in &new_rows {
                        let collapsed =
                            collapse_against(row, &mut cursor_sender, &mut cursor_ts);
                        let item = build_item(
                            row,
                            collapsed,
                            &self.avatars,
                            &self.media,
                            self.user_jid.as_deref(),
                            &mut |jid| avatar_fetches.push(jid),
                        );
                        guard.push_back(item);
                    }
                }
                for jid in avatar_fetches {
                    let _ = sender.output(ChatTabOutput::RequestFetchAvatar(jid));
                }
                // The connect_changed handler will autoscroll if `bottomed`
                // — meaning we only follow new messages when the user was
                // already at (or near) the bottom. If they scrolled up to
                // read history, they stay where they are.
            }
            ChatTabInput::Send => {
                let text = self.composer_buffer.text().to_string();
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    return;
                }
                if let Some((prev, when)) = &self.last_send {
                    if prev == trimmed && when.elapsed() < std::time::Duration::from_secs(1) {
                        tracing::warn!(
                            chat = %self.chat_id,
                            "Send debounced (duplicate within 1s)"
                        );
                        self.composer_buffer.set_text("");
                        return;
                    }
                }
                self.last_send = Some((trimmed.to_string(), std::time::Instant::now()));

                // Optimistic local echo: synthesise a bubble with a
                // sentinel id and push it before the IPC roundtrip even
                // starts. When the worker echoes the real row back, the
                // matching local entry is dropped so the real one slots
                // in at the same visual position.
                let local_id = format!(
                    "local-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos())
                        .unwrap_or_default()
                );
                let now_unix = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or_default();
                // Collapse the optimistic echo against the trailing
                // message just like a real row would. Read the cursor
                // from the factory's tail so a stale `last_sender`
                // can't misgroup my echo under the recipient's avatar.
                let (cursor_sender, cursor_ts) = self.factory_tail_cursor();
                let local_collapsed = match (cursor_sender.as_deref(), cursor_ts) {
                    (Some("\0me"), Some(prev_ts)) => {
                        now_unix.saturating_sub(prev_ts) <= COLLAPSE_WINDOW_SECS
                    }
                    _ => false,
                };
                let local_avatar = self
                    .user_jid
                    .as_deref()
                    .and_then(|j| self.avatars.get(j));
                let local_item = MessageItem {
                    id: local_id.clone(),
                    from_me: true,
                    sender_name: String::new(),
                    sender_jid: self.user_jid.clone(),
                    sender_avatar_path: local_avatar,
                    is_collapsed: local_collapsed,
                    content: trimmed.to_string(),
                    message_type: "text".to_string(),
                    timestamp: crate::time::format_message_time(now_unix),
                    timestamp_unix: now_unix,
                    media_summary: String::new(),
                    media_mimetype: None,
                    media_size_bytes: None,
                    media_duration_secs: None,
                    media_path: None,
                    media_status: "none".to_string(),
                    media_filename: None,
                    thumbnail: None,
                };
                self.seen_message_ids.insert(local_id.clone());
                self.pending_echoes
                    .entry(trimmed.to_string())
                    .or_default()
                    .push_back(local_id);
                // Force sticky on send — even if the user had scrolled up
                // to read history, sending a message is a strong intent
                // signal that they want to see what they just typed.
                self.bottomed.set(true);
                {
                    let mut guard = self.messages.guard();
                    guard.push_back(local_item);
                }
                // The connect_changed handler will jump us to the new
                // bottom now that bottomed=true.

                let _ = sender.output(ChatTabOutput::Send {
                    chat_id: self.chat_id.clone(),
                    text: trimmed.to_string(),
                });
                self.composer_buffer.set_text("");
            }
            ChatTabInput::MediaReady {
                message_ids,
                path,
                mimetype,
            } => {
                // Mutate the factory items in place via per-row Input —
                // no remove+insert, so the listbox keeps the same widget
                // hierarchy and the scroll position never jumps.
                let id_set: std::collections::HashSet<&String> = message_ids.iter().collect();
                let indices: Vec<usize> = self
                    .messages
                    .guard()
                    .iter()
                    .enumerate()
                    .filter_map(|(i, f)| {
                        if id_set.contains(&f.item.id) {
                            Some(i)
                        } else {
                            None
                        }
                    })
                    .collect();
                for idx in indices {
                    self.messages.send(
                        idx,
                        MessageBubbleInput::UpdateMedia {
                            path: Some(path.clone()),
                            status: "done".into(),
                            mimetype: mimetype.clone(),
                        },
                    );
                }
            }
            ChatTabInput::SetUserJid(new_jid) => {
                self.user_jid = new_jid.clone();
                let Some(jid) = new_jid else {
                    return;
                };
                if jid.is_empty() {
                    return;
                }
                // Back-fill sender_jid on every existing from_me row + paint
                // the cached avatar if the inventory already has it.
                let cached = self.avatars.get(&jid);
                let indices: Vec<usize> = self
                    .messages
                    .guard()
                    .iter()
                    .enumerate()
                    .filter_map(|(i, f)| if f.item.from_me { Some(i) } else { None })
                    .collect();
                for idx in indices {
                    self.messages.send(
                        idx,
                        crate::components::message_bubble::MessageBubbleInput::SetSenderJid(
                            jid.clone(),
                        ),
                    );
                    if let Some(p) = cached.clone() {
                        self.messages.send(
                            idx,
                            crate::components::message_bubble::MessageBubbleInput::SetAvatar(p),
                        );
                    }
                }
                if cached.is_none() && self.avatars.needs_fetch(&jid) {
                    let _ = sender.output(ChatTabOutput::RequestFetchAvatar(jid));
                }
            }
            ChatTabInput::AvatarReady { jid, path } => {
                let indices: Vec<usize> = self
                    .messages
                    .guard()
                    .iter()
                    .enumerate()
                    .filter_map(|(i, f)| {
                        if f.item.sender_jid.as_deref() == Some(jid.as_str()) {
                            Some(i)
                        } else {
                            None
                        }
                    })
                    .collect();
                for idx in indices {
                    self.messages.send(
                        idx,
                        crate::components::message_bubble::MessageBubbleInput::SetAvatar(
                            path.clone(),
                        ),
                    );
                }
            }
            ChatTabInput::MediaFailed(message_id) => {
                let indices: Vec<usize> = self
                    .messages
                    .guard()
                    .iter()
                    .enumerate()
                    .filter_map(|(i, f)| {
                        if f.item.id == message_id {
                            Some(i)
                        } else {
                            None
                        }
                    })
                    .collect();
                for idx in indices {
                    self.messages.send(
                        idx,
                        MessageBubbleInput::UpdateMedia {
                            path: None,
                            status: "failed".into(),
                            mimetype: None,
                        },
                    );
                }
            }
            ChatTabInput::StickToBottom => {
                self.bottomed.set(true);
                if let Some(scroll) = self.scroll.clone() {
                    // The page may have only just been realised by
                    // AdwTabView selecting it — schedule both an idle
                    // and a 50ms timeout so we catch the layout pass.
                    let s1 = scroll.clone();
                    glib::idle_add_local_once(move || {
                        let adj = s1.vadjustment();
                        let target = adj.upper() - adj.page_size();
                        if target >= 0.0 {
                            adj.set_value(target);
                        }
                    });
                    glib::timeout_add_local_once(
                        std::time::Duration::from_millis(50),
                        move || {
                            let adj = scroll.vadjustment();
                            let target = adj.upper() - adj.page_size();
                            if target >= 0.0 {
                                adj.set_value(target);
                            }
                        },
                    );
                }
            }
            ChatTabInput::NearBottom => {
                // Soft cap: when the user is parked at the bottom and the
                // factory holds more than `MAX_KEEP` rows, drop the
                // oldest down to `TARGET`. Re-opens the scroll-up path
                // (clears `reached_top` because there's now older history
                // we don't have in memory).
                const MAX_KEEP: usize = 150;
                const TARGET: usize = 100;
                let count = self.messages.len();
                if count <= MAX_KEEP {
                    return;
                }
                let to_drop = count - TARGET;
                let mut dropped_ids: Vec<String> = Vec::with_capacity(to_drop);
                {
                    let guard = self.messages.guard();
                    for fac in guard.iter().take(to_drop) {
                        dropped_ids.push(fac.item.id.clone());
                    }
                }
                {
                    let mut guard = self.messages.guard();
                    for _ in 0..to_drop {
                        guard.pop_front();
                    }
                }
                for id in &dropped_ids {
                    self.seen_message_ids.remove(id);
                }
                // New oldest = first remaining item's timestamp.
                self.oldest_ts = self
                    .messages
                    .guard()
                    .iter()
                    .next()
                    .map(|f| f.item.timestamp_unix);
                // We dropped real history — older pages are once again
                // legitimately available to fetch.
                self.reached_top = false;
                tracing::info!(
                    chat = %self.chat_id,
                    dropped = to_drop,
                    remaining = self.messages.len(),
                    "ChatTab: pruned top after near-bottom"
                );
            }
            ChatTabInput::NearTop => {
                if self.loading_older || self.reached_top {
                    return;
                }
                let Some(before_ts) = self.oldest_ts else {
                    return;
                };
                self.loading_older = true;
                tracing::info!(
                    chat = %self.chat_id,
                    before_ts,
                    "ChatTab: requesting older page",
                );
                let _ = sender.output(ChatTabOutput::RequestLoadOlder {
                    chat_id: self.chat_id.clone(),
                    before_ts,
                });
            }
            ChatTabInput::PrependOlder {
                messages,
                reached_top,
            } => {
                self.loading_older = false;
                if reached_top || messages.len() < 50 {
                    self.reached_top = true;
                }
                if messages.is_empty() {
                    return;
                }
                // LockScroll / UnlockScroll pattern (gotkit autoscroll):
                // capture (upper, value) before prepend, then after the
                // layout settles set value = old_value + (new_upper -
                // old_upper). User stays on the same content while
                // history grows above. We also turn `bottomed` off
                // explicitly so the connect_changed handler doesn't
                // pull us back to the bottom on the upper notification.
                let saved = self
                    .scroll
                    .as_ref()
                    .map(|s| (s.vadjustment().upper(), s.vadjustment().value()));
                let prev_bottomed = self.bottomed.replace(false);

                let new_oldest = messages.iter().map(|r| r.timestamp).min();
                if let Some(t) = new_oldest {
                    self.oldest_ts = Some(match self.oldest_ts {
                        Some(prev) => prev.min(t),
                        None => t,
                    });
                }

                {
                    let mut guard = self.messages.guard();
                    // Walk the prepended block forwards to compute collapse
                    // state inside it; insert each row at the front in
                    // reverse so the final order stays chronological. Note
                    // we don't recompute the existing oldest row's collapse
                    // state — minor visual artifact at the seam.
                    let mut sender_cursor: Option<String> = None;
                    let mut ts_cursor: Option<i64> = None;
                    let collapsed_flags: Vec<bool> = messages
                        .iter()
                        .map(|r| collapse_against(r, &mut sender_cursor, &mut ts_cursor))
                        .collect();
                    let mut avatar_fetches: Vec<String> = Vec::new();
                    for (row, collapsed) in messages.iter().zip(&collapsed_flags).rev() {
                        if !self.seen_message_ids.insert(row.message_id.clone()) {
                            continue;
                        }
                        let item = build_item(
                            row,
                            *collapsed,
                            &self.avatars,
                            &self.media,
                            self.user_jid.as_deref(),
                            &mut |jid| avatar_fetches.push(jid),
                        );
                        guard.push_front(item);
                    }
                    drop(guard);
                    for jid in avatar_fetches {
                        let _ = sender.output(ChatTabOutput::RequestFetchAvatar(jid));
                    }
                }

                if let (Some(scroll), Some((old_upper, old_value))) = (self.scroll.clone(), saved) {
                    let bottomed_flag = self.bottomed.clone();
                    glib::idle_add_local_once(move || {
                        let adj = scroll.vadjustment();
                        let new_upper = adj.upper();
                        let delta = new_upper - old_upper;
                        adj.set_value(old_value + delta);
                        // Restore the bottomed state we suppressed during
                        // the prepend — `prev_bottomed` reflects what the
                        // user wanted before the lazy-load fired.
                        bottomed_flag.set(prev_bottomed);
                    });
                }
            }
            ChatTabInput::RequestMediaDownload(id) => {
                // Mark the in-flight state in the shared inventory so
                // closing/reopening the tab doesn't lose the spinner.
                self.media.set_downloading(&id);
                // In-place factory update via per-row Input — the listbox
                // keeps the same widget instance, so no row-rebuild and
                // no scroll jump on click.
                let indices: Vec<usize> = self
                    .messages
                    .guard()
                    .iter()
                    .enumerate()
                    .filter_map(|(i, f)| {
                        if f.item.id == id {
                            Some(i)
                        } else {
                            None
                        }
                    })
                    .collect();
                for idx in indices {
                    self.messages.send(
                        idx,
                        MessageBubbleInput::UpdateMedia {
                            path: None,
                            status: "downloading".into(),
                            mimetype: None,
                        },
                    );
                }
                let _ = sender.output(ChatTabOutput::RequestMediaDownload(id));
            }
        }
    }
}

impl ChatTab {
    pub fn chat_id(&self) -> &str {
        &self.chat_id
    }

    /// `(sender_key, timestamp)` for the trailing item in the factory.
    /// Used to seed collapse decisions for incoming Append batches and
    /// optimistic Send echoes — the factory is the single source of
    /// truth for "what was just rendered", so this avoids the state
    /// drift that creeps in when a separate `last_sender` field is
    /// kept in sync across many code paths.
    fn factory_tail_cursor(&self) -> (Option<String>, Option<i64>) {
        let Some(last) = self.messages.back() else {
            return (None, None);
        };
        let key = if last.item.from_me {
            "\0me".to_string()
        } else {
            last.item.sender_name.clone()
        };
        (Some(key), Some(last.item.timestamp_unix))
    }
}
