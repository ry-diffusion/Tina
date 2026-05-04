// Single match-arm dispatcher invoked by `component.rs::update`.

use relm4::ComponentSender;

use super::super::messages::ChatTabInput;
use super::super::model::ChatTab;

impl ChatTab {
    pub(in crate::components::chat_tab) fn dispatch(
        &mut self,
        msg: ChatTabInput,
        sender: ComponentSender<Self>,
    ) {
        match msg {
            ChatTabInput::SetMeta { name, kind } => self.handle_set_meta(name, kind),
            ChatTabInput::Reset(rows) => self.handle_reset(rows, &sender),
            ChatTabInput::Append(rows) => self.handle_append(rows, &sender),
            ChatTabInput::Send => self.handle_send(&sender),
            ChatTabInput::PickAttachment(kind) => self.handle_pick_attachment(kind, &sender),
            ChatTabInput::AttachFile {
                kind,
                path,
                mimetype,
                filename,
            } => self.handle_attach_file(kind, path, mimetype, filename, &sender),
            ChatTabInput::SendMedia {
                kind,
                path,
                caption,
                mimetype,
                filename,
            } => self.handle_send_media(kind, path, caption, mimetype, filename, &sender),
            ChatTabInput::ToggleRecord => self.handle_toggle_record(&sender),
            ChatTabInput::RecordingFinished { path, seconds } => {
                self.handle_recording_finished(path, seconds, &sender)
            }
            ChatTabInput::RecordingFailed(e) => self.handle_recording_failed(e),
            ChatTabInput::OpenStickerPicker => self.handle_open_sticker_picker(&sender),
            ChatTabInput::StickersLoaded(items) => self.handle_stickers_loaded(items, &sender),
            ChatTabInput::SendStickerByPath(path) => self.handle_send_sticker_path(path, &sender),
            ChatTabInput::ReceiptUpdate { message_ids, status } => {
                self.handle_receipt_update(message_ids, status);
            }
            ChatTabInput::MediaReady {
                message_ids,
                path,
                mimetype,
            } => self.handle_media_ready(message_ids, path, mimetype),
            ChatTabInput::SetUserJid(j) => self.handle_set_user_jid(j, &sender),
            ChatTabInput::AvatarReady { jid, path } => self.handle_avatar_ready(jid, path),
            ChatTabInput::MediaFailed(id) => self.handle_media_failed(id),
            ChatTabInput::StickToBottom => self.handle_stick_to_bottom(),
            ChatTabInput::NearBottom => self.handle_near_bottom(),
            ChatTabInput::TrimBottom => self.handle_trim_bottom(),
            ChatTabInput::NearTop => self.handle_near_top(&sender),
            ChatTabInput::PrependOlder {
                messages,
                reached_top,
            } => self.handle_prepend_older(messages, reached_top, &sender),
            ChatTabInput::RequestMediaDownload(id) => {
                self.handle_request_media_download(id, &sender)
            }
            ChatTabInput::JumpToMessage(id) => self.handle_jump_to_message(id),
        }
    }
}
