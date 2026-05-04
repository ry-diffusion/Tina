// One open chat — header strip with the contact's name, scrollable thread
// of message bubbles, and a single-line composer. The "active chat" gating
// (which thread receives push updates) is the parent's job; a tab just
// renders whatever it's been handed.
//
// The match-arm bodies live next door in `actions.rs`; this file owns the
// view + the `init` + a thin `update` dispatcher.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use adw::prelude::*;
use relm4::factory::FactoryVecDeque;
use relm4::prelude::*;

use super::build::{build_item, collapse_against};
use super::messages::{ChatTabInit, ChatTabInput, ChatTabOutput};
use super::model::ChatTab;
use super::scroll::{wire_changed, wire_value_changed};

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
        _root: Self::Root,
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
        // local to this loop — for any subsequent Append/Send we
        // re-read the trailing item directly from the factory, keeping
        // the factory as the single source of truth.
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

        let mut seen: HashSet<String> = HashSet::with_capacity(init.initial.len());
        for r in &init.initial {
            seen.insert(r.message_id.clone());
        }

        let oldest_ts = init.initial.iter().map(|r| r.timestamp).min();

        let bottomed = Rc::new(Cell::new(true));
        let updated_value = Rc::new(Cell::new(false));

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
            pending_echoes: HashMap::new(),
            bottomed: bottomed.clone(),
            updated_value: updated_value.clone(),
        };

        let messages_list = model.messages.widget();
        let widgets = view_output!();
        model.scroll = Some(widgets.scroll.clone());

        wire_changed(&widgets.scroll, bottomed.clone(), updated_value.clone());
        wire_value_changed(
            &widgets.scroll,
            sender.input_sender().clone(),
            bottomed,
            updated_value,
        );

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: ChatTabInput, sender: ComponentSender<Self>) {
        self.dispatch(msg, sender);
    }
}
