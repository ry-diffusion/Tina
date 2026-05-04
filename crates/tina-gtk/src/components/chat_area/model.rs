// State for the chat area: two panes, a tab → controller registry, and
// the user's identity used to seed new tabs.

use std::collections::HashMap;

use adw::prelude::*;
use relm4::Controller;

use crate::components::chat_tab::ChatTab;
use crate::inventory::{AvatarInventory, ChatInventory, MediaInventory, MessageInventory};

use super::pane::Pane;

pub struct ChatArea {
    /// Two panes; pane 1's `toolbar_view` is hidden when empty so the
    /// Paned collapses visually to a single pane.
    pub(super) panes: [Pane; 2],
    /// chat_id -> (controller, page, pane_idx).
    pub(super) open_tabs: HashMap<String, (Controller<ChatTab>, adw::TabPage, usize)>,
    pub(super) chat_meta: HashMap<String, (String, String)>,
    pub(super) paned: gtk::Paned,
    /// Revealer wrapping pane 1, used to slide the second split in/out
    /// instead of toggling visibility instantly.
    pub(super) pane1_revealer: gtk::Revealer,
    /// Pane that receives "open in current" clicks and new chats. Updated
    /// whenever a tab is selected in a pane (selection ⇒ implicit focus).
    pub(super) focused_pane: usize,
    pub(super) avatars: AvatarInventory,
    pub(super) media: MediaInventory,
    pub(super) chats: ChatInventory,
    #[allow(dead_code)] // first consumer lands with reply rendering
    pub(super) messages: MessageInventory,
    pub(super) user_jid: Option<tina_core::WaIdentity>,
}

impl ChatArea {
    pub(super) fn pane_tab_count(&self, idx: usize) -> i32 {
        self.panes[idx].tab_view.n_pages()
    }

    /// Snapshot the open chat_ids and emit them so the parent can
    /// forward to the sidebar (which highlights + sorts active chats
    /// to the top). Cheap; called only on tab open/close.
    pub(super) fn broadcast_active_tabs(
        &self,
        sender: &relm4::ComponentSender<Self>,
    ) {
        let ids: Vec<String> = self.open_tabs.keys().cloned().collect();
        let _ = sender.output(super::messages::ChatAreaOutput::ActiveTabsChanged(ids));
    }

    /// Reveal pane 1 only when it has tabs (so the Paned divider
    /// disappears too). Also keeps the window's close button visible on
    /// exactly one header — whichever pane is the rightmost-visible one
    /// — so the X never accidentally lives on a header that the user
    /// might mistake for "close this tab".
    pub(super) fn refresh_pane_visibility(&self) {
        let p1_visible = self.pane_tab_count(1) > 0;
        self.panes[0].toolbar_view.set_visible(true);
        // Drive the Revealer instead of toolbar_view's visibility directly
        // — the Revealer slides the pane in/out and resizes the Paned
        // divider along with it.
        self.pane1_revealer.set_reveal_child(p1_visible);
        // Window controls live only on the rightmost visible pane.
        self.panes[0].header.set_show_end_title_buttons(!p1_visible);
        self.panes[1].header.set_show_end_title_buttons(p1_visible);
        // The split-move button needs at least one tab in the source pane.
        self.panes[0]
            .split_btn
            .set_sensitive(self.pane_tab_count(0) > 0);
        self.panes[1]
            .split_btn
            .set_sensitive(self.pane_tab_count(1) > 0);
        // When pane 1 just appeared, give it a sensible starting size.
        if p1_visible {
            let width = self.paned.width();
            if width > 0 && self.paned.position() <= 0 {
                self.paned.set_position(width / 2);
            }
        }
    }

    /// Pick the right child of the title Stack based on tab count, and
    /// repaint the single-tab avatar/name from the cached state.
    pub(super) fn refresh_pane_header(&self, idx: usize) {
        let pane = &self.panes[idx];
        if self.pane_tab_count(idx) >= 2 {
            pane.stack.set_visible_child_name("multi");
        } else {
            pane.stack.set_visible_child_name("single");
            pane.title.set_title(&pane.current_chat_name);
            pane.avatar.set_text(Some(&pane.current_chat_name));
            self.apply_pane_avatar(idx);
        }
    }

    pub(super) fn apply_pane_avatar(&self, idx: usize) {
        let pane = &self.panes[idx];
        let texture = self
            .avatars
            .load_texture(pane.current_chat_avatar.as_deref())
            .map(|t| t.upcast::<gtk::gdk::Paintable>());
        pane.avatar.set_custom_image(texture.as_ref());
    }
}
