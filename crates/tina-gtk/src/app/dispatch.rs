// `AppMsg` dispatcher: decides between scene transitions, forwarding
// to the main page child, and routing UI intents back to the service
// worker.

use adw::prelude::*;
use relm4::prelude::*;
use tracing::info;

use crate::components::login::LoginInput;
use crate::components::main_page::MainInput;
use crate::service::Cmd;

use super::messages::{AppMsg, ConnectionStatus, Scene};
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
                // Whatsmeow auto-reconnects on transient drops. We
                // surface this as `Connecting` rather than `Offline`
                // so a flicker on the wire doesn't read as "you're
                // logged out". `LoggedOut` is the explicit terminal.
                self.connection = ConnectionStatus::Connecting;
                let _ = self
                    .main
                    .sender()
                    .send(MainInput::SetConnection(ConnectionStatus::Connecting));
                self.toast(format!("Disconnected: {reason}"));
            }
            AppMsg::LoggedOut => self.handle_logged_out(),
            AppMsg::ChatsUpserted(rows) => {
                let _ = self.main.sender().send(MainInput::ChatsUpserted(rows));
            }
            AppMsg::StatusAuthorsUpserted(rows) => {
                let _ = self
                    .main
                    .sender()
                    .send(MainInput::StatusAuthorsUpserted(rows));
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
            AppMsg::HistorySyncDone => {
                info!(
                    scene = ?self.scene,
                    "[sync] HistorySyncDone — Cmd::LoadChats",
                );
                self.handle_history_sync_done();
                // Clear the sidebar's headerbar progress affordance —
                // `MainInput` is a no-op while we're still in
                // `Scene::Syncing` (MainPage isn't mounted yet) but
                // costs nothing, and matters for re-syncs that
                // happen mid-session (auto-reconnect after a drop).
                let _ = self
                    .main
                    .sender()
                    .send(MainInput::HistorySyncEnded);
            }
            AppMsg::HistorySyncProgress {
                sync_type,
                progress,
            } => {
                info!(
                    %sync_type,
                    progress,
                    scene = ?self.scene,
                    "[sync] HistorySyncProgress",
                );
                self.sync_type = sync_type.clone();
                self.sync_progress = progress.min(100);
                // Forward to the sidebar so the headerbar subtitle +
                // top progress bar reflect the active stream while
                // the user is in-app. During `Scene::Syncing` the
                // sidebar isn't visible but the message lands
                // harmlessly.
                let _ = self.main.sender().send(MainInput::HistorySyncProgress {
                    sync_type,
                    progress,
                });
            }
            AppMsg::RepairStarted => self.handle_repair_started(),
            AppMsg::RepairProgress {
                stage,
                current,
                total,
                indeterminate,
            } => self.handle_repair_progress(stage, current, total, indeterminate),
            AppMsg::RepairEnded => self.handle_repair_ended(),
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
            AppMsg::RequestRepair => {
                // Close the Preferences dialog when the action fires —
                // the user kicked off a long-running command from
                // inside the dialog, the in-headerbar progress strip
                // is what they should be watching now.
                self.settings.widget().close();
                self.service.handle.send(Cmd::Repair);
            }
            AppMsg::RequestPreferences => self.handle_open_preferences(),
            AppMsg::RequestLoadStatuses => self.service.handle.send(Cmd::LoadStatuses),
            AppMsg::RequestRefreshChat(chat_jid) => {
                self.service.handle.send(Cmd::RefreshChat { chat_jid });
            }
            AppMsg::OpenStatusAuthor { sender_jid, name } => {
                info!(%sender_jid, %name, "[stories] OpenStatusAuthor dispatched");
                self.service.handle.send(Cmd::OpenStatusAuthor { sender_jid, name });
            }
            AppMsg::ShowStoriesViewer { name, posts } => {
                info!(%name, count = posts.len(), "[stories] ShowStoriesViewer");
                self.handle_open_stories(name, posts);
            }
            AppMsg::RequestLogout => self.service.handle.send(Cmd::Logout),
            AppMsg::SetDownloadMethod(m) => {
                self.media.set_download_method(m);
                self.service.handle.send(Cmd::SetDownloadMethod(m));
            }
            AppMsg::PreferencesLoaded { method, pid } => {
                self.media.set_download_method(method);
                use crate::components::settings::SettingsInput;
                let _ = self
                    .settings
                    .sender()
                    .send(SettingsInput::SetDownloadMethod(method));
                let _ = self
                    .settings
                    .sender()
                    .send(SettingsInput::SetNanachiPid(pid));
            }
            AppMsg::ClearMediaCache => {
                self.settings.widget().close();
                self.service.handle.send(Cmd::ClearMediaCache);
            }
            AppMsg::ClearAvatarCache => {
                self.settings.widget().close();
                self.service.handle.send(Cmd::ClearAvatarCache);
            }
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
                // Tell the chat area first so the bubble flips out of
                // its "downloading" state regardless of how the user
                // dismisses the dialog (close / retry / Esc).
                let _ = self
                    .main
                    .sender()
                    .send(MainInput::MediaFailed {
                        message_id: message_id.clone(),
                    });
                self.show_download_failed_dialog(message_id, error);
            }
        }
    }

    fn handle_qr(&mut self, qr: String) {
        self.scene = Scene::QrLogin;
        let _ = self.login.sender().send(LoginInput::SetQr(qr));
    }

    /// Modal alert when a media download fails. Replaces the old toast
    /// path because download failures are explicit user actions —
    /// surfacing them as a transient toast meant the user could miss
    /// the error by the time they noticed the spinner had stopped.
    /// Offers a Retry response that re-issues the same `Cmd::DownloadMedia`.
    fn show_download_failed_dialog(&self, message_id: String, error: String) {
        let dialog = adw::AlertDialog::builder()
            .heading("Download failed")
            .body(&error)
            .build();
        dialog.add_response("close", "Close");
        dialog.add_response("retry", "Retry");
        dialog.set_response_appearance("retry", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("retry"));
        dialog.set_close_response("close");

        // Capture only what the response handler needs — the worker
        // handle is `Clone` so this doesn't pin the AppModel.
        let handle = self.service.handle.clone();
        dialog.connect_response(None, move |_, resp| {
            if resp == "retry" {
                handle.send(Cmd::DownloadMedia {
                    message_id: message_id.clone(),
                });
            }
        });

        let parent: Option<gtk::Window> = self
            .toast_overlay
            .root()
            .and_then(|r| r.downcast::<gtk::Window>().ok());
        dialog.present(parent.as_ref());
    }

    fn handle_open_stories(&self, name: String, posts: Vec<tina_db::MessageRow>) {
        // Same anchor pattern as `lightbox.rs`: pass any widget that's
        // a descendant of the application window and let `AdwDialog`
        // walk up to find the right parent. The toast overlay is
        // always mounted so it's a stable handle.
        crate::components::stories::open_stories_viewer(&self.toast_overlay, &name, posts);
    }

    fn handle_open_preferences(&self) {
        // Recompute disk-usage / RSS rows right before the dialog
        // becomes visible. They'd be stale otherwise — values were
        // last sampled the previous time the dialog was open.
        let _ = self
            .settings
            .sender()
            .send(crate::components::settings::SettingsInput::Refresh);
        // Worker round-trip for the persisted method + nanachi pid;
        // result lands as `AppMsg::PreferencesLoaded`.
        self.service.handle.send(Cmd::LoadPreferences);
        // Parent: walk up from the toast overlay (which is wrapped
        // inside the AdwApplicationWindow). `Widget::root()` returns
        // the topmost ancestor; if it's a Window we attach the
        // dialog to it for the modal/center-on-parent behaviour
        // AdwDialog implements internally.
        let parent: Option<gtk::Window> = self
            .toast_overlay
            .root()
            .and_then(|r| r.downcast::<gtk::Window>().ok());
        self.settings.widget().present(parent.as_ref());
    }

    fn handle_connected(
        &mut self,
        account_id: String,
        phone_number: Option<String>,
        jid: Option<tina_core::WaIdentity>,
        push_name: Option<String>,
    ) {
        self.phone = phone_number.clone();
        self.connection = ConnectionStatus::Connected;
        // Phone-rooted JIDs sometimes carry a device suffix
        // (`5561…:91@s.whatsapp.net`); the contacts pipeline + avatar
        // store key on the suffix-less form, so strip via base_jid.
        let base_j = jid.as_ref().map(|x| {
            tina_core::WaIdentity::parse(&crate::format::base_jid(x.raw()))
        });
        let _ = self
            .main
            .sender()
            .send(MainInput::SetConnection(ConnectionStatus::Connected));
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
            info!("[sync] Scene::Syncing → Scene::InApp");
            self.scene = Scene::InApp;
            // Pin the bar at 100% on exit; nicer than letting it sit
            // mid-fill if whatsmeow signalled "done" before we got a
            // final 100% chunk.
            self.sync_progress = 100;
        }
        self.service.handle.send(Cmd::LoadChats);
    }

    fn handle_repair_started(&mut self) {
        self.repairing = true;
        self.repair_stage.clear();
        self.repair_current = 0;
        self.repair_total = 0;
        self.repair_indeterminate = true;
        self.pre_repair_scene = Some(self.scene);
        self.scene = Scene::Repairing;
        let _ = self.main.sender().send(MainInput::SetRepairing(true));
    }

    fn handle_repair_ended(&mut self) {
        self.repairing = false;
        if self.scene == Scene::Repairing {
            self.scene = self.pre_repair_scene.take().unwrap_or(Scene::InApp);
        } else {
            self.pre_repair_scene = None;
        }
        let _ = self.main.sender().send(MainInput::SetRepairing(false));
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
