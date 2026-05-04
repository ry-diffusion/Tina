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
        let mut messages = FactoryVecDeque::builder()
            .launch(gtk::ListBox::default())
            .forward(sender.input_sender(), |o| match o {
                crate::components::message_bubble::MessageBubbleOut::DownloadRequested(id) => {
                    ChatTabInput::RequestMediaDownload(id)
                }
                crate::components::message_bubble::MessageBubbleOut::JumpToMessage(id) => {
                    ChatTabInput::JumpToMessage(id)
                }
            });

        // Seed with initial history. The collapse cursor is purely
        // local to this loop — for any subsequent Append/Send we
        // re-read the trailing item directly from the factory, keeping
        // the factory as the single source of truth.
        let mut last_sender: Option<String> = None;
        let mut last_ts: Option<i64> = None;
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
        {
            let mut guard = messages.guard();
            for row in &init.initial {
                let collapsed = collapse_against(row, &mut last_sender, &mut last_ts);
                let item = build_item(
                    row,
                    collapsed,
                    &init.avatars,
                    &init.media,
                    init.user_jid.as_ref().map(|x| x.raw()),
                    &init_chat_ctx,
                    &mut |jid| avatar_fetches.push(jid),
                );
                guard.push_back(item);
            }
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
            pending_media_echoes: HashMap::new(),
            bottomed: bottomed.clone(),
            updated_value: updated_value.clone(),
            recorder: None,
            recording_active: Rc::new(Cell::new(false)),
            sticker_popover: None,
            sticker_grid: None,
        };

        let messages_list = model.messages.widget();
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
