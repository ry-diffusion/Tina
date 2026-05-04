// IPC event pipeline: owns the `DirtyBuffer` + flush timer + correlation
// of CommandResult. The IPC reader never blocks on the DB; bulk events
// accumulate until flush. Realtime events (Connected, QR, etc.) process
// inline.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{RwLock, mpsc};
use tokio::time;

use tina_core::IpcEvent;
use tina_db::TinaDb;
use tina_ipc::{NanachiManager, SLOW_IPC_THRESHOLD};

use crate::events::WorkerEvent;

use super::buffer::{DirtyBuffer, FLUSH_THRESHOLD, FLUSH_WINDOW};
use super::flush::flush;
use super::realtime::handle_realtime_event;

/// Dispatcher: dono único do `DirtyBuffer` + timer de flush + correlação
/// de CommandResult. IPC reader nunca espera DB; eventos bulk acumulam
/// até flush. Eventos realtime processam inline.
pub(super) async fn dispatcher_loop(
    db: Arc<TinaDb>,
    event_tx: mpsc::Sender<WorkerEvent>,
    open_chats: Arc<RwLock<HashMap<String, HashSet<String>>>>,
    outstanding: Arc<std::sync::Mutex<HashMap<String, tina_ipc::CommandTiming>>>,
    mut raw_rx: mpsc::Receiver<String>,
) {
    let mut buffer = DirtyBuffer::default();
    let mut deadline: Option<time::Instant> = None;

    loop {
        // Sleep condicional: só fica pendente se buffer tem conteúdo.
        let timer = async {
            if let Some(t) = deadline {
                time::sleep_until(t).await;
            } else {
                std::future::pending::<()>().await;
            }
        };

        tokio::select! {
            biased;
            line = raw_rx.recv() => {
                let Some(line) = line else { break };
                let Some(event) = NanachiManager::parse_event(&line) else { continue };

                record_command_rtt(&event, &outstanding);

                let started = Instant::now();
                let kind = event_kind(&event);

                let bulked = route_event(&db, &event_tx, &mut buffer, event).await;

                if bulked {
                    if deadline.is_none() && !buffer.is_empty() {
                        deadline = Some(time::Instant::now() + FLUSH_WINDOW);
                    }
                    if buffer.total_count() >= FLUSH_THRESHOLD {
                        if let Err(e) = flush(&db, &event_tx, &open_chats, &mut buffer).await {
                            tracing::error!("flush error: {}", e);
                        }
                        deadline = None;
                    }
                }

                let elapsed = started.elapsed();
                if elapsed > SLOW_IPC_THRESHOLD {
                    tracing::warn!(
                        "🐌 evento {} levou {:?} (DB ou pipeline lento)",
                        kind,
                        elapsed
                    );
                }
            }
            _ = timer, if deadline.is_some() => {
                if let Err(e) = flush(&db, &event_tx, &open_chats, &mut buffer).await {
                    tracing::error!("flush error: {}", e);
                }
                deadline = None;
            }
        }
    }

    // Drain final ao fechar.
    if !buffer.is_empty() {
        let _ = flush(&db, &event_tx, &open_chats, &mut buffer).await;
    }
}

/// Route `event` to either the DirtyBuffer (returns `true`) or the
/// realtime handler (returns `false`).
async fn route_event(
    db: &TinaDb,
    event_tx: &mpsc::Sender<WorkerEvent>,
    buffer: &mut DirtyBuffer,
    event: IpcEvent,
) -> bool {
    match event {
        IpcEvent::MessagesUpsert {
            account_id,
            messages,
        } => {
            if !messages.is_empty() {
                buffer
                    .messages
                    .entry(account_id)
                    .or_default()
                    .extend(messages);
            }
            true
        }
        IpcEvent::ContactsUpsert {
            account_id,
            contacts,
        } => {
            if !contacts.is_empty() {
                buffer
                    .contacts
                    .entry(account_id)
                    .or_default()
                    .extend(contacts);
            }
            true
        }
        IpcEvent::GroupsUpsert {
            account_id,
            groups,
        } => {
            if !groups.is_empty() {
                buffer.groups.entry(account_id).or_default().extend(groups);
            }
            true
        }
        other => {
            if let Err(e) = handle_realtime_event(db, event_tx, other).await {
                tracing::error!("realtime handler error: {}", e);
            }
            false
        }
    }
}

fn record_command_rtt(
    event: &IpcEvent,
    outstanding: &Arc<std::sync::Mutex<HashMap<String, tina_ipc::CommandTiming>>>,
) {
    let IpcEvent::CommandResult { ref command_id, .. } = *event else {
        return;
    };
    let Ok(mut map) = outstanding.lock() else {
        return;
    };
    let Some(t) = map.remove(command_id) else {
        return;
    };
    let rtt = t.sent_at.elapsed();
    if rtt > SLOW_IPC_THRESHOLD {
        tracing::warn!("🐌 IPC round-trip {} → {:?}", t.kind, rtt);
    } else {
        tracing::trace!("IPC round-trip {} → {:?}", t.kind, rtt);
    }
}

pub(super) fn event_kind(e: &IpcEvent) -> &'static str {
    match e {
        IpcEvent::Ready { .. } => "Ready",
        IpcEvent::QrCode { .. } => "QrCode",
        IpcEvent::PairingCode { .. } => "PairingCode",
        IpcEvent::Connected { .. } => "Connected",
        IpcEvent::Disconnected { .. } => "Disconnected",
        IpcEvent::LoggedOut { .. } => "LoggedOut",
        IpcEvent::ContactsUpsert { .. } => "ContactsUpsert",
        IpcEvent::GroupsUpsert { .. } => "GroupsUpsert",
        IpcEvent::MessagesUpsert { .. } => "MessagesUpsert",
        IpcEvent::HistorySyncComplete { .. } => "HistorySyncComplete",
        IpcEvent::HistorySyncProgress { .. } => "HistorySyncProgress",
        IpcEvent::ReconcileProgress { .. } => "ReconcileProgress",
        IpcEvent::Error { .. } => "Error",
        IpcEvent::MediaDownloadProgress { .. } => "MediaDownloadProgress",
        IpcEvent::MediaDownloaded { .. } => "MediaDownloaded",
        IpcEvent::MediaDownloadFailed { .. } => "MediaDownloadFailed",
        IpcEvent::AvatarUpdated { .. } => "AvatarUpdated",
        IpcEvent::AvatarFailed { .. } => "AvatarFailed",
        IpcEvent::CommandResult { .. } => "CommandResult",
    }
}
