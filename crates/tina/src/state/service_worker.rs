use std::{path::PathBuf, sync::Arc, thread::JoinHandle};

use color_eyre::eyre::Context;
use slint::{ComponentHandle, Weak};
use tokio::sync::{Mutex, RwLock, mpsc};

use crate::{Scene, Tina, scenes::LoginScene};
use tina_worker::{TinaWorker, WorkerEvent};

use super::messages::UIMessage;
use super::ui::{
    apply_chat_opened, apply_chats_upserted, apply_messages_appended, crash_app,
    set_repairing, set_selected_account, setup_chat_callbacks, setup_settings_callbacks,
    show_error, show_scene, update_account_list, update_qr_code, update_repair_progress,
    update_user_profile,
};

type UiSender = mpsc::UnboundedSender<UIMessage>;
type UiReceiver = mpsc::UnboundedReceiver<UIMessage>;
type UiSendError = mpsc::error::SendError<UIMessage>;

type WorkerStorage = Arc<Mutex<Option<Arc<TinaWorker>>>>;

pub struct TinaUIServiceWorker {
    channel: UiSender,
    #[allow(dead_code)]
    worker: WorkerStorage,
    worker_thread: Option<JoinHandle<()>>,
}

impl TinaUIServiceWorker {
    pub fn new(ui_handle: &Tina, nanachi_dir: PathBuf) -> Self {
        let (channel, r) = mpsc::unbounded_channel();
        let tx = channel.clone();
        let worker = Arc::new(Mutex::new(None));
        let worker_clone = worker.clone();

        let worker_thread = std::thread::Builder::new()
            .name("Tina UI Service Worker Thread".to_string())
            .spawn({
                let handle_weak = ui_handle.as_weak();
                move || {
                    tokio::runtime::Runtime::new()
                        .unwrap()
                        .block_on(ui_worker_loop(
                            r,
                            handle_weak,
                            nanachi_dir,
                            tx,
                            worker_clone,
                        ))
                        .unwrap()
                }
            })
            .expect("Failed to boot up Service UI Worker Thread");

        Self {
            channel,
            worker,
            worker_thread: Some(worker_thread),
        }
    }

    pub fn send(&self, msg: UIMessage) -> Result<(), UiSendError> {
        self.channel.send(msg)
    }

    #[allow(dead_code)]
    pub async fn worker(&self) -> Option<Arc<TinaWorker>> {
        self.worker.lock().await.clone()
    }

    pub fn join(self) -> std::thread::Result<()> {
        drop(self);
        Ok(())
    }
}

impl Drop for TinaUIServiceWorker {
    fn drop(&mut self) {
        let _ = self.channel.send(UIMessage::Quit);
        if let Some(thread) = self.worker_thread.take() {
            let _ = thread.join();
        }
    }
}

