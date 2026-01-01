use std::{path::PathBuf, sync::Arc, thread::JoinHandle};

use chrono::Datelike;
use color_eyre::eyre::Context;
use slint::{ComponentHandle, Weak};
use tokio::sync::{Mutex, RwLock, mpsc};

use crate::{Scene, Tina, scenes::LoginScene};
use tina_worker::{TinaWorker, WorkerEvent};

use super::messages::UIMessage;
use super::ui::{
    crash_app, load_account_data, set_selected_account, setup_chat_callbacks,
    setup_settings_callbacks, show_error, show_scene, update_account_list, update_chat_name,
    update_chat_preview, update_chats_list, update_qr_code, update_user_profile,
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

    /// Send a UI message to the worker thread
    pub fn send(&self, msg: UIMessage) -> Result<(), UiSendError> {
        self.channel.send(msg)
    }

    /// Get a reference to the TinaWorker
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
    // Initialize TinaWorker
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

    // Store worker reference for external access
    *worker_storage.lock().await = Some(worker.clone());

    // Setup UI callbacks for settings and chats
    setup_settings_callbacks(&handle);
    setup_chat_callbacks(&handle, tx.clone());

    // Start worker
    worker.start().await.wrap_err("Failed to start worker")?;

    // Spawn event handler task
    let handle_ui = handle.clone();
    let worker_clone = worker.clone();
    let tx_events = tx.clone();
    let in_login_flow_shared = Arc::new(RwLock::new(false));
    let in_login_flow_reader = in_login_flow_shared.clone();
    let event_handle = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let is_login = *in_login_flow_reader.read().await;
            handle_worker_event(&handle_ui, &worker_clone, event, &tx_events, is_login).await;
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
                // Skip SelectAccount scene - go directly to InApp
                show_scene(&handle, Scene::InApp);
            }
            UIMessage::ShowSyncing => {
                show_scene(&handle, Scene::Syncing);
            }
            UIMessage::ShowInApp => {
                *in_login_flow_shared.write().await = false;
                // Load initial data and show
                if let Some(account_id) = &selected_account {
                    if let Err(e) = load_account_data(&worker, account_id).await {
                        tracing::warn!("Failed to load account data: {}", e);
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
                    tracing::info!("Loading chats for account: {}", account_id);
                    match worker.get_chats(account_id).await {
                        Ok(chats) => {
                            tracing::info!("Loaded {} chats", chats.len());
                            update_chats_list(&handle, &chats);

                            // Spawn tasks to load names and previews asynchronously
                            for chat_jid in chats {
                                let worker_clone = worker.clone();
                                let account_id_clone = account_id.clone();
                                let chat_jid_clone = chat_jid.clone();
                                let tx_clone = tx.clone();

                                tokio::spawn(async move {
                                    // Load chat name
                                    if let Ok(Some(name)) = worker_clone
                                        .get_chat_name(&account_id_clone, &chat_jid_clone)
                                        .await
                                    {
                                        let _ = tx_clone.send(UIMessage::UpdateChatName {
                                            chat_jid: chat_jid_clone.clone(),
                                            name,
                                        });
                                    }

                                    // Load last message
                                    if let Ok(messages) = worker_clone
                                        .get_messages(
                                            &account_id_clone,
                                            Some(&chat_jid_clone),
                                            1,
                                            0,
                                        )
                                        .await
                                    {
                                        if let Some(last_msg) = messages.first() {
                                            let content = last_msg
                                                .content
                                                .clone()
                                                .unwrap_or_else(|| "[Media]".to_string());
                                            let timestamp = format_timestamp(last_msg.timestamp);

                                            let _ = tx_clone.send(UIMessage::UpdateChatPreview {
                                                chat_jid: chat_jid_clone,
                                                last_message: content,
                                                timestamp,
                                            });
                                        }
                                    }
                                });
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to load chats: {}", e);
                            show_error(&handle, &format!("Failed to load chats: {}", e));
                        }
                    }
                } else {
                    tracing::warn!("No account selected, cannot load chats");
                }
            }
            UIMessage::UpdateChatPreview {
                chat_jid,
                last_message,
                timestamp,
            } => {
                update_chat_preview(&handle, &chat_jid, &last_message, &timestamp);
            }
            UIMessage::UpdateChatName { chat_jid, name } => {
                update_chat_name(&handle, &chat_jid, &name);
            }
        }
    }
}

fn format_timestamp(timestamp: i64) -> String {
    use chrono::{DateTime, Local, Utc};

    let dt = DateTime::<Utc>::from_timestamp(timestamp, 0).unwrap_or_else(|| Utc::now());
    let local: DateTime<Local> = dt.into();
    let now = Local::now();

    if local.date_naive() == now.date_naive() {
        local.format("%H:%M").to_string()
    } else if local.year() == now.year() {
        local.format("%d/%m").to_string()
    } else {
        local.format("%d/%m/%y").to_string()
    }
}

/// Handle worker events and send UI messages
#[tracing::instrument(skip(handle, _worker, tx))]
async fn handle_worker_event(
    handle: &Weak<Tina>,
    _worker: &Arc<TinaWorker>,
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
            // Update user profile with phone number
            update_user_profile(handle, Some(&account_id), phone_number.as_deref(), None);
            let _ = tx.send(UIMessage::ShowSyncing);
        }
        WorkerEvent::Connected {
            account_id,
            phone_number,
        } => {
            tracing::info!("Connected: {} (phone: {:?})", account_id, phone_number);
            // Update user profile with phone number
            update_user_profile(handle, Some(&account_id), phone_number.as_deref(), None);
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
            show_scene(handle, Scene::InApp);
        }
        WorkerEvent::NewMessage {
            account_id: _,
            chat_jid,
            content,
            timestamp,
        } => {
            tracing::debug!("New message in chat {}", chat_jid);
            let content = content.unwrap_or_else(|| "[Media]".to_string());
            let formatted_timestamp = format_timestamp(timestamp);
            update_chat_preview(handle, &chat_jid, &content, &formatted_timestamp);
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
