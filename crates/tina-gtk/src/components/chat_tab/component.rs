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
use relm4::prelude::*;
use relm4::typed_view::list::TypedListView;

use super::build::{build_item, collapse_against, day_flips};
use super::messages::{ChatTabInit, ChatTabInput, ChatTabOutput};
use super::model::ChatTab;
use super::scroll::{wire_changed, wire_value_changed};
use crate::components::message_row::{MessageRowItem, RowUiInventory};

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
                list_view -> gtk::ListView {
                    set_single_click_activate: false,
                    add_css_class: "background",
                    add_css_class: "tina-message-list",
                    // Natural sizing so `gtk::Picture` (image / video
                    // thumb) can grow to its preferred height instead
                    // of being clipped by ListView's default minimum
                    // sizing — same pattern Fractal uses on its
                    // room_history ListView.
                    set_vscroll_policy: gtk::ScrollablePolicy::Natural,
                },
            },

            gtk::Separator {},

            // Composer / read-only banner — newsletters and the
            // status@broadcast pseudo-chat don't accept replies, so we
            // swap the Entry+Send pair for a centred dim label rather
            // than show a deceptively typeable composer that errors
            // out on send.
            gtk::Stack {
                set_transition_type: gtk::StackTransitionType::Crossfade,
                #[watch]
                set_visible_child_name: if model.is_read_only() {
                    "readonly"
                } else {
                    "compose"
                },

                add_named[Some("compose")] = &gtk::Box {
                    set_orientation: gtk::Orientation::Horizontal,
                    set_margin_top: 6,
                    set_margin_bottom: 6,
                    set_margin_start: 12,
                    set_margin_end: 12,
                    set_spacing: 6,

                    // Attach popover. Each row is a dedicated kind so
                    // the file dialog can apply a sensible mime
                    // filter. Stickers stay separate from images on
                    // purpose — they ship with a forced image/webp
                    // mimetype on the wire, and silently rebranding
                    // an arbitrary jpg as a sticker would surface as
                    // a broken bubble on the peer.
                    gtk::MenuButton {
                        set_icon_name: "attachment-symbolic",
                        set_tooltip_text: Some("Attach"),
                        set_valign: gtk::Align::Center,
                        #[wrap(Some)]
                        set_popover = &gtk::Popover {
                            set_position: gtk::PositionType::Top,
                            #[wrap(Some)]
                            set_child = &gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 2,

                                gtk::Button {
                                    set_label: "Photo",
                                    add_css_class: "flat",
                                    connect_clicked[sender] => move |btn| {
                                        if let Some(pop) = btn
                                            .ancestor(gtk::Popover::static_type())
                                            .and_downcast::<gtk::Popover>()
                                        {
                                            pop.popdown();
                                        }
                                        let _ = sender.input_sender().send(
                                            ChatTabInput::PickAttachment(
                                                tina_core::MediaKind::Image,
                                            ),
                                        );
                                    },
                                },
                                gtk::Button {
                                    set_label: "Video",
                                    add_css_class: "flat",
                                    connect_clicked[sender] => move |btn| {
                                        if let Some(pop) = btn
                                            .ancestor(gtk::Popover::static_type())
                                            .and_downcast::<gtk::Popover>()
                                        {
                                            pop.popdown();
                                        }
                                        let _ = sender.input_sender().send(
                                            ChatTabInput::PickAttachment(
                                                tina_core::MediaKind::Video,
                                            ),
                                        );
                                    },
                                },
                                gtk::Button {
                                    set_label: "Audio file",
                                    add_css_class: "flat",
                                    connect_clicked[sender] => move |btn| {
                                        if let Some(pop) = btn
                                            .ancestor(gtk::Popover::static_type())
                                            .and_downcast::<gtk::Popover>()
                                        {
                                            pop.popdown();
                                        }
                                        let _ = sender.input_sender().send(
                                            ChatTabInput::PickAttachment(
                                                tina_core::MediaKind::Audio,
                                            ),
                                        );
                                    },
                                },
                                gtk::Button {
                                    set_label: "Sticker (file)",
                                    add_css_class: "flat",
                                    connect_clicked[sender] => move |btn| {
                                        if let Some(pop) = btn
                                            .ancestor(gtk::Popover::static_type())
                                            .and_downcast::<gtk::Popover>()
                                        {
                                            pop.popdown();
                                        }
                                        let _ = sender.input_sender().send(
                                            ChatTabInput::PickAttachment(
                                                tina_core::MediaKind::Sticker,
                                            ),
                                        );
                                    },
                                },
                                gtk::Button {
                                    set_label: "Document",
                                    add_css_class: "flat",
                                    connect_clicked[sender] => move |btn| {
                                        if let Some(pop) = btn
                                            .ancestor(gtk::Popover::static_type())
                                            .and_downcast::<gtk::Popover>()
                                        {
                                            pop.popdown();
                                        }
                                        let _ = sender.input_sender().send(
                                            ChatTabInput::PickAttachment(
                                                tina_core::MediaKind::Document,
                                            ),
                                        );
                                    },
                                },
                            },
                        },
                    },

                    // Sticker picker. The Popover stores the catalog
                    // FlowBox; we cache widget refs on `model` so
                    // `StickersLoaded` can repaint without rebuilding
                    // the popover hierarchy.
                    #[name(sticker_picker_btn)]
                    gtk::Button {
                        set_icon_name: "sticker-regular-symbolic",
                        set_tooltip_text: Some("Stickers"),
                        set_valign: gtk::Align::Center,
                        connect_clicked => ChatTabInput::OpenStickerPicker,
                    },

                    #[name(composer_entry)]
                    gtk::Entry {
                        set_buffer: &model.composer_buffer,
                        set_hexpand: true,
                        set_placeholder_text: Some("Message…"),
                        connect_activate => ChatTabInput::Send,
                    },

                    // Voice-record toggle. Tap to start, tap again to
                    // stop — tracking active state via a Cell so the
                    // icon + tooltip can flip without a full
                    // ChatTabInput round trip.
                    gtk::Button {
                        #[watch]
                        set_icon_name: if model.recording_active.get() {
                            "media-playback-stop-symbolic"
                        } else {
                            "mic-3-symbolic"
                        },
                        #[watch]
                        set_tooltip_text: Some(if model.recording_active.get() {
                            "Stop recording"
                        } else {
                            "Record voice note"
                        }),
                        #[watch]
                        set_css_classes: if model.recording_active.get() {
                            &["destructive-action"]
                        } else {
                            &[]
                        },
                        connect_clicked => ChatTabInput::ToggleRecord,
                    },

                    gtk::Button {
                        // Paper-plane bundled from relm4-icons. The
                        // older curved-arrow asset is kept in
                        // build.rs for compatibility but the WhatsApp
                        // mental model maps better to a send icon.
                        set_icon_name: "paper-plane-symbolic",
                        set_tooltip_text: Some("Send"),
                        add_css_class: "suggested-action",
                        connect_clicked => ChatTabInput::Send,
                    },
                },

                add_named[Some("readonly")] = &gtk::Box {
                    set_orientation: gtk::Orientation::Horizontal,
                    set_halign: gtk::Align::Center,
                    set_margin_top: 12,
                    set_margin_bottom: 12,
                    set_margin_start: 12,
                    set_margin_end: 12,
                    set_spacing: 8,

                    gtk::Image {
                        set_icon_name: Some("channel-secure-symbolic"),
                        add_css_class: "dim-label",
                    },
                    gtk::Label {
                        #[watch]
                        set_label: model.read_only_label(),
                        add_css_class: "dim-label",
                    },
                },
            },
        }
    }

    fn init(
        init: Self::Init,
        _root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // Virtualised typed list view — only realises widget trees
        // for rows currently in the viewport. Replaces the previous
        // FactoryVecDeque<MessageBubble> + gtk::ListBox setup.
        let mut list: TypedListView<MessageRowItem, gtk::NoSelection> =
            TypedListView::new();
        // NoSelection has no select/unselect plumbing — the timeline
        // doesn't have a "selected message" affordance, which is why
        // we picked this selection model in the first place.
        let _ = &mut list;
        let ui_state = RowUiInventory::new();
        let row_sender = sender.input_sender().clone();

        // Seed with initial history. The collapse cursor is purely
        // local to this loop — for any subsequent Append/Send we
        // re-read the trailing item directly from the list view,
        // keeping the list as the source of truth.
        let mut last_sender: Option<String> = None;
        let mut last_ts: Option<i64> = None;
        let mut last_day: Option<String> = None;
        let mut avatar_fetches: Vec<String> = Vec::new();
        let init_chat_ctx = super::build::ChatContext {
            kind: init.kind.clone(),
            display_name: if init.name.is_empty() {
                None
            } else {
                Some(init.name.clone())
            },
            avatar_path: init.avatars.get(&init.chat_id),
        };
        for row in &init.initial {
            let collapsed = collapse_against(row, &mut last_sender, &mut last_ts);
            let day_flip = day_flips(row, &mut last_day);
            let mut item = build_item(
                row,
                collapsed,
                &init.avatars,
                &init.media,
                &init.mentions,
                init.user_jid.as_ref().map(|x| x.raw()),
                &init_chat_ctx,
                &mut |jid| avatar_fetches.push(jid),
            );
            if day_flip {
                item.is_first_of_day = true;
                item.day_label = crate::time::format_day_divider(row.timestamp);
                // First of a new day breaks any avatar/header
                // collapse — the day pill needs a fresh cozy
                // header above it, otherwise the divider sits
                // visually attached to the prior day's bubble.
                item.is_collapsed = false;
            }
            list.append(MessageRowItem::new(
                item,
                init.avatars.clone(),
                init.media.clone(),
                ui_state.clone(),
                row_sender.clone(),
            ));
        }
        // Drain after the guard drops — Sender::send is cheap but we
        // don't want it to interleave with factory pushes.
        for jid in avatar_fetches {
            let _ = sender.output(ChatTabOutput::RequestFetchAvatar(
                tina_core::WaIdentity::parse(&jid),
            ));
        }

        let mut seen: HashSet<String> = HashSet::with_capacity(init.initial.len());
        for r in &init.initial {
            seen.insert(r.message_id.clone());
        }

        let oldest_ts = init.initial.iter().map(|r| r.timestamp).min();
        let newest_ts = init.initial.iter().map(|r| r.timestamp).max();

        let bottomed = Rc::new(Cell::new(true));
        let updated_value = Rc::new(Cell::new(false));
        let scroll_lock = Rc::new(Cell::new(false));

        // The local seed cursors are dropped here — `factory_tail_cursor()`
        // is what every subsequent collapse decision queries.
        let _ = (last_sender, last_ts);

        let mut model = ChatTab {
            chat_id: init.chat_id,
            name: init.name,
            kind: init.kind,
            list,
            composer_buffer: gtk::EntryBuffer::default(),
            avatars: init.avatars,
            media: init.media,
            mentions: init.mentions,
            ui_state,
            pending_mentions: HashSet::new(),
            mention_popover: None,
            user_jid: init.user_jid,
            scroll: None,
            sender_handle: row_sender,
            seen_message_ids: seen,
            last_send: None,
            oldest_ts,
            loading_older: false,
            reached_top: false,
            newest_ts,
            loading_newer: false,
            // Initial page from `OpenChat` always pulls the newest 50;
            // therefore the list's tail starts as the DB's actual
            // tail. Live `MessagesAppended` keeps it that way until a
            // soft-cap trim drops the newest rows from the list.
            reached_bottom: true,
            pending_echoes: HashMap::new(),
            pending_media_echoes: HashMap::new(),
            bottomed: bottomed.clone(),
            updated_value: updated_value.clone(),
            scroll_lock: scroll_lock.clone(),
            recorder: None,
            recording_active: Rc::new(Cell::new(false)),
            sticker_popover: None,
            sticker_grid: None,
        };

        let list_view = &model.list.view;
        let widgets = view_output!();
        model.scroll = Some(widgets.scroll.clone());

        // Build the sticker-picker popover post-view so we can
        // anchor it to the live button. Stays empty until the
        // first OpenStickerPicker triggers a worker fetch.
        let sticker_grid = gtk::FlowBox::builder()
            .min_children_per_line(4)
            .max_children_per_line(4)
            .selection_mode(gtk::SelectionMode::None)
            .row_spacing(4)
            .column_spacing(4)
            .homogeneous(true)
            .build();
        let scrolled = gtk::ScrolledWindow::builder()
            .min_content_width(380)
            .min_content_height(280)
            .child(&sticker_grid)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let sticker_popover = gtk::Popover::builder()
            .child(&scrolled)
            .position(gtk::PositionType::Top)
            .has_arrow(true)
            .build();
        sticker_popover.set_parent(&widgets.sticker_picker_btn);
        model.sticker_popover = Some(sticker_popover);
        model.sticker_grid = Some(sticker_grid);

        // Ctrl+V on the composer pastes images straight into the
        // attach-preview flow. Default GtkEntry paste only handles
        // text, so we install a capture-phase key controller that
        // checks the clipboard for image content first.
        super::clipboard_paste::wire_paste(
            &widgets.composer_entry,
            sender.input_sender().clone(),
        );

        // `@`-mention autocomplete popover. Constructed lazily so
        // the entry widget exists before `set_parent`. Seeded with
        // the candidate list the inventory already has for this
        // chat (populated by the OpenChat handler before the tab
        // launched), so a freshly-opened group already filters
        // without round-tripping the worker.
        let mention_popover = crate::components::mention_popover::MentionPopover::new(
            &widgets.composer_entry,
            model.avatars.clone(),
            sender.input_sender().clone(),
        );
        mention_popover.set_candidates(model.mentions.candidates_for(&model.chat_id));
        model.mention_popover = Some(mention_popover);

        wire_changed(&widgets.scroll, bottomed.clone(), updated_value.clone());
        wire_value_changed(
            &widgets.scroll,
            sender.input_sender().clone(),
            bottomed,
            updated_value,
            scroll_lock,
        );

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: ChatTabInput, sender: ComponentSender<Self>) {
        self.dispatch(msg, sender);
    }
}
