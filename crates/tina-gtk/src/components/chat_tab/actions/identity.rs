// Metadata, identity and avatar handlers.

use relm4::ComponentSender;

use crate::components::message_bubble::MessageBubbleInput;

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
        let indices: Vec<usize> = self
            .messages
            .guard()
            .iter()
            .enumerate()
            .filter_map(|(i, f)| if f.item.from_me { Some(i) } else { None })
            .collect();
        for idx in indices {
            self.messages
                .send(idx, MessageBubbleInput::SetSenderJid(raw.to_string()));
            if let Some(p) = cached.clone() {
                self.messages
                    .send(idx, MessageBubbleInput::SetAvatar(p));
            }
        }
        if cached.is_none() && self.avatars.needs_fetch(raw) {
            let _ = sender.output(ChatTabOutput::RequestFetchAvatar(jid));
        }
    }

    pub(in crate::components::chat_tab) fn handle_avatar_ready(
        &mut self,
        jid: tina_core::WaIdentity,
        path: String,
    ) {
        let raw = jid.raw();
        let indices: Vec<usize> = self
            .messages
            .guard()
            .iter()
            .enumerate()
            .filter_map(|(i, f)| {
                if f.item.sender_jid.as_deref() == Some(raw) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();
        for idx in indices {
            self.messages
                .send(idx, MessageBubbleInput::SetAvatar(path.clone()));
        }
    }
}
