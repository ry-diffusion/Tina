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
        new_jid: Option<String>,
        sender: &ComponentSender<Self>,
    ) {
        self.user_jid = new_jid.clone();
        let Some(jid) = new_jid else {
            return;
        };
        if jid.is_empty() {
            return;
        }
        // Back-fill sender_jid on every existing from_me row + paint
        // the cached avatar if the inventory already has it.
        let cached = self.avatars.get(&jid);
        let indices: Vec<usize> = self
            .messages
            .guard()
            .iter()
            .enumerate()
            .filter_map(|(i, f)| if f.item.from_me { Some(i) } else { None })
            .collect();
        for idx in indices {
            self.messages
                .send(idx, MessageBubbleInput::SetSenderJid(jid.clone()));
            if let Some(p) = cached.clone() {
                self.messages
                    .send(idx, MessageBubbleInput::SetAvatar(p));
            }
        }
        if cached.is_none() && self.avatars.needs_fetch(&jid) {
            let _ = sender.output(ChatTabOutput::RequestFetchAvatar(jid));
        }
    }

    pub(in crate::components::chat_tab) fn handle_avatar_ready(
        &mut self,
        jid: String,
        path: String,
    ) {
        let indices: Vec<usize> = self
            .messages
            .guard()
            .iter()
            .enumerate()
            .filter_map(|(i, f)| {
                if f.item.sender_jid.as_deref() == Some(jid.as_str()) {
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
