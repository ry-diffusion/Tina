// QR-pairing scene. Mirrors the look of GNOME's "phone link" dialogs:
// AdwStatusPage with the QR centred above a numbered checklist.

use adw::prelude::*;
use gtk::gdk;
use relm4::prelude::*;

#[derive(Debug)]
pub enum LoginInput {
    SetQr(String),
    Reset,
}

pub struct LoginPage {
    qr_texture: Option<gdk::Texture>,
}

#[relm4::component(pub)]
impl SimpleComponent for LoginPage {
    type Init = ();
    type Input = LoginInput;
    type Output = ();

    view! {
        adw::ToolbarView {
            add_top_bar = &adw::HeaderBar {
                add_css_class: "flat",
            },
            #[wrap(Some)]
            set_content = &adw::Clamp {
                set_maximum_size: 480,
                set_margin_top: 12,
                set_margin_bottom: 24,
                set_margin_start: 12,
                set_margin_end: 12,

                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_spacing: 18,
                    set_valign: gtk::Align::Center,

                    adw::StatusPage {
                        set_icon_name: Some("phone-symbolic"),
                        set_title: "Link with your phone",
                        set_description: Some(
                            "Scan the QR with WhatsApp on your phone."
                        ),
                    },

                    gtk::Frame {
                        set_halign: gtk::Align::Center,
                        add_css_class: "card",

                        #[name(qr_picture)]
                        gtk::Picture {
                            set_size_request: (256, 256),
                            set_can_shrink: false,
                            #[watch]
                            set_paintable: model.qr_texture.as_ref().map(|t| t.upcast_ref::<gdk::Paintable>()),
                        },
                    },

                    #[name(qr_loading)]
                    gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                        set_spacing: 8,
                        set_halign: gtk::Align::Center,
                        #[watch]
                        set_visible: model.qr_texture.is_none(),

                        gtk::Spinner {
                            set_spinning: true,
                            set_width_request: 24,
                            set_height_request: 24,
                        },
                        gtk::Label {
                            set_label: "Waiting for QR code…",
                            add_css_class: "dim-label",
                        },
                    },

                    gtk::ListBox {
                        set_selection_mode: gtk::SelectionMode::None,
                        add_css_class: "boxed-list",

                        gtk::ListBoxRow {
                            set_activatable: false,
                            gtk::Label {
                                set_xalign: 0.0,
                                set_margin_top: 12,
                                set_margin_bottom: 12,
                                set_margin_start: 14,
                                set_margin_end: 14,
                                set_label: "1.  Open WhatsApp on your phone",
                            },
                        },
                        gtk::ListBoxRow {
                            set_activatable: false,
                            gtk::Label {
                                set_xalign: 0.0,
                                set_margin_top: 12,
                                set_margin_bottom: 12,
                                set_margin_start: 14,
                                set_margin_end: 14,
                                set_wrap: true,
                                set_label: "2.  Tap Menu or Settings and pick Linked Devices",
                            },
                        },
                        gtk::ListBoxRow {
                            set_activatable: false,
                            gtk::Label {
                                set_xalign: 0.0,
                                set_margin_top: 12,
                                set_margin_bottom: 12,
                                set_margin_start: 14,
                                set_margin_end: 14,
                                set_wrap: true,
                                set_label: "3.  Point your phone at the screen",
                            },
                        },
                    },
                },
            },
        }
    }

    fn init(
        _: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = LoginPage { qr_texture: None };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: LoginInput, _sender: ComponentSender<Self>) {
        match msg {
            LoginInput::SetQr(qr) => {
                self.qr_texture = crate::qr::render_qr_texture(&qr);
            }
            LoginInput::Reset => {
                self.qr_texture = None;
            }
        }
    }
}
