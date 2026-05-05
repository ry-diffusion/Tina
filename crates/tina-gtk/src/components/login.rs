// QR-pairing scene. Two-column layout: heading + numbered checklist on
// the left, QR card on the right. Splitting the columns keeps the
// instructions stable when the QR Stack swaps between its loading and
// QR-ready states (any natural-size flutter on the QR side never
// pushes the heading around any more).

use adw::prelude::*;
use crate::fl;
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
                // Wider clamp than the previous single-column layout
                // so heading+checklist (left) and QR card (right) fit
                // side by side without crowding.
                set_maximum_size: 720,
                set_margin_top: 24,
                set_margin_bottom: 24,
                set_margin_start: 24,
                set_margin_end: 24,

                gtk::Box {
                    set_orientation: gtk::Orientation::Horizontal,
                    set_spacing: 36,
                    set_halign: gtk::Align::Center,
                    set_valign: gtk::Align::Center,

                    // ── LEFT COLUMN: heading + numbered checklist ──
                    gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                        set_spacing: 18,
                        set_hexpand: true,
                        set_valign: gtk::Align::Center,

                        gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_spacing: 8,
                            set_halign: gtk::Align::Start,

                            gtk::Image {
                                set_icon_name: Some("phone-symbolic"),
                                set_pixel_size: 48,
                                set_halign: gtk::Align::Start,
                                add_css_class: "dim-label",
                            },
                            gtk::Label {
                                set_label: &fl!("login-link-phone"),
                                set_halign: gtk::Align::Start,
                                add_css_class: "title-2",
                            },
                            gtk::Label {
                                set_label: &fl!("login-scan-qr"),
                                set_halign: gtk::Align::Start,
                                set_xalign: 0.0,
                                set_wrap: true,
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
                                    set_label: &fl!("login-step-1"),
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
                                    set_label: &fl!("login-step-2"),
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
                                    set_label: &fl!("login-step-3"),
                                },
                            },
                        },
                    },

                    // ── RIGHT COLUMN: QR card ──
                    // Isolating the QR into its own column means any
                    // natural-size shimmer between the loading and
                    // QR-ready states stays on this side and never
                    // shifts the heading or checklist.
                    gtk::Frame {
                        set_halign: gtk::Align::End,
                        set_valign: gtk::Align::Center,
                        set_hexpand: false,
                        set_vexpand: false,
                        set_size_request: (244, 244),
                        add_css_class: "card",

                        #[name(qr_stack)]
                        gtk::Stack {
                            // Children must be declared before
                            // `set_visible_child_name`; otherwise the
                            // initial watch tries to switch to a page
                            // that doesn't exist yet → Gtk-WARNING.
                            set_size_request: (220, 220),
                            set_hexpand: false,
                            set_vexpand: false,
                            set_halign: gtk::Align::Fill,
                            set_valign: gtk::Align::Fill,
                            set_transition_type: gtk::StackTransitionType::Crossfade,

                            // Spinner alone, centred in the full 220×220
                            // page — the "Waiting for QR code…" label
                            // pulled the spinner above the geometric
                            // centre because the Box centred the
                            // [spinner, label] group as a whole.
                            add_named[Some("loading")] = &gtk::Spinner {
                                set_size_request: (220, 220),
                                set_hexpand: false,
                                set_vexpand: false,
                                set_halign: gtk::Align::Center,
                                set_valign: gtk::Align::Center,
                                set_spinning: true,
                            },

                            #[name(qr_picture)]
                            add_named[Some("qr")] = &gtk::Picture {
                                set_size_request: (220, 220),
                                set_hexpand: false,
                                set_vexpand: false,
                                set_can_shrink: true,
                                set_content_fit: gtk::ContentFit::Contain,
                                set_halign: gtk::Align::Center,
                                set_valign: gtk::Align::Center,
                                #[watch]
                                set_paintable: model.qr_texture.as_ref().map(|t| t.upcast_ref::<gdk::Paintable>()),
                            },

                            #[watch]
                            set_visible_child_name: if model.qr_texture.is_some() {
                                "qr"
                            } else {
                                "loading"
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
        _sender: ComponentSender<Self>,
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
