// Service worker bridge.
//
// Owns a tokio runtime on a dedicated OS thread, where it instantiates a
// `TinaWorker`. The UI sends `Cmd`s over a `tokio::sync::mpsc` channel; the
// worker pushes `WorkerEvent`s back into the relm4 component as `AppMsg`.

use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;

use relm4::Sender;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tracing::{error, info};

use tina_worker::{TinaWorker, WorkerEvent};

use crate::app::AppMsg;

/// Commands the UI can send to the worker thread.
#[derive(Debug)]
pub enum Cmd {
    /// Boot: list accounts, auto-create on empty, start chosen account.
    Initialize,
    /// Re-emits the latest snapshot of chats for the active account.
    LoadChats,
    /// Open (or re-load) a chat: fetches metadata + last 200 messages and
    /// emits `AppMsg::ChatOpened`. Use `FocusChat` for the cheap "user just
    /// switched tabs" case.
    OpenChat(String),
    /// Tell the worker which open chat is currently focused — drives where
    /// `MessagesAppended` events are routed. `None` = no chat focused.
    FocusChat(Option<String>),
    /// Send a plain-text message to a chat.
    SendText { chat_id: String, text: String },
    /// Trigger reconcile (whatsmeow → tina).
    Repair,
    /// Trigger an async media download for a specific message.
    DownloadMedia { message_id: String },
    /// Lazy-load older messages (page back). The UI passes the timestamp
    /// of its currently-oldest row; the worker returns the next batch
    /// strictly older than that.
    LoadOlder {
        chat_id: String,
        before_ts: i64,
        limit: i64,
    },
    /// Logout the active account.
    Logout,
    /// Shut down the worker thread.
    Shutdown,
}

#[derive(Clone)]
pub struct ServiceHandle {
    tx: mpsc::UnboundedSender<Cmd>,
}

impl ServiceHandle {
    pub fn send(&self, cmd: Cmd) {
        if let Err(e) = self.tx.send(cmd) {
            error!("service tx closed: {e}");
        }
    }
}

pub struct ServiceWorker {
    pub handle: ServiceHandle,
    _thread: JoinHandle<()>,
}

impl ServiceWorker {
    pub fn spawn(nanachi_dir: PathBuf, app_sender: Sender<AppMsg>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let app_sender_thread = app_sender.clone();
        let thread = std::thread::Builder::new()
            .name("tina-service".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime");
                rt.block_on(run(nanachi_dir, rx, app_sender_thread));
            })
            .expect("spawn service thread");
        Self {
            handle: ServiceHandle { tx },
            _thread: thread,
        }
    }
}

