// Dispatcher: maps every `ChatAreaInput` arm onto a method on
// `ChatArea`.

use relm4::ComponentSender;

use super::super::messages::ChatAreaInput;
use super::super::model::ChatArea;

impl ChatArea {
    pub(in crate::components::chat_area) fn dispatch(
        &mut self,
        msg: ChatAreaInput,
        sender: ComponentSender<Self>,
    ) {
        match msg {
            ChatAreaInput::OpenInCurrent(id) => self.handle_open_in_current(id, &sender),
            ChatAreaInput::OpenInNewTab(id) => self.handle_open_in_new_tab(id, &sender),
            ChatAreaInput::ChatOpened {
                chat_id,
                name,
                kind,
                messages,
            } => self.handle_chat_opened(chat_id, name, kind, messages, &sender),
            ChatAreaInput::MessagesAppended { chat_id, messages } => {
                self.handle_messages_appended(chat_id, messages)
            }
            ChatAreaInput::OlderMessagesLoaded {
                chat_id,
                messages,
                reached_top,
            } => self.handle_older_messages_loaded(chat_id, messages, reached_top),
            ChatAreaInput::NewerMessagesLoaded {
                chat_id,
                messages,
                reached_bottom,
            } => self.handle_newer_messages_loaded(chat_id, messages, reached_bottom),
            ChatAreaInput::MediaReady {
                message_ids,
                path,
                mimetype,
            } => self.handle_media_ready(message_ids, path, mimetype),
            ChatAreaInput::MediaFailed { message_id } => self.handle_media_failed(message_id),
            ChatAreaInput::AvatarReady { jid, path } => self.handle_avatar_ready(jid, path),
            ChatAreaInput::AvatarTextureReady(path) => {
                self.handle_avatar_texture_ready(&path)
            }
            ChatAreaInput::PaneTabSelected { pane, chat_id } => {
                self.handle_pane_tab_selected(pane, chat_id)
            }
            ChatAreaInput::TabClosed { pane: _, chat_id } => {
                self.handle_tab_closed(chat_id, &sender)
            }
            ChatAreaInput::PaneFocused(idx) => {
                self.focused_pane = idx;
            }
            ChatAreaInput::AutoMergePane1 => self.handle_auto_merge_pane1(),
            ChatAreaInput::MoveTabToOtherPane(from) => self.handle_move_tab_to_other_pane(from),
            ChatAreaInput::SendFromTab {
                chat_id,
                text,
                mentioned_jids,
                local_id,
            } => self.forward_send(chat_id, text, mentioned_jids, local_id, &sender),
            ChatAreaInput::SendMediaFromTab {
                chat_id,
                kind,
                path,
                caption,
                mimetype,
                filename,
                local_id,
            } => self.forward_send_media(chat_id, kind, path, caption, mimetype, filename, local_id, &sender),
            ChatAreaInput::RequestMediaDownload(id) => self.forward_media_download(id, &sender),
            ChatAreaInput::RequestLoadOlder { chat_id, before_ts } => {
                self.forward_load_older(chat_id, before_ts, &sender)
            }
            ChatAreaInput::RequestLoadNewer { chat_id, after_ts } => {
                self.forward_load_newer(chat_id, after_ts, &sender)
            }
            ChatAreaInput::RequestFetchAvatar(jid) => self.forward_fetch_avatar(jid, &sender),
            ChatAreaInput::RequestStickers { chat_id } => {
                self.forward_request_stickers(chat_id, &sender);
            }
            ChatAreaInput::RequestMarkRead {
                chat_id,
                sender_jid,
                message_ids,
            } => {
                let _ = sender.output(super::super::messages::ChatAreaOutput::RequestMarkRead {
                    chat_id,
                    sender_jid,
                    message_ids,
                });
            }
            ChatAreaInput::StickersLoaded { chat_id, items } => {
                self.handle_stickers_loaded(chat_id, items);
            }
            ChatAreaInput::ReceiptUpdate { message_ids, status } => {
                self.handle_receipt_update(message_ids, status);
            }
            ChatAreaInput::SetUserJid(jid) => self.handle_set_user_jid(jid),
            ChatAreaInput::MentionCandidatesLoaded { chat_id, candidates } => {
                self.handle_mention_candidates_loaded(chat_id, candidates);
            }
        }
    }
}
