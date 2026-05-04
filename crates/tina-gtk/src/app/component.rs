// Root of the relm4 component tree.
//
// `AppModel` owns the `AdwApplicationWindow`, the `ServiceWorker` handle to
// the tokio side, and the navigation between the four top-level scenes
// (Init / QrLogin / Syncing / InApp / FatalError). Most actual chat UI lives
// inside the `MainPage` child component; this file is the state machine
// + plumbing (the action bodies live in `dispatch.rs`).

use adw::prelude::*;
use gtk::glib;
use relm4::prelude::*;

use crate::components::login::LoginPage;
use crate::components::main_page::{MainOutput, MainPage};
use crate::service::{Cmd, ServiceWorker};

use super::messages::{AppInit, AppMsg, Scene};
use super::model::AppModel;
use super::pages::{error_page, init_page, syncing_page};

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
        _root: Self::Root,
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
                MainOutput::SetChatPinned { chat_id, pinned } => {
                    AppMsg::SetChatPinned { chat_id, pinned }
                }
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
        self.dispatch(msg);
    }
}

// Suppress an unused warning: the macro already wires the toast_overlay
// field via `#[name]`; keeping the symbol referenced ensures the glib
// crate keeps a strong link in case the rest of the file is trimmed.
#[allow(dead_code)]
fn _force_glib_link() -> glib::ExitCode {
    glib::ExitCode::SUCCESS
}
