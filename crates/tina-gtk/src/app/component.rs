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
use super::pages::{error_page, init_page};

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
                        Scene::Repairing => "repair",
                        Scene::Error => "err",
                    },

                    add_named[Some("init")] = &init_page(),
                    add_named[Some("qr")] = model.login.widget(),

                    // Initial-bootstrap full-screen page. Bound to the
                    // live HistorySyncProgress percentage so the user
                    // can watch the bar fill instead of staring at a
                    // bare spinner.
                    add_named[Some("sync")] = &adw::StatusPage {
                        set_icon_name: Some("loop-symbolic"),
                        set_title: "Syncing messages",
                        #[watch]
                        set_description: Some(&model.sync_stage_label()),

                        #[wrap(Some)]
                        set_child = &gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_spacing: 12,
                            set_halign: gtk::Align::Center,
                            set_valign: gtk::Align::Center,
                            set_width_request: 360,

                            // Pulsing bar for the brief "0%" window
                            // before whatsmeow's first chunk arrives.
                            #[name(sync_pulse_bar)]
                            gtk::ProgressBar {
                                set_pulse_step: 0.08,
                                #[watch]
                                set_visible: model.sync_progress == 0,
                            },
                            gtk::ProgressBar {
                                #[watch]
                                set_visible: model.sync_progress > 0,
                                #[watch]
                                set_fraction: model.sync_fraction(),
                                set_show_text: true,
                                #[watch]
                                set_text: Some(&model.sync_percent_text()),
                            },
                        },
                    },

                    add_named[Some("main")] = model.main.widget(),

                    // Full-screen overlay during a Reconcile (manual
                    // repair). Replaces the in-app view so the user
                    // can't poke the chat list while the worker is
                    // mid-reconcile.
                    add_named[Some("repair")] = &adw::StatusPage {
                        set_icon_name: Some("wrench-symbolic"),
                        #[watch]
                        set_title: model.repair_title(),
                        #[watch]
                        set_description: Some(&model.repair_description()),

                        #[wrap(Some)]
                        set_child = &gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_spacing: 12,
                            set_halign: gtk::Align::Center,
                            set_valign: gtk::Align::Center,
                            set_width_request: 360,

                            #[name(repair_pulse_bar)]
                            gtk::ProgressBar {
                                set_pulse_step: 0.08,
                                #[watch]
                                set_visible: model.repair_indeterminate
                                    || model.repair_total <= 0,
                            },
                            gtk::ProgressBar {
                                #[watch]
                                set_visible: !model.repair_indeterminate
                                    && model.repair_total > 0,
                                #[watch]
                                set_fraction: model.repair_fraction(),
                                #[watch]
                                set_show_text: !model.repair_progress_text().is_empty(),
                                #[watch]
                                set_text: Some(&model.repair_progress_text()),
                            },
                        },
                    },

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
            pre_repair_scene: None,
            error: None,
            repairing: false,
            repair_stage: String::new(),
            repair_current: 0,
            repair_total: 0,
            repair_indeterminate: true,
            sync_progress: 0,
            sync_type: String::new(),
            connection: crate::app::ConnectionStatus::Connecting,
            phone: None,
            service,
            login,
            main,
            toast_overlay: adw::ToastOverlay::new(),
        };

        let widgets = view_output!();

        // Drive the indeterminate fillers on the syncing/repair pages.
        // GTK's ProgressBar only animates when `pulse()` is called, so
        // a recurring glib timer ticks both bars; weak refs let the
        // timer self-terminate on widget destruction.
        for weak in [
            widgets.sync_pulse_bar.downgrade(),
            widgets.repair_pulse_bar.downgrade(),
        ] {
            glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                let Some(bar) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                if bar.is_visible() {
                    bar.pulse();
                }
                glib::ControlFlow::Continue
            });
        }

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