async fn run(
    nanachi_dir: PathBuf,
    mut rx: mpsc::UnboundedReceiver<Cmd>,
    app: Sender<AppMsg>,
) {
    let mut worker = match TinaWorker::new(nanachi_dir).await {
        Ok(w) => w,
        Err(e) => {
            let _ = app.send(AppMsg::FatalError(format!("worker init: {e}")));
            return;
        }
    };
    let event_rx = match worker.take_event_receiver() {
        Some(rx) => rx,
        None => {
            let _ = app.send(AppMsg::FatalError("event channel taken".into()));
            return;
        }
    };

    let worker = Arc::new(worker);
    if let Err(e) = worker.start().await {
        let _ = app.send(AppMsg::FatalError(format!("worker start: {e}")));
        return;
    }

    // Forward worker events to the UI.
    let app_evt = app.clone();
    let event_pump = tokio::spawn(forward_events(event_rx, app_evt));

    // Active account for this worker session. Single-account today; widening
    // is a sender->Cmd refactor away.
    let selected: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    while let Some(cmd) = rx.recv().await {
        match cmd {
            Cmd::Initialize => {
                if let Err(e) = initialize(&worker, &app, &selected).await {
                    let _ = app.send(AppMsg::FatalError(format!("initialize: {e}")));
                }
            }
            Cmd::LoadChats => {
                let acc = selected.lock().await.clone();
                if let Some(account_id) = acc {
                    match worker.list_chat_rows(&account_id).await {
                        Ok(rows) => {
                            let _ = app.send(AppMsg::ChatsUpserted(rows));
                        }
                        Err(e) => error!("list_chat_rows: {e}"),
                    }
                }
            }
            Cmd::OpenChat(id) => {
                let acc = selected.lock().await.clone();
                let Some(account_id) = acc else { continue };
                worker.set_active_chat(&account_id, Some(&id)).await;
                let row = worker.get_chat_row(&account_id, &id).await.ok().flatten();
                let (name, kind) = row
                    .as_ref()
                    .map(|r| (r.name.clone(), r.kind.clone()))
                    .unwrap_or_else(|| (id.clone(), "unknown".into()));
                // Initial page is small (50). The chat tab will lazy-load
                // older batches as the user scrolls up; keeping the
                // first paint cheap matters more than guaranteeing the
                // whole history is in memory.
                let messages = worker
                    .get_message_rows(&account_id, &id, 50, 0)
                    .await
                    .unwrap_or_default();
                let _ = app.send(AppMsg::ChatOpened {
                    chat_id: Some(id),
                    name,
                    kind,
                    messages,
                });
            }
            Cmd::FocusChat(chat_id) => {
                let acc = selected.lock().await.clone();
                let Some(account_id) = acc else { continue };
                worker
                    .set_active_chat(&account_id, chat_id.as_deref())
                    .await;
            }
            Cmd::SendText { chat_id, text } => {
                let acc = selected.lock().await.clone();
                let Some(account_id) = acc else { continue };
                if let Err(e) = worker.send_message(&account_id, &chat_id, &text).await {
                    error!("send_message: {e}");
                    continue;
                }
                // Belt-and-suspenders: wait for the dispatcher's 100ms
                // flush window to elapse, then re-fetch the tail of the
                // chat and re-emit MessagesAppended ourselves. ChatTab
                // dedups by message_id, so this is a no-op when the
                // dispatcher already routed the synthetic echo through.
                let app = app.clone();
                let worker = worker.clone();
                let chat_id_clone = chat_id.clone();
                let account_id_clone = account_id.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    match worker
                        .get_message_rows(&account_id_clone, &chat_id_clone, 20, 0)
                        .await
                    {
                        Ok(messages) if !messages.is_empty() => {
                            let _ = app.send(AppMsg::MessagesAppended {
                                chat_id: chat_id_clone,
                                messages,
                            });
                        }
                        Ok(_) => {}
                        Err(e) => error!("post-send fetch: {e}"),
                    }
                });
            }
            Cmd::Repair => {
                let acc = selected.lock().await.clone();
                let Some(account_id) = acc else { continue };
                let _ = app.send(AppMsg::RepairStarted);
                if let Err(e) = worker.reconcile_account(&account_id).await {
                    error!("reconcile: {e}");
                    let _ = app.send(AppMsg::RepairEnded);
                }
            }
            Cmd::LoadOlder {
                chat_id,
                before_ts,
                limit,
            } => {
                let acc = selected.lock().await.clone();
                let Some(account_id) = acc else { continue };
                match worker
                    .get_message_rows_before(&account_id, &chat_id, before_ts, limit)
                    .await
                {
                    Ok(messages) => {
                        let _ = app.send(AppMsg::OlderMessagesLoaded {
                            chat_id,
                            messages,
                            reached_top: false,
                        });
                    }
                    Err(e) => error!("load_older: {e}"),
                }
            }
            Cmd::DownloadMedia { message_id } => {
                let acc = selected.lock().await.clone();
                let Some(account_id) = acc else { continue };
                if let Err(e) = worker.download_media(&account_id, &message_id).await {
                    error!("download_media: {e}");
                    let _ = app.send(AppMsg::MediaDownloadFailed {
                        message_id,
                        error: e.to_string(),
                    });
                }
            }
            Cmd::Logout => {
                let acc = selected.lock().await.clone();
                if let Some(account_id) = acc {
                    if let Err(e) = worker.logout_account(&account_id).await {
                        error!("logout: {e}");
                    }
                }
            }
            Cmd::Shutdown => break,
        }
    }
    event_pump.abort();
    let _ = worker.stop().await;
}

