// `AppMsg` dispatcher: decides between scene transitions, forwarding
// to the main page child, and routing UI intents back to the service
// worker.

use relm4::prelude::*;

use crate::components::login::LoginInput;
use crate::components::main_page::MainInput;
use crate::service::Cmd;

use super::messages::{AppMsg, Scene};
use super::model::AppModel;

impl AppModel {
    pub(super) fn dispatch(&mut self, msg: AppMsg) {
        match msg {
            AppMsg::ShowQrLogin => self.scene = Scene::QrLogin,
            AppMsg::ShowInApp => self.scene = Scene::InApp,
            AppMsg::QrCode(qr) => self.handle_qr(qr),
            AppMsg::Connected {
                account_id,
                phone_number,
                jid,
                push_name,
            } => self.handle_connected(account_id, phone_number, jid, push_name),
            AppMsg::Disconnected(reason) => {
                self.toast(format!("Disconnected: {reason}"));
            }
            AppMsg::LoggedOut => self.handle_logged_out(),
            AppMsg::ChatsUpserted(rows) => {
                let _ = self.main.sender().send(MainInput::ChatsUpserted(rows));
            }
            AppMsg::MessagesAppended { chat_id, messages } => {
                let _ = self
                    .main
                    .sender()
                    .send(MainInput::MessagesAppended { chat_id, messages });
            }
            AppMsg::ChatOpened {
                chat_id,
                name,
                kind,
                messages,
            } => {
                let _ = self.main.sender().send(MainInput::ChatOpened {
                    chat_id,
                    name,
                    kind,
                    messages,
                });
            }
            AppMsg::HistorySyncDone => self.handle_history_sync_done(),
            AppMsg::RepairStarted => {
                self.repairing = true;
                let _ = self.main.sender().send(MainInput::SetRepairing(true));
            }
            AppMsg::RepairProgress {
                stage,
                current,
                total,
                indeterminate,
            } => self.handle_repair_progress(stage, current, total, indeterminate),
            AppMsg::RepairEnded => {
                self.repairing = false;
                let _ = self.main.sender().send(MainInput::SetRepairing(false));
            }
            AppMsg::FatalError(e) => {
                self.error = Some(e);
                self.scene = Scene::Error;
            }
            AppMsg::Toast(text) => self.toast(text),
            AppMsg::OpenChatNew(id) => self.service.handle.send(Cmd::OpenChat(id)),
            AppMsg::CloseChat(id) => self.service.handle.send(Cmd::CloseChat(id)),
            AppMsg::SendText { chat_id, text } => {
                self.service.handle.send(Cmd::SendText { chat_id, text });
            }
            AppMsg::RequestRepair => self.service.handle.send(Cmd::Repair),
            AppMsg::RequestLogout => self.service.handle.send(Cmd::Logout),
            AppMsg::SetChatPinned { chat_id, pinned } => {
                self.service.handle.send(Cmd::SetChatPinned { chat_id, pinned });
            }
            AppMsg::RequestMediaDownload(message_id) => {
                self.service.handle.send(Cmd::DownloadMedia { message_id });
            }
            AppMsg::RequestLoadOlder { chat_id, before_ts } => {
                self.service.handle.send(Cmd::LoadOlder {
                    chat_id,
                    before_ts,
                    limit: 50,
                });
            }
            AppMsg::RequestFetchAvatar(jid) => {
                self.service.handle.send(Cmd::FetchAvatar { jid });
            }
            AppMsg::AvatarReady { jid, path } => {
                let _ = self.main.sender().send(MainInput::AvatarReady { jid, path });
            }
            AppMsg::OlderMessagesLoaded {
                chat_id,
                messages,
                reached_top,
            } => {
                let _ = self.main.sender().send(MainInput::OlderMessagesLoaded {
                    chat_id,
                    messages,
                    reached_top,
                });
            }
            AppMsg::MediaDownloadProgress { .. } => {
                // No-op at app root; routed to the focused tab via MainPage.
            }
            AppMsg::MediaReady {
                message_ids,
                path,
                mimetype,
            } => {
                let _ = self.main.sender().send(MainInput::MediaReady {
                    message_ids,
                    path,
                    mimetype,
                });
            }
            AppMsg::MediaDownloadFailed { message_id, error } => {
                self.toast(format!("Download failed: {error}"));
                let _ = self
                    .main
                    .sender()
                    .send(MainInput::MediaFailed { message_id });
            }
        }
    }

    fn handle_qr(&mut self, qr: String) {
        self.scene = Scene::QrLogin;
        let _ = self.login.sender().send(LoginInput::SetQr(qr));
    }

    fn handle_connected(
        &mut self,
        account_id: String,
        phone_number: Option<String>,
        jid: Option<String>,
        push_name: Option<String>,
    ) {
        self.phone = phone_number.clone();
        let base_j = jid.as_deref().map(crate::format::base_jid);
        let _ = self.main.sender().send(MainInput::SetIdentity {
            account_id,
            phone: phone_number,
            jid: base_j.clone(),
            push_name,
        });
        // Self-portrait: kick off the same avatar pipeline we use for
        // any other JID. AvatarReady will drop the resulting path into
        // MainPage, and the profile popover binds it through #[watch].
        if let Some(j) = base_j {
            self.service.handle.send(Cmd::FetchAvatar { jid: j });
        }
        if self.scene == Scene::QrLogin {
            self.scene = Scene::Syncing;
        }
    }

    fn handle_logged_out(&mut self) {
        self.scene = Scene::QrLogin;
        let _ = self.login.sender().send(LoginInput::Reset);
    }

    fn handle_history_sync_done(&mut self) {
        if self.scene == Scene::Syncing {
            self.scene = Scene::InApp;
        }
        self.service.handle.send(Cmd::LoadChats);
    }

    fn handle_repair_progress(
        &mut self,
        stage: String,
        current: i64,
        total: i64,
        indeterminate: bool,
    ) {
        self.repair_stage = stage.clone();
        self.repair_current = current;
        self.repair_total = total;
        self.repair_indeterminate = indeterminate;
        let _ = self.main.sender().send(MainInput::RepairProgress {
            stage,
            current,
            total,
            indeterminate,
        });
    }
}
