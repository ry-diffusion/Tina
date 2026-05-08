// Routing handlers: forwarded events from chat tabs (messages/media/avatar)
// and identity broadcast.

use relm4::ComponentSender;
use relm4::prelude::*;
use tina_db::MessageRow;

use crate::components::chat_tab::ChatTabInput;

use super::super::messages::ChatAreaOutput;
use super::super::model::ChatArea;

impl ChatArea {
    pub(in crate::components::chat_area) fn handle_messages_appended(
        &mut self,
        chat_id: String,
        messages: Vec<MessageRow>,
    ) {
        if let Some((controller, _, _)) = self.open_tabs.get(&chat_id) {
            let _ = controller.sender().send(ChatTabInput::Append(messages));
        } else {
            tracing::warn!(
                chat = %chat_id,
                "MessagesAppended received for chat with no open tab",
            );
        }
    }

    pub(in crate::components::chat_area) fn handle_older_messages_loaded(
        &mut self,
        chat_id: String,
        messages: Vec<MessageRow>,
        reached_top: bool,
    ) {
        if let Some((controller, _, _)) = self.open_tabs.get(&chat_id) {
            let _ = controller.sender().send(ChatTabInput::PrependOlder {
                messages,
                reached_top,
            });
        }
    }

    pub(in crate::components::chat_area) fn handle_newer_messages_loaded(
        &mut self,
        chat_id: String,
        messages: Vec<MessageRow>,
        reached_bottom: bool,
    ) {
        if let Some((controller, _, _)) = self.open_tabs.get(&chat_id) {
            let _ = controller.sender().send(ChatTabInput::AppendNewer {
                messages,
                reached_bottom,
            });
        }
    }

    pub(in crate::components::chat_area) fn handle_media_ready(
        &mut self,
        message_ids: Vec<String>,
        path: String,
        mimetype: Option<String>,
    ) {
        self.media
            .set_ready(&message_ids, &path, mimetype.as_deref());
        for (controller, _, _) in self.open_tabs.values() {
            let _ = controller.sender().send(ChatTabInput::MediaReady {
                message_ids: message_ids.clone(),
                path: path.clone(),
                mimetype: mimetype.clone(),
            });
        }
    }

    pub(in crate::components::chat_area) fn handle_media_failed(&mut self, message_id: String) {
        self.media.set_failed(&message_id);
        for (controller, _, _) in self.open_tabs.values() {
            let _ = controller
                .sender()
                .send(ChatTabInput::MediaFailed(message_id.clone()));
        }
    }

    pub(in crate::components::chat_area) fn handle_avatar_ready(
        &mut self,
        jid: tina_core::WaIdentity,
        path: String,
    ) {
        let raw = jid.raw().to_string();
        self.avatars.put(raw.clone(), path.clone());
        for pane in &mut self.panes {
            if pane.current_chat_id.as_deref() == Some(raw.as_str()) {
                pane.current_chat_avatar = Some(path.clone());
            }
        }
        self.apply_pane_avatar(0);
        self.apply_pane_avatar(1);
        for (controller, _, _) in self.open_tabs.values() {
            let _ = controller.sender().send(ChatTabInput::AvatarReady {
                jid: jid.clone(),
                path: path.clone(),
            });
        }
    }

    /// Local glycin decode of an avatar landed in the inventory
    /// cache. Forward to every open tab so its rows that show this
    /// avatar can rebind and pull the texture.
    pub(in crate::components::chat_area) fn handle_avatar_texture_ready(
        &mut self,
        path: &str,
    ) {
        // Refresh the per-pane header avatar paintables.
        self.apply_pane_avatar(0);
        self.apply_pane_avatar(1);
        for (controller, _, _) in self.open_tabs.values() {
            let _ = controller
                .sender()
                .send(ChatTabInput::AvatarTextureReady(path.to_string()));
        }
    }

    pub(in crate::components::chat_area) fn handle_set_user_jid(
        &mut self,
        jid: Option<tina_core::WaIdentity>,
    ) {
        self.user_jid = jid.clone();
        for (controller, _, _) in self.open_tabs.values() {
            let _ = controller
                .sender()
                .send(ChatTabInput::SetUserJid(jid.clone()));
        }
    }