async fn initialize(
    worker: &Arc<TinaWorker>,
    app: &Sender<AppMsg>,
    selected: &Arc<Mutex<Option<String>>>,
) -> color_eyre::Result<()> {
    let mut accounts = worker.list_accounts().await?;
    let account = if let Some(first) = accounts.drain(..).next() {
        first
    } else {
        // Auto-create one (UUID v7 keeps SQLite B-tree happy).
        let id = uuid::Uuid::now_v7().to_string();
        worker.create_account(&id, None).await?
    };

    *selected.lock().await = Some(account.id.clone());

    if account.phone_number.is_some() {
        let _ = app.send(AppMsg::ShowInApp);
        if let Ok(rows) = worker.list_chat_rows(&account.id).await {
            let _ = app.send(AppMsg::ChatsUpserted(rows));
        }
    } else {
        let _ = app.send(AppMsg::ShowQrLogin);
    }
    worker.start_account(&account.id).await?;
    Ok(())
}

async fn forward_events(
    mut event_rx: mpsc::Receiver<WorkerEvent>,
    app: Sender<AppMsg>,
) {
    while let Some(event) = event_rx.recv().await {
        match event {
            WorkerEvent::NanachiReady => info!("nanachi ready"),
            WorkerEvent::AccountReady { account_id } => {
                info!(%account_id, "account ready");
            }
            WorkerEvent::QrCode { qr, .. } => {
                let _ = app.send(AppMsg::QrCode(qr));
            }
            WorkerEvent::Connected {
                account_id,
                phone_number,
            } => {
                let _ = app.send(AppMsg::Connected {
                    account_id,
                    phone_number,
                });
            }
            WorkerEvent::Disconnected { reason, .. } => {
                let _ = app.send(AppMsg::Disconnected(reason));
            }
            WorkerEvent::LoggedOut { .. } => {
                let _ = app.send(AppMsg::LoggedOut);
            }
            WorkerEvent::ChatsUpserted { rows, .. } => {
                let _ = app.send(AppMsg::ChatsUpserted(rows));
            }
            WorkerEvent::MessagesAppended {
                chat_id, messages, ..
            } => {
                tracing::info!(
                    chat = %chat_id,
                    count = messages.len(),
                    "service: MessagesAppended → AppMsg",
                );
                let _ = app.send(AppMsg::MessagesAppended { chat_id, messages });
            }
            WorkerEvent::HistorySyncComplete { messages_count, .. } => {
                info!(messages_count, "history sync done");
                let _ = app.send(AppMsg::HistorySyncDone);
                let _ = app.send(AppMsg::RepairEnded);
            }
            WorkerEvent::ReconcileProgress {
                stage,
                current,
                total,
                indeterminate,
                ..
            } => {
                let _ = app.send(AppMsg::RepairProgress {
                    stage,
                    current,
                    total,
                    indeterminate,
                });
            }
            WorkerEvent::Error { error, .. } => {
                error!(%error, "worker error");
                let _ = app.send(AppMsg::Toast(error));
            }
            WorkerEvent::MediaDownloadProgress {
                message_id,
                current,
                total,
                ..
            } => {
                let _ = app.send(AppMsg::MediaDownloadProgress {
                    message_id,
                    current,
                    total,
                });
            }
            WorkerEvent::MediaReady {
                affected_message_ids,
                path,
                mimetype,
                ..
            } => {
                let _ = app.send(AppMsg::MediaReady {
                    message_ids: affected_message_ids,
                    path,
                    mimetype,
                });
            }
            WorkerEvent::MediaDownloadFailed {
                message_id, error, ..
            } => {
                let _ = app.send(AppMsg::MediaDownloadFailed { message_id, error });
            }
        }
    }
}
