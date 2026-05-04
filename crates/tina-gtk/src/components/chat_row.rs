// One row in the sidebar's chat list. `ChatRowItem` is the data; it
// implements `relm4::typed_view::RelmListItem` so a
// `TypedListView<ChatRowItem, _>` can host it directly — the relm4
// abstraction owns the GObject boxing, factory plumbing and bind
// lifecycle, so this module no longer needs `unsafe { qdata }` plumbing
// or a hand-rolled `glib::Object` subclass.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;
use std::sync::OnceLock;

use adw::prelude::*;
use relm4::Sender;
use relm4::typed_view::list::RelmListItem;

use tina_db::ChatRow;

use crate::components::sidebar::SidebarInput;
use crate::time::format_chat_timestamp;

#[derive(Debug, Clone)]
pub struct ChatRowItem {
    pub chat_id: String,
    pub kind: String,
    pub name: String,
    pub preview: String,
    pub timestamp: String,
    pub last_ts: i64,
    pub unread: i64,
    pub pinned: bool,
    pub avatar_path: Option<String>,
    /// `true` when the chat currently has a tab open in the chat area.
    /// Drives both the sort key (active chats float to the top) and the
    /// `tina-tab-open` CSS class for the visual highlight.
    pub is_active: bool,
}

impl ChatRowItem {
    pub fn from_row(row: &ChatRow) -> Self {
        let raw = row.last_message_preview.clone().unwrap_or_default();
        let mtype = row.last_message_type.as_deref().unwrap_or("");
        let preview = match mtype {
            "image" => "📷 Foto".to_string(),
            "audio" => match row.last_message_duration_secs {
                Some(s) if s > 0 => format!("🎤 {}:{:02}", s / 60, s % 60),
                _ => "🎤 Mensagem de voz".to_string(),
            },
            "video" => match row.last_message_duration_secs {
                Some(s) if s > 0 => format!("🎬 Vídeo {}:{:02}", s / 60, s % 60),
                _ => "🎬 Vídeo".to_string(),
            },
            "sticker" => "🎴 Figurinha".to_string(),
            "document" => "📄 Documento".to_string(),
            "contact" => "👤 Contato".to_string(),
            "location" => "📍 Localização".to_string(),
            _ => match raw.as_str() {
                "[Image]" => "📷 Foto".to_string(),
                "[Audio]" => "🎤 Mensagem de voz".to_string(),
                "[Video]" => "🎬 Vídeo".to_string(),
                "[Sticker]" => "🎴 Figurinha".to_string(),
                "[Document]" => "📄 Documento".to_string(),
                "[Contact]" => "👤 Contato".to_string(),
                "[Location]" => "📍 Localização".to_string(),
                "[Live Location]" => "📍 Localização em tempo real".to_string(),
                other => other.to_string(),
            },
        };
        let preview = if row.last_message_from_me && !preview.is_empty() {
            format!("Você: {preview}")
        } else {
            preview
        };
        let last_ts = row.last_message_ts.unwrap_or(0);
        Self {
            chat_id: row.chat_id.clone(),
            kind: row.kind.clone(),
            name: crate::format::format_jid_or_phone(if row.name.is_empty() {
                &row.chat_id
            } else {
                &row.name
            }),
            preview,
            timestamp: format_chat_timestamp(last_ts),
            last_ts,
            unread: row.unread_count,
            pinned: row.pinned,
            avatar_path: row.avatar_path.clone(),
            is_active: false,
        }
    }
}

// Sort order: pinned first → active (currently in a tab) next →
// newest next → alpha last. Reverse-compare bools so `true` floats
// first. The pinned-before-active ordering matches what users expect
// from messengers like Telegram/WhatsApp — explicit pins outrank
// transient "I happen to be chatting here right now".
impl Ord for ChatRowItem {
    fn cmp(&self, other: &Self) -> Ordering {
        match other.pinned.cmp(&self.pinned) {
            Ordering::Equal => {}
            o => return o,
        }
        match other.is_active.cmp(&self.is_active) {
            Ordering::Equal => {}
            o => return o,
        }
        match other.last_ts.cmp(&self.last_ts) {
            Ordering::Equal => {}
            o => return o,
        }
        self.name.cmp(&other.name)
    }
}
impl PartialOrd for ChatRowItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for ChatRowItem {
    fn eq(&self, other: &Self) -> bool {
        self.chat_id == other.chat_id
    }
}
impl Eq for ChatRowItem {}

