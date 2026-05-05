// Per-row widget bundle + the `RelmListItem` setup/bind/unbind impl.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use relm4::typed_view::list::RelmListItem;

use super::context_menu::{attach_context_menu, RowMenuTarget};
use super::css::CSS_TAB_OPEN;
use super::item::ChatRowItem;

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

impl RelmListItem for ChatRowItem {
    type Root = gtk::Box;
    type Widgets = ChatRowWidgets;

    fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
        let root = build_root();

        // 44px avatars — sized between Fractal's tight 32px sidebar
        // and Telegram-style 54px, matches WhatsApp Desktop's "tall
        // enough to read at a glance, narrow enough to fit a 320-px
        // sidebar" middle ground.
        let avatar = adw::Avatar::builder().size(44).show_initials(true).build();
        root.append(&avatar);

        let body = build_body();
        root.append(&body);

        let (name, timestamp) = build_top_row(&body);
        let (preview, pin_icon, badge) = build_bottom_row(&body);

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
            .avatars
            .load_texture(self.avatar_path.as_deref())
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

fn build_root() -> gtk::Box {
    gtk::Box::builder()
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
        .build()
}

fn build_body() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build()
}

fn build_top_row(body: &gtk::Box) -> (gtk::Label, gtk::Label) {
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

    (name, timestamp)
}

fn build_bottom_row(body: &gtk::Box) -> (gtk::Label, gtk::Image, gtk::Label) {
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
    badge.add_css_class("tina-unread-badge");
    badge.set_valign(gtk::Align::Center);
    badge.set_visible(false);
    bottom.append(&badge);

    (preview, pin_icon, badge)
}
