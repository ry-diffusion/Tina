// Profile menu button + popover that lives at the start of the sidebar
// headerbar. Owns the signed-in user's identity (name/phone/avatar) so
// the sidebar doesn't have to thread those fields through its view.
//
// Outputs are user intents — repair, logout — which the sidebar bubbles
// up to the parent.

use adw::prelude::*;
use relm4::prelude::*;

#[derive(Debug)]
pub enum ProfileMenuInput {
    SetIdentity {
        phone: Option<String>,
        jid: Option<String>,
        push_name: Option<String>,
    },
    /// Avatar arrived for the signed-in user.
    SetAvatar(String),
    Preferences,
    Logout,
    /// Reserved: the popover used to disable the Repair button while a
    /// reconcile was in flight. Repair now lives in the preferences
    /// dialog, so this is a no-op kept for callers that still send it.
    #[allow(dead_code)]
    SetRepairing(bool),
}

#[derive(Debug)]
pub enum ProfileMenuOutput {
    Preferences,
    Logout,
}

pub struct ProfileMenu {
    phone: Option<String>,
    jid: Option<String>,
    name: Option<String>,
    avatar: Option<String>,
    repairing: bool,
}

impl ProfileMenu {
    pub fn jid(&self) -> Option<&str> {
        self.jid.as_deref()
    }
}

#[relm4::component(pub)]
impl SimpleComponent for ProfileMenu {
    type Init = ();
    type Input = ProfileMenuInput;
    type Output = ProfileMenuOutput;

    view! {
        #[root]
        gtk::MenuButton {
            add_css_class: "flat",
            add_css_class: "circular",
            set_tooltip_text: Some("Profile"),

            #[wrap(Some)]
            set_child = &adw::Avatar {
                set_size: 28,
                set_show_initials: true,
                #[watch]
                set_text: Some(model.display_name()),
                #[watch]
                set_custom_image: model.avatar_paintable().as_ref(),
            },

            #[wrap(Some)]
            set_popover = &gtk::Popover {
                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_spacing: 12,
                    set_margin_top: 12,
                    set_margin_bottom: 12,
                    set_margin_start: 12,
                    set_margin_end: 12,
                    set_width_request: 240,

                    // Identity row: avatar on the left, name + phone
                    // stacked on the right. Reads as a single line of
                    // info instead of three centred blocks — closer
                    // to GNOME Online Accounts / Text Editor's
                    // identity affordances. Phone is selectable but
                    // `can_focus: false` keeps the popover from
                    // auto-selecting it on present.
                    gtk::Box {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_spacing: 12,
                        set_halign: gtk::Align::Fill,

                        adw::Avatar {
                            set_size: 48,
                            set_show_initials: true,
                            #[watch]
                            set_text: Some(model.display_name()),
                            #[watch]
                            set_custom_image: model.avatar_paintable().as_ref(),
                            set_valign: gtk::Align::Center,
                        },

                        gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_spacing: 2,
                            set_valign: gtk::Align::Center,
                            set_hexpand: true,

                            gtk::Label {
                                #[watch]
                                set_label: model.name.as_deref().unwrap_or("Tina"),
                                set_halign: gtk::Align::Start,
                                set_xalign: 0.0,
                                set_ellipsize: gtk::pango::EllipsizeMode::End,
                                add_css_class: "heading",
                            },

                            gtk::Label {
                                #[watch]
                                set_label: model.phone.as_deref().unwrap_or("Not connected"),
                                set_halign: gtk::Align::Start,
                                set_xalign: 0.0,
                                set_ellipsize: gtk::pango::EllipsizeMode::End,
                                set_selectable: true,
                                set_can_focus: false,
                                add_css_class: "dim-label",
                                add_css_class: "caption",
                            },
                        },
                    },

                    gtk::Separator {},

                    // Menu rows styled like AdwPreferencesDialog /
                    // GNOME Text Editor's menu — flat button with the
                    // action on the start and the keyboard accel
                    // dim-labelled on the end. Accel labels mirror the
                    // ShortcutController bindings installed on the
                    // application window in `app/component.rs`.
                    gtk::Button {
                        add_css_class: "flat",
                        connect_clicked => ProfileMenuInput::Preferences,
                        gtk::Box {
                            set_orientation: gtk::Orientation::Horizontal,
                            set_spacing: 24,
                            gtk::Label {
                                set_label: "Preferences",
                                set_xalign: 0.0,
                                set_hexpand: true,
                            },
                            gtk::Label {
                                set_label: "Ctrl+,",
                                add_css_class: "dim-label",
                                add_css_class: "caption",
                            },
                        },
                    },

                    gtk::Button {
                        add_css_class: "flat",
                        add_css_class: "destructive-action",
                        connect_clicked => ProfileMenuInput::Logout,
                        gtk::Box {
                            set_orientation: gtk::Orientation::Horizontal,
                            set_spacing: 24,
                            gtk::Label {
                                set_label: "Log out",
                                set_xalign: 0.0,
                                set_hexpand: true,
                            },
                        },
                    },
                },
            },
        }
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = ProfileMenu {
            phone: None,
            jid: None,
            name: None,
            avatar: None,
            repairing: false,
        };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: ProfileMenuInput, sender: ComponentSender<Self>) {
        match msg {
            ProfileMenuInput::SetIdentity {
                phone,
                jid,
                push_name,
            } => {
                let formatted_phone = phone.as_deref().map(crate::format::format_jid_or_phone);
                self.phone = formatted_phone.clone();
                self.jid = jid;
                self.name = push_name.or(formatted_phone);
            }
            ProfileMenuInput::SetAvatar(path) => self.avatar = Some(path),
            ProfileMenuInput::SetRepairing(r) => self.repairing = r,
            ProfileMenuInput::Preferences => {
                let _ = sender.output(ProfileMenuOutput::Preferences);
            }
            ProfileMenuInput::Logout => {
                let _ = sender.output(ProfileMenuOutput::Logout);
            }
        }
    }
}

impl ProfileMenu {
    fn display_name(&self) -> &str {
        self.name
            .as_deref()
            .or(self.phone.as_deref())
            .unwrap_or("Tina")
    }

    fn avatar_paintable(&self) -> Option<gtk::gdk::Paintable> {
        self.avatar
            .as_deref()
            .and_then(|p| gtk::gdk::Texture::from_filename(p).ok())
            .map(|t| t.upcast::<gtk::gdk::Paintable>())
    }
}