// ── RelmListItem impl: setup/bind/unbind ─────────────────────────────

/// Holds the per-row state the sidebar needs to find at bind/unbind
/// time. The factory's setup runs once per recycled list-item widget,
/// so we keep the gesture+popover plumbing here and just refresh
/// `menu_target` on each bind.
pub struct ChatRowWidgets {
    pub avatar: adw::Avatar,
    pub name: gtk::Label,
    pub timestamp: gtk::Label,
    pub preview: gtk::Label,
    pub badge: gtk::Label,
    pub pin_icon: gtk::Image,
    /// Shared with the per-row context-menu's gesture closures. Bind
    /// updates this to point at the row's current chat; unbind clears
    /// it so a stale popover can't act on a recycled widget.
    pub menu_target: Rc<RefCell<Option<RowMenuTarget>>>,
}

#[derive(Clone, Debug)]
pub struct RowMenuTarget {
    pub chat_id: String,
    pub pinned: bool,
}

impl RelmListItem for ChatRowItem {
    type Root = gtk::Box;
    type Widgets = ChatRowWidgets;

    fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            // Boxes are hexpand=false by default — without this, our
            // background CSS would only colour the area occupied by the
            // children (a thin strip) instead of the row's full width.
            .hexpand(true)
            .build();

        let avatar = adw::Avatar::builder()
            .size(40)
            .show_initials(true)
            .build();
        root.append(&avatar);

        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        root.append(&body);

        let top = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        body.append(&top);

        let name = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        name.add_css_class("heading");
        top.append(&name);

        let timestamp = gtk::Label::new(None);
        timestamp.add_css_class("dim-label");
        timestamp.add_css_class("caption");
        top.append(&timestamp);

        let bottom = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        body.append(&bottom);

        let preview = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .wrap(false)
            .lines(1)
            .single_line_mode(true)
            .max_width_chars(30)
            .build();
        preview.add_css_class("dim-label");
        preview.add_css_class("caption");
        bottom.append(&preview);

        let pin_icon = gtk::Image::from_icon_name("view-pin-symbolic");
        pin_icon.add_css_class("dim-label");
        pin_icon.set_visible(false);
        bottom.append(&pin_icon);

        let badge = gtk::Label::new(None);
        badge.add_css_class("accent");
        badge.add_css_class("caption-heading");
        badge.set_visible(false);
        bottom.append(&badge);

        // Per-row context menu. The target chat is updated at bind time
        // through the shared cell; gesture handlers see whatever the
        // most recent bind put there.
        let menu_target: Rc<RefCell<Option<RowMenuTarget>>> = Rc::new(RefCell::new(None));
        attach_context_menu(&root, menu_target.clone());

        let widgets = ChatRowWidgets {
            avatar,
            name,
            timestamp,
            preview,
            badge,
            pin_icon,
            menu_target,
        };
        (root, widgets)
    }

    fn bind(&mut self, widgets: &mut Self::Widgets, root: &mut Self::Root) {
        widgets.avatar.set_text(Some(&self.name));
        let avatar_paintable: Option<gtk::gdk::Paintable> = self
            .avatar_path
            .as_deref()
            .and_then(|p| gtk::gdk::Texture::from_filename(p).ok())
            .map(|t| t.upcast());
        widgets.avatar.set_custom_image(avatar_paintable.as_ref());

        widgets.name.set_label(&self.name);
        widgets.timestamp.set_label(&self.timestamp);
        widgets.preview.set_label(&self.preview);

        if self.unread > 0 {
            widgets.badge.set_label(&format!("{}", self.unread));
            widgets.badge.set_visible(true);
            widgets.pin_icon.set_visible(false);
        } else if self.pinned {
            widgets.badge.set_visible(false);
            widgets.pin_icon.set_visible(true);
        } else {
            widgets.badge.set_visible(false);
            widgets.pin_icon.set_visible(false);
        }

        if self.is_active {
            root.add_css_class(CSS_TAB_OPEN);
        } else {
            root.remove_css_class(CSS_TAB_OPEN);
        }

        *widgets.menu_target.borrow_mut() = Some(RowMenuTarget {
            chat_id: self.chat_id.clone(),
            pinned: self.pinned,
        });
    }

    fn unbind(&mut self, widgets: &mut Self::Widgets, _root: &mut Self::Root) {
        *widgets.menu_target.borrow_mut() = None;
    }
}

