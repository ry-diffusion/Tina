// Root of the relm4 component tree.
//
// `AppModel` owns the `AdwApplicationWindow`, the `ServiceWorker` handle to
// the tokio side, and the navigation between the four top-level scenes
// (Init / QrLogin / Syncing / InApp / FatalError). Most actual chat UI lives
// inside the `MainPage` child component; this file is mostly the state
// machine + plumbing.

use std::path::PathBuf;

use adw::prelude::*;
use gtk::glib;
use relm4::Controller;
use relm4::prelude::*;
use tina_db::{ChatRow, MessageRow};

use crate::components::login::{LoginInput, LoginPage};
use crate::components::main_page::{MainInput, MainOutput, MainPage};
use crate::service::{Cmd, ServiceWorker};

pub struct AppInit {
    pub nanachi_dir: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    Init,
    QrLogin,
    Syncing,
    InApp,
    Error,
}

#[derive(Debug)]
pub enum AppMsg {
    // From the service worker:
    ShowQrLogin,
    ShowInApp,
    QrCode(String),
    Connected {
        account_id: String,
        phone_number: Option<String>,
        jid: Option<String>,
        push_name: Option<String>,
    },
    Disconnected(String),
    LoggedOut,
    ChatsUpserted(Vec<ChatRow>),
    MessagesAppended {
        chat_id: String,
        messages: Vec<MessageRow>,
    },
    ChatOpened {
        chat_id: Option<String>,
        name: String,
        kind: String,
        messages: Vec<MessageRow>,
    },
    HistorySyncDone,
    RepairStarted,
    RepairProgress {
        stage: String,
        current: i64,
        total: i64,
        indeterminate: bool,
    },
    RepairEnded,
    FatalError(String),
    Toast(String),

    MediaDownloadProgress {
        message_id: String,
        current: i64,
        total: i64,
    },
    MediaReady {
        message_ids: Vec<String>,
        path: String,
        mimetype: Option<String>,
    },
    MediaDownloadFailed {
        message_id: String,
        error: String,
    },
    RequestMediaDownload(String),
    OlderMessagesLoaded {
        chat_id: String,
        messages: Vec<MessageRow>,
        reached_top: bool,
    },
    RequestLoadOlder {
        chat_id: String,
        before_ts: i64,
    },
    AvatarReady {
        jid: String,
        path: String,
    },
    RequestFetchAvatar(String),

    // From the UI:
    OpenChatNew(String),
    CloseChat(String),
    SendText {
        chat_id: String,
        text: String,
    },
    RequestRepair,
    RequestLogout,
}

pub struct AppModel {
    scene: Scene,
    error: Option<String>,
    repairing: bool,
    repair_stage: String,
    repair_current: i64,
    repair_total: i64,
    repair_indeterminate: bool,
    phone: Option<String>,
    service: ServiceWorker,
    login: Controller<LoginPage>,
    main: Controller<MainPage>,
    toast_overlay: adw::ToastOverlay,
}

#[relm4::component(pub)]
impl SimpleComponent for AppModel {
    type Init = AppInit;
    type Input = AppMsg;
    type Output = ();