async fn ui_worker_loop(
    mut r: UiReceiver,
    handle: Weak<Tina>,
    nanachi_dir: PathBuf,
    tx: UiSender,
    worker_storage: WorkerStorage,
) -> color_eyre::Result<()> {
    let mut worker = TinaWorker::new(nanachi_dir).await.map_err(|e| {
        crash_app(&handle, &format!("Failed to create worker: {}", e));
        e
    })?;

    let mut event_rx = worker.take_event_receiver().ok_or_else(|| {
        let err = color_eyre::eyre::eyre!("Failed to get event receiver");
        crash_app(&handle, "Failed to get event receiver");
        err
    })?;

    let worker = Arc::new(worker);
    let login_scene = LoginScene::new(handle.clone(), worker.clone(), tx.clone());

    *worker_storage.lock().await = Some(worker.clone());

    setup_settings_callbacks(&handle, tx.clone());
    setup_chat_callbacks(&handle, tx.clone());

    worker.start().await.wrap_err("Failed to start worker")?;

    let handle_ui = handle.clone();
    let tx_events = tx.clone();
    let in_login_flow_shared = Arc::new(RwLock::new(false));
    let in_login_flow_reader = in_login_flow_shared.clone();
    let event_handle = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let is_login = *in_login_flow_reader.read().await;
            handle_worker_event(&handle_ui, event, &tx_events, is_login).await;
        }
    });

    let mut selected_account: Option<String> = None;

    loop {
        let m = match r.recv().await {
            None => {
                event_handle.abort();
                return Ok(());
            }
            Some(m) => m,
        };

        match m {
            UIMessage::Quit => {
                event_handle.abort();
                return Ok(());
            }
            UIMessage::Initialize => {
                if let Err(e) = login_scene.clone().check_and_transition().await {
                    tracing::error!("Initialization failed: {}", e);
                    crash_app(&handle, &format!("Initialization failed: {}", e));
                }
            }
            UIMessage::CreateAccount => {
                if let Err(e) = login_scene.clone().handle_create_account().await {
                    tracing::error!("Failed to create account: {}", e);
                    show_error(&handle, &format!("Failed to create account: {}", e));
                }
            }
            UIMessage::LoginRequested(account_id) => {
                if let Err(e) = login_scene
                    .clone()
                    .handle_login_request(account_id.clone())
                    .await
                {
                    tracing::error!("Failed to login {}: {}", account_id, e);
                    show_error(&handle, &format!("Failed to login {}: {}", account_id, e));
                }
            }
            UIMessage::ShowScene(scene) => {
                show_scene(&handle, scene);
            }
            UIMessage::ShowQrLogin => {
                *in_login_flow_shared.write().await = true;
                show_scene(&handle, Scene::QRLogin);
            }
            UIMessage::ShowAccountSelection(accounts) => {
                let fallback = accounts.first().map(|a| a.id.clone());
                update_account_list(&handle, &accounts);
                let next_selection = selected_account.clone().or(fallback);
                set_selected_account(&handle, next_selection.as_deref());
                selected_account = next_selection;
                show_scene(&handle, Scene::InApp);
            }
            UIMessage::ShowSyncing => {
                show_scene(&handle, Scene::Syncing);
            }
            UIMessage::ShowInApp => {
                *in_login_flow_shared.write().await = false;
                if let Some(account_id) = &selected_account {
                    if let Ok(rows) = worker.list_chat_rows(account_id).await {
                        apply_chats_upserted(&handle, rows);
                    }
                }
                show_scene(&handle, Scene::InApp);
            }
            UIMessage::ShowError(msg) => {
                show_error(&handle, &msg);
            }
            UIMessage::QrCodeReceived(qr) => {
                update_qr_code(&handle, &qr);
            }
            UIMessage::AccountSelected(account_id) => {
                set_selected_account(
                    &handle,
                    if account_id.is_empty() {
                        None
                    } else {
                        Some(&account_id)
                    },
                );
                selected_account = if account_id.is_empty() {
                    None
                } else {
                    Some(account_id)
                };
            }
            UIMessage::LoadChats => {
                if let Some(account_id) = &selected_account {
                    match worker.list_chat_rows(account_id).await {
                        Ok(rows) => {
                            tracing::info!("Loaded {} chats", rows.len());
                            apply_chats_upserted(&handle, rows);
                        }
                        Err(e) => {
                            tracing::error!("Failed to load chats: {}", e);
                            show_error(&handle, &format!("Failed to load chats: {}", e));
                        }
                    }
                }
            }
            UIMessage::ApplyChatsUpserted(rows) => {
                apply_chats_upserted(&handle, rows);
            }
            UIMessage::RepairRequested => {
                let Some(account_id) = selected_account.clone() else {
                    continue;
                };
                update_repair_progress(&handle, "Iniciando…", 0, 0, true);
                set_repairing(&handle, true);
                if let Err(e) = worker.reconcile_account(&account_id).await {
                    tracing::error!("Repair failed: {}", e);
                    set_repairing(&handle, false);
                } else {
                    tracing::info!("Repair (reconcile) requested for {}", account_id);
                }
            }
            UIMessage::SetActiveChat(chat_id) => {
                let Some(account_id) = selected_account.clone() else {
                    continue;
                };
                worker
                    .set_active_chat(&account_id, chat_id.as_deref())
                    .await;
                match chat_id.as_deref() {
                    None => {
                        apply_chat_opened(&handle, None, None, None, Vec::new());
                    }
                    Some(id) => {
                        let row = worker.get_chat_row(&account_id, id).await.ok().flatten();
                        let (name, kind) = row
                            .as_ref()
                            .map(|r| (r.name.clone(), r.kind.clone()))
                            .unwrap_or_else(|| (id.to_string(), "unknown".to_string()));
                        let messages = worker
                            .get_message_rows(&account_id, id, 200, 0)
                            .await
                            .unwrap_or_default();
                        apply_chat_opened(&handle, Some(id), Some(&name), Some(&kind), messages);
                    }
                }
            }
        }
    }
}

#[tracing::instrument(skip(handle, tx))]
async fn handle_worker_event(
    handle: &Weak<Tina>,
    event: WorkerEvent,
    tx: &UiSender,
    in_login_flow: bool,
) {
    match event {
        WorkerEvent::NanachiReady => {
            tracing::info!("Nanachi is ready");
        }
        WorkerEvent::QrCode { account_id, qr } => {
            tracing::info!("QR Code for account {}", account_id);
            update_qr_code(handle, &qr);
        }
        WorkerEvent::Connected {
            account_id,
            phone_number,
        } if in_login_flow => {
            tracing::info!(
                "Connected during login: {} (phone: {:?})",
                account_id,
                phone_number
            );
            update_user_profile(handle, Some(&account_id), phone_number.as_deref(), None);
            let _ = tx.send(UIMessage::ShowSyncing);
        }
        WorkerEvent::Connected {
            account_id,
            phone_number,
        } => {
            tracing::info!("Connected: {} (phone: {:?})", account_id, phone_number);
            update_user_profile(handle, Some(&account_id), phone_number.as_deref(), None);
        }
        WorkerEvent::ReconcileProgress {
            account_id: _,
            stage,
            current,
            total,
            indeterminate,
        } => {
            tracing::debug!("ReconcileProgress: {} ({}/{})", stage, current, total);
            update_repair_progress(handle, &stage, current, total, indeterminate);
        }
        WorkerEvent::HistorySyncComplete {
            account_id,
            messages_count,
        } => {
            tracing::info!(
                "History sync complete for {}: {} messages",
                account_id,
                messages_count
            );
            // Garante que a UI sai do "Syncing".
            let _ = tx.send(UIMessage::ShowInApp);
            // Também marca o reconcile como terminado (botão Reparar).
            set_repairing(handle, false);
        }
        WorkerEvent::ChatsUpserted { account_id: _, rows } => {
            tracing::debug!("ChatsUpserted: {} rows", rows.len());
            let _ = tx.send(UIMessage::ApplyChatsUpserted(rows));
        }
        WorkerEvent::MessagesAppended {
            account_id: _,
            chat_id,
            messages,
        } => {
            tracing::debug!(
                "MessagesAppended chat={} count={}",
                chat_id,
                messages.len()
            );
            apply_messages_appended(handle, &chat_id, messages);
        }
        WorkerEvent::Error { account_id, error } => {
            let msg = format!("Error ({}): {}", account_id.unwrap_or_default(), error);
            tracing::error!("{}", msg);
            show_error(handle, &msg);
        }
        _ => {
            tracing::debug!("Event: {:?}", event);
        }
    }
}
