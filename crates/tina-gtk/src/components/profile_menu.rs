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
    Repair,
    Logout,
    /// Whether a repair/reconcile is currently in flight — disables the
    /// "Repair" button so the user can't double-trigger it.
    SetRepairing(bool),
}

#[derive(Debug)]
pub enum ProfileMenuOutput {
    Repair,
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

                    gtk::Box {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_spacing: 12,
                        set_halign: gtk::Align::Center,

                        adw::Avatar {
                            set_size: 56,
                            set_show_initials: true,
                            #[watch]
                            set_text: Some(model.display_name()),
                            #[watch]
                            set_custom_image: model.avatar_paintable().as_ref(),
                        },
                    },

                    gtk::Label {
                        #[watch]
                        set_label: model.name.as_deref().unwrap_or("Tina"),
                        set_halign: gtk::Align::Center,
                        add_css_class: "title-2",
                    },

                    gtk::Label {
                        #[watch]
                        set_label: model.phone.as_deref().unwrap_or("Not connected"),
                        set_halign: gtk::Align::Center,
                        set_selectable: true,
                        add_css_class: "dim-label",
                        add_css_class: "caption",
                    },

                    gtk::Separator {},

                    gtk::Button {
                        set_label: "Repair (reconcile)",
                        add_css_class: "flat",
                        #[watch]
                        set_sensitive: !model.repairing,
                        connect_clicked => ProfileMenuInput::Repair,
                    },

                    gtk::Button {
                        set_label: "Log out",
                        add_css_class: "flat",
                        add_css_class: "destructive-action",
                        connect_clicked => ProfileMenuInput::Logout,
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
            ProfileMenuInput::Repair => {
                let _ = sender.output(ProfileMenuOutput::Repair);
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