// ── Context menu wiring ──────────────────────────────────────────────

/// Sender registered once at sidebar init; `attach_context_menu`'s
/// click handlers read from it. `OnceLock` is enough for our case
/// since the app only ever has one sidebar.
static CONTEXT_MENU_SENDER: OnceLock<Sender<SidebarInput>> = OnceLock::new();

/// Wire the per-row sender so right-click context menus can dispatch
/// `SidebarInput`s. Idempotent — call once on sidebar startup.
pub fn install_context_menu_sender(sender: Sender<SidebarInput>) {
    let _ = CONTEXT_MENU_SENDER.set(sender);
}

fn attach_context_menu(root: &gtk::Box, target: Rc<RefCell<Option<RowMenuTarget>>>) {
    let popover = gtk::Popover::builder()
        .has_arrow(false)
        .position(gtk::PositionType::Bottom)
        .build();

    let menu = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .margin_top(4)
        .margin_bottom(4)
        .margin_start(4)
        .margin_end(4)
        .build();

    let mk_id_button = |label: &str,
                        mk_msg: Box<dyn Fn(String) -> SidebarInput + 'static>|
     -> gtk::Button {
        let btn = gtk::Button::with_label(label);
        btn.add_css_class("flat");
        let target = target.clone();
        let pop = popover.clone();
        btn.connect_clicked(move |_| {
            if let (Some(t), Some(sender)) =
                (target.borrow().clone(), CONTEXT_MENU_SENDER.get())
            {
                let _ = sender.send(mk_msg(t.chat_id));
            }
            pop.popdown();
        });
        btn
    };

    menu.append(&mk_id_button(
        "Open",
        Box::new(SidebarInput::OpenChatRequested),
    ));
    menu.append(&mk_id_button(
        "Open in new tab",
        Box::new(SidebarInput::OpenInNewTabRequested),
    ));

    let pin_btn = gtk::Button::with_label("Pin");
    pin_btn.add_css_class("flat");
    {
        let target = target.clone();
        let pop = popover.clone();
        pin_btn.connect_clicked(move |_| {
            if let (Some(t), Some(sender)) =
                (target.borrow().clone(), CONTEXT_MENU_SENDER.get())
            {
                let _ = sender.send(SidebarInput::PinChatRequested {
                    chat_id: t.chat_id,
                    pinned: !t.pinned,
                });
            }
            pop.popdown();
        });
    }
    menu.append(&pin_btn);

    popover.set_child(Some(&menu));
    popover.set_parent(root);

    let gesture = gtk::GestureClick::new();
    gesture.set_button(gtk::gdk::BUTTON_SECONDARY);
    let pop = popover.clone();
    let target_for_gesture = target.clone();
    let pin_btn_for_gesture = pin_btn.clone();
    gesture.connect_pressed(move |_, _, x, y| {
        if let Some(t) = target_for_gesture.borrow().clone() {
            pin_btn_for_gesture.set_label(if t.pinned { "Unpin" } else { "Pin" });
        }
        let rect = gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1);
        pop.set_pointing_to(Some(&rect));
        pop.popup();
    });
    root.add_controller(gesture);
}

// ── CSS ──────────────────────────────────────────────────────────────

const CSS_TAB_OPEN: &str = "tina-tab-open";

/// Active rows (chats currently open in a tab) get a small accent
/// ring around the avatar — the same indicator paper-plane uses for
/// its `.selected-avatar` state. Subtle, doesn't fight with GTK's
/// own `:selected` row styling, and stays out of the way of pinned/
/// unread badge real estate on the right edge.
pub const CHAT_ROW_CSS: &str = "
.tina-tab-open avatar {
  outline: 2px solid @accent_color;
  outline-offset: 2px;
}
";

