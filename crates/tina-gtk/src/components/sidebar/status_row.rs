// Sidebar status author rows. Aggregated from `status@broadcast`
// messages — each row is one contact who's posted a status the user
// can read. Rendered inside the sidebar's "status" stack page when
// the Status filter tab is active.
//
// The row layout mirrors WhatsApp's: avatar with a coloured ring
// indicating "has unviewed posts", contact display name, post-count
// + relative timestamp on the right.

use std::cmp::Ordering;
use std::sync::Arc;

use adw::prelude::*;
use relm4::typed_view::list::RelmListItem;

use tina_db::StatusAuthorRow;

use crate::inventory::AvatarInventory;
use crate::time::format_chat_timestamp;

#[derive(Clone)]
pub struct StatusAuthorItem {
    pub sender_jid: String,
    pub name: String,
    pub avatar_path: Option<String>,
    pub last_ts: i64,
    pub timestamp: String,
    pub post_count: i64,
    pub avatars: AvatarInventory,
}

impl StatusAuthorItem {
    pub fn from_row(row: &StatusAuthorRow, avatars: AvatarInventory) -> Self {
        let mut avatar_path = row.avatar_path.clone();
        if avatar_path.is_none()
            && let Some(p) = avatars.get(&row.sender_jid) {
                avatar_path = Some(p);
            }
        Self {
            sender_jid: row.sender_jid.clone(),
            name: crate::format::format_jid_or_phone(&row.name),
            avatar_path,
            last_ts: row.last_ts,
            timestamp: format_chat_timestamp(row.last_ts),
            post_count: row.post_count,
            avatars,
        }
    }

}

/// Recycled-row widget bundle. Two-line layout matching the chat
/// list's spacing/sizing — name (heading) on top, "N posts · HH:MM"
/// (caption dim-label) on the bottom. The "Photo" / "Video" preview
/// from the original draft was redundant with the post count and
/// crowded the row at narrow widths.
pub struct StatusAuthorWidgets {
    avatar: adw::Avatar,
    name_label: gtk::Label,
    sub_label: gtk::Label,
}

impl RelmListItem for StatusAuthorItem {
    type Root = gtk::Box;
    type Widgets = StatusAuthorWidgets;

    fn setup(_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            .hexpand(true)
            .build();

        let avatar = adw::Avatar::builder()
            .size(40)
            .show_initials(true)
            .valign(gtk::Align::Center)
            .build();
        // Story ring marking unviewed posts. CSS-only — see
        // `chat_row::CHAT_ROW_CSS` for the `box-shadow` rule.
        avatar.add_css_class("tina-status-ring");
        root.append(&avatar);

        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        root.append(&body);

        let name_label = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        name_label.add_css_class("heading");
        body.append(&name_label);

        let sub_label = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .single_line_mode(true)
            .build();
        sub_label.add_css_class("dim-label");
        sub_label.add_css_class("caption");
        body.append(&sub_label);

        (
            root,
            StatusAuthorWidgets {
                avatar,
                name_label,
                sub_label,
            },
        )
    }

    fn bind(&mut self, widgets: &mut Self::Widgets, _root: &mut Self::Root) {
        widgets.avatar.set_text(Some(&self.name));
        let texture = self
            .avatars
            .load_texture(self.avatar_path.as_deref())
            .map(|t| t.upcast::<gtk::gdk::Paintable>());
        widgets.avatar.set_custom_image(texture.as_ref());
        widgets.name_label.set_label(&self.name);
        widgets.sub_label.set_label(&format!(
            "{} post{} · {}",
            self.post_count,
            if self.post_count == 1 { "" } else { "s" },
            self.timestamp,
        ));
    }
}

/// Newest-post-first ordering. The aggregate query already returns
/// rows in this order but the SortListModel re-sorts, so we encode
/// the same predicate here.
impl Ord for StatusAuthorItem {
    fn cmp(&self, other: &Self) -> Ordering {
        other.last_ts.cmp(&self.last_ts)
    }
}
impl PartialOrd for StatusAuthorItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for StatusAuthorItem {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for StatusAuthorItem {}

/// Dummy reference so the cargo dead-code lint doesn't drop the type
/// when the typed-view crate is the sole import.
#[allow(dead_code)]
fn _arc_marker(_: Arc<StatusAuthorItem>) {}
