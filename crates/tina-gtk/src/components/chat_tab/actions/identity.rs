// Metadata, identity and avatar handlers.

use relm4::ComponentSender;

use super::super::messages::ChatTabOutput;
use super::super::model::ChatTab;

impl ChatTab {
    pub(in crate::components::chat_tab) fn handle_set_meta(
        &mut self,
        name: String,
        kind: String,
    ) {
        self.name = name;
        self.kind = kind;
    }

    pub(in crate::components::chat_tab) fn handle_set_user_jid(
        &mut self,
        new_jid: Option<tina_core::WaIdentity>,
        sender: &ComponentSender<Self>,
    ) {
        self.user_jid = new_jid.clone();
        let Some(jid) = new_jid else {
            return;
        };
        let raw = jid.raw();
        if raw.is_empty() {
            return;
        }
        // Back-fill sender_jid on every existing from_me row + paint
        // the cached avatar if the inventory already has it.
        let cached = self.avatars.get(raw);
        let raw_owned = raw.to_string();
        let cached_for_apply = cached.clone();
        self.update_items_where(
            |it| it.from_me,
            |it| {
                it.sender_jid = Some(raw_owned.clone());
                if let Some(p) = cached_for_apply.clone() {
                    it.sender_avatar_path = Some(p);
                }
            },
        );
        if cached.is_none() && self.avatars.needs_fetch(raw) {
            let _ = sender.output(ChatTabOutput::RequestFetchAvatar(jid));
        }
    }

    pub(in crate::components::chat_tab) fn handle_avatar_ready(
        &mut self,
        jid: tina_core::WaIdentity,
        path: String,
    ) {
        let raw = jid.raw().to_string();
        let raw_for_match = raw.clone();
        self.update_items_where(
            move |it| it.sender_jid.as_deref() == Some(raw_for_match.as_str()),
            |it| {
                it.sender_avatar_path = Some(path.clone());
            },
        );
    }

    /// glycin async decode of a sender's avatar file landed in the
    /// shared cache. Force a rebind of any row showing the same
    /// avatar_path so the bind pass picks up the now-cached texture.
    /// We use `update_items_where` with an identity mutation
    /// because the data didn't change — only the underlying texture
    /// cache did, and re-binding is what reads from it.
    pub(in crate::components::chat_tab) fn handle_avatar_texture_ready(
        &mut self,
        path: &str,
    ) {
        let path_owned = path.to_string();
        self.update_items_where(
            move |it| it.sender_avatar_path.as_deref() == Some(path_owned.as_str()),
            |_| {},
        );
    }
}