    view! {
        adw::ApplicationWindow {
            set_title: Some("Tina"),
            set_default_size: (1280, 820),

            #[name(toast_overlay)]
            adw::ToastOverlay {
                #[wrap(Some)]
                set_child = &gtk::Stack {
                    set_transition_type: gtk::StackTransitionType::Crossfade,

                    #[watch]
                    set_visible_child_name: match model.scene {
                        Scene::Init => "init",
                        Scene::QrLogin => "qr",
                        Scene::Syncing => "sync",
                        Scene::InApp => "main",
                        Scene::Error => "err",
                    },

                    add_named[Some("init")] = &init_page(),
                    add_named[Some("qr")] = model.login.widget(),
                    add_named[Some("sync")] = &syncing_page(),
                    add_named[Some("main")] = model.main.widget(),
                    add_named[Some("err")] = &error_page(model.error.clone().unwrap_or_default()),
                },
            },
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let service = ServiceWorker::spawn(init.nanachi_dir, sender.input_sender().clone());

        let login = LoginPage::builder().launch(()).detach();

        let avatars = crate::inventory::AvatarInventory::new();
        let media = crate::inventory::MediaInventory::new();

        let main = MainPage::builder()
            .launch(crate::components::main_page::MainInit {
                service: service.handle.clone(),
                avatars,
                media,
            })
            .forward(sender.input_sender(), |o| match o {
                MainOutput::OpenChatNew(id) => AppMsg::OpenChatNew(id),
                MainOutput::CloseChat(id) => AppMsg::CloseChat(id),
                MainOutput::SendText { chat_id, text } => AppMsg::SendText { chat_id, text },
                MainOutput::RequestRepair => AppMsg::RequestRepair,
                MainOutput::RequestLogout => AppMsg::RequestLogout,
                MainOutput::RequestMediaDownload(id) => AppMsg::RequestMediaDownload(id),
                MainOutput::RequestLoadOlder { chat_id, before_ts } => {
                    AppMsg::RequestLoadOlder { chat_id, before_ts }
                }
                MainOutput::RequestFetchAvatar(jid) => AppMsg::RequestFetchAvatar(jid),
            });
        let model = AppModel {
            scene: Scene::Init,
            error: None,
            repairing: false,
            repair_stage: String::new(),
            repair_current: 0,
            repair_total: 0,
            repair_indeterminate: true,
            phone: None,
            service,
            login,
            main,
            toast_overlay: adw::ToastOverlay::new(),
        };

        let widgets = view_output!();

        if !gtk::gdk_pixbuf::Pixbuf::formats()
            .iter()
            .any(|f| f.name().as_deref() == Some("webp"))
        {
            let toast = adw::Toast::builder()
                .title("Aviso: Suporte a WebP não encontrado! Figurinhas podem não carregar. Instale webp-pixbuf-loader.")
                .timeout(10)
                .build();
            widgets.toast_overlay.add_toast(toast);
        }

        model.service.handle.send(Cmd::Initialize);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: AppMsg, _sender: ComponentSender<Self>) {
        match msg {
            AppMsg::ShowQrLogin => self.scene = Scene::QrLogin,
            AppMsg::ShowInApp => self.scene = Scene::InApp,
            AppMsg::QrCode(qr) => {
                self.scene = Scene::QrLogin;
                let _ = self.login.sender().send(LoginInput::SetQr(qr));
            }
            AppMsg::Connected {
                account_id,
                phone_number,
                jid,
                push_name,
            } => {
                self.phone = phone_number.clone();
                let base_j = jid.as_deref().map(crate::format::base_jid);
                let _ = self.main.sender().send(MainInput::SetIdentity {
                    account_id,
                    phone: phone_number,
                    jid: base_j.clone(),
                    push_name,
                });
                // Self-portrait: kick off the same avatar pipeline we use
                // for any other JID. AvatarReady will drop the resulting
                // path into MainPage, and the profile popover binds it
                // through #[watch].
                if let Some(j) = base_j {
                    self.service.handle.send(Cmd::FetchAvatar { jid: j });
                }
                if self.scene == Scene::QrLogin {
                    self.scene = Scene::Syncing;
                }
            }
            AppMsg::Disconnected(reason) => {
                self.toast(format!("Disconnected: {reason}"));
            }
            AppMsg::LoggedOut => {
                self.scene = Scene::QrLogin;
                let _ = self.login.sender().send(LoginInput::Reset);
            }
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
            AppMsg::HistorySyncDone => {
                if self.scene == Scene::Syncing {
                    self.scene = Scene::InApp;
                }
                self.service.handle.send(Cmd::LoadChats);
            }
            AppMsg::RepairStarted => {
                self.repairing = true;
                let _ = self.main.sender().send(MainInput::SetRepairing(true));
            }
            AppMsg::RepairProgress {
                stage,
                current,
                total,
                indeterminate,
            } => {
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
            AppMsg::RepairEnded => {
                self.repairing = false;
                let _ = self.main.sender().send(MainInput::SetRepairing(false));
            }
            AppMsg::FatalError(e) => {
                self.error = Some(e);
                self.scene = Scene::Error;
            }
            AppMsg::Toast(text) => {
                self.toast(text);
            }
            AppMsg::OpenChatNew(id) => {
                self.service.handle.send(Cmd::OpenChat(id));
            }
            AppMsg::CloseChat(id) => {
                self.service.handle.send(Cmd::CloseChat(id));
            }
            AppMsg::SendText { chat_id, text } => {
                self.service.handle.send(Cmd::SendText { chat_id, text });
            }
            AppMsg::RequestRepair => {
                self.service.handle.send(Cmd::Repair);
            }
            AppMsg::RequestLogout => {
                self.service.handle.send(Cmd::Logout);
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
                let _ = self
                    .main
                    .sender()
                    .send(MainInput::AvatarReady { jid, path });
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
}

impl AppModel {
    fn toast(&self, text: String) {
        let toast = adw::Toast::builder().title(&text).timeout(3).build();
        self.toast_overlay.add_toast(toast);
    }
}

// ---------- static placeholder pages (init / syncing / fatal) -----------

fn init_page() -> adw::StatusPage {
    let page = adw::StatusPage::builder()
        .icon_name("chat-bubble-text-symbolic")
        .title("Tina")
        .description("Initialising…")
        .build();
    let spinner = gtk::Spinner::builder().spinning(true).build();
    page.set_child(Some(&spinner));
    page
}

fn syncing_page() -> adw::StatusPage {
    let page = adw::StatusPage::builder()
        .icon_name("emblem-synchronizing-symbolic")
        .title("Syncing messages")
        .description("Hang on while we pull your history.")
        .build();
    let spinner = gtk::Spinner::builder().spinning(true).build();
    page.set_child(Some(&spinner));
    page
}

fn error_page(msg: String) -> adw::StatusPage {
    adw::StatusPage::builder()
        .icon_name("dialog-error-symbolic")
        .title("Something went wrong")
        .description(&msg)
        .build()
}

// Suppress an unused warning for the toast_overlay field — the macro
// already wires it via `#[name]`.
#[allow(dead_code)]
fn _force_glib_link() -> glib::ExitCode {
    glib::ExitCode::SUCCESS
}