    /// Worker delivered the resolved `@`-mention candidates for a
    /// chat. Stash them in the shared inventory (so future tabs of
    /// the same chat reuse them, and the bubble renderer can resolve
    /// `@<digits>` to a name without round-tripping) and forward to
    /// the matching open tab so its composer popover repaints.
    pub(in crate::components::chat_area) fn handle_mention_candidates_loaded(
        &mut self,
        chat_id: String,
        candidates: Vec<tina_db::MentionCandidate>,
    ) {
        self.mentions.set_candidates(&chat_id, &candidates);
        if let Some((controller, _, _)) = self.open_tabs.get(&chat_id) {
            let _ = controller
                .sender()
                .send(ChatTabInput::MentionCandidatesLoaded(candidates));
        }
    }

    pub(in crate::components::chat_area) fn forward_send(
        &mut self,
        chat_id: String,
        text: String,
        mentioned_jids: Vec<String>,
        local_id: String,
        sender: &ComponentSender<Self>,
    ) {
        let _ = sender.output(ChatAreaOutput::SendText {
            chat_id,
            text,
            mentioned_jids,
            local_id,
        });
    }

    pub(in crate::components::chat_area) fn forward_request_stickers(
        &mut self,
        chat_id: String,
        sender: &ComponentSender<Self>,
    ) {
        let _ = sender.output(ChatAreaOutput::RequestStickers { chat_id });
    }

    pub(in crate::components::chat_area) fn handle_stickers_loaded(
        &mut self,
        chat_id: String,
        items: Vec<(String, String)>,
    ) {
        if let Some((controller, _, _)) = self.open_tabs.get(&chat_id) {
            let _ = controller
                .sender()
                .send(crate::components::chat_tab::ChatTabInput::StickersLoaded(items));
        }
    }

    pub(in crate::components::chat_area) fn handle_receipt_update(
        &mut self,
        message_ids: Vec<String>,
        status: String,
    ) {
        // Fan-out: we don't know which tab holds these ids without
        // walking each one's seen-set, so just push to all and let
        // each ChatTab no-op when it doesn't recognize anything.
        for (controller, _, _) in self.open_tabs.values() {
            let _ = controller.sender().send(
                crate::components::chat_tab::ChatTabInput::ReceiptUpdate {
                    message_ids: message_ids.clone(),
                    status: status.clone(),
                },
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::components::chat_area) fn forward_send_media(
        &mut self,
        chat_id: String,
        kind: tina_core::MediaKind,
        path: String,
        caption: Option<String>,
        mimetype: Option<String>,
        filename: Option<String>,
        local_id: Option<String>,
        sender: &ComponentSender<Self>,
    ) {
        let _ = sender.output(ChatAreaOutput::SendMedia {
            chat_id,
            kind,
            path,
            caption,
            mimetype,
            filename,
            local_id,
        });
    }

    pub(in crate::components::chat_area) fn forward_media_download(
        &mut self,
        id: String,
        sender: &ComponentSender<Self>,
    ) {
        let _ = sender.output(ChatAreaOutput::RequestMediaDownload(id));
    }

    pub(in crate::components::chat_area) fn forward_load_older(
        &mut self,
        chat_id: String,
        before_ts: i64,
        sender: &ComponentSender<Self>,
    ) {
        let _ = sender.output(ChatAreaOutput::RequestLoadOlder { chat_id, before_ts });
    }

    pub(in crate::components::chat_area) fn forward_load_newer(
        &mut self,
        chat_id: String,
        after_ts: i64,
        sender: &ComponentSender<Self>,
    ) {
        let _ = sender.output(ChatAreaOutput::RequestLoadNewer { chat_id, after_ts });
    }

    pub(in crate::components::chat_area) fn forward_fetch_avatar(
        &mut self,
        jid: tina_core::WaIdentity,
        sender: &ComponentSender<Self>,
    ) {
        let _ = sender.output(ChatAreaOutput::RequestFetchAvatar(jid));
    }
}
