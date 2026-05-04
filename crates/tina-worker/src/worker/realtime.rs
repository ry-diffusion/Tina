// Realtime IPC events: small, low-volume, processed inline (don't go
// through the DirtyBuffer).

use std::sync::Arc;

use tokio::sync::mpsc;

use tina_core::IpcEvent;
use tina_db::TinaDb;

use crate::error::Result;
use crate::events::WorkerEvent;

pub(super) async fn handle_realtime_event(
    db: &TinaDb,
    event_tx: &mpsc::Sender<WorkerEvent>,
    event: IpcEvent,
) -> Result<()> {
    match event {
        IpcEvent::Ready { account_id } => handle_ready(event_tx, account_id).await,
        IpcEvent::QrCode { account_id, qr } => {
            let _ = event_tx.send(WorkerEvent::QrCode { account_id, qr }).await;
        }
        IpcEvent::PairingCode { account_id, code } => {
            tracing::info!("Pairing code for {}: {}", account_id, code);
        }
        IpcEvent::Connected {
            account_id,
            phone_number,
            jid,
            push_name,
        } => {
            handle_connected(db, event_tx, account_id, phone_number, jid, push_name).await?;
        }
        IpcEvent::Disconnected { account_id, reason } => {
            let _ = event_tx
                .send(WorkerEvent::Disconnected { account_id, reason })
                .await;
        }
        IpcEvent::LoggedOut { account_id } => {
            db.clear_account_identity(&account_id).await?;
            let _ = event_tx.send(WorkerEvent::LoggedOut { account_id }).await;
        }
        // Bulkables são consumidos pelo dispatcher antes de chegarem
        // aqui; se aparecerem no realtime handler é bug do roteamento.
        IpcEvent::ContactsUpsert { .. }
        | IpcEvent::GroupsUpsert { .. }
        | IpcEvent::MessagesUpsert { .. } => {
            tracing::error!("bulk event reached realtime handler — routing bug");
        }
        IpcEvent::ReconcileProgress {
            account_id,
            stage,
            current,
            total,
            indeterminate,
        } => {
            let _ = event_tx
                .send(WorkerEvent::ReconcileProgress {
                    account_id,
                    stage,
                    current,
                    total,
                    indeterminate,
                })
                .await;
        }
        IpcEvent::HistorySyncComplete {
            account_id,
            messages_count,
        } => {
            let _ = event_tx
                .send(WorkerEvent::HistorySyncComplete {
                    account_id,
                    messages_count,
                })
                .await;
        }
        IpcEvent::HistorySyncProgress {
            account_id,
            sync_type,
            progress,
        } => {
            let _ = event_tx
                .send(WorkerEvent::HistorySyncProgress {
                    account_id,
                    sync_type,
                    progress,
                })
                .await;
        }
        IpcEvent::Error { account_id, error } => {
            let _ = event_tx
                .send(WorkerEvent::Error { account_id, error })
                .await;
        }
        IpcEvent::MediaDownloadProgress {
            account_id,
            message_id,
            current,
            total,
        } => {
            let _ = event_tx
                .send(WorkerEvent::MediaDownloadProgress {
                    account_id,
                    message_id,
                    current,
                    total,
                })
                .await;
        }
        IpcEvent::MediaDownloaded {
            account_id,
            message_id,
            path,
            sha256,
            mimetype,
        } => {
            handle_media_downloaded(db, event_tx, account_id, message_id, path, sha256, mimetype)
                .await;
        }
        IpcEvent::MediaDownloadFailed {
            account_id,
            message_id,
            error,
        } => {
            let _ = db
                .set_media_status(&account_id, &message_id, "failed")
                .await;
            let _ = event_tx
                .send(WorkerEvent::MediaDownloadFailed {
                    account_id,
                    message_id,
                    error,
                })
                .await;
        }
        IpcEvent::AvatarUpdated {
            account_id,
            jid,
            path,
        } => {
            // Persist before forwarding so the UI's next list_chat_rows
            // already returns the path.
            if let Err(e) = db.set_avatar_path(&account_id, &jid, &path).await {
                tracing::error!("set_avatar_path: {e}");
            }
            let _ = event_tx
                .send(WorkerEvent::AvatarReady {
                    account_id,
                    jid,
                    path,
                })
                .await;
        }
        IpcEvent::AvatarFailed {
            account_id,
            jid,
            error,
        } => {
            let _ = event_tx
                .send(WorkerEvent::AvatarFailed {
                    account_id,
                    jid,
                    error,
                })
                .await;
        }
        IpcEvent::CommandResult {
            command_id,
            success,
            error,
            ..
        } => {
            if !success {
                tracing::warn!(
                    "Command {} failed: {}",
                    command_id,
                    error.unwrap_or_else(|| "<no error>".to_string())
                );
            }
        }
    }
    Ok(())
}

async fn handle_ready(event_tx: &mpsc::Sender<WorkerEvent>, account_id: String) {
    if account_id.is_empty() {
        let _ = event_tx.send(WorkerEvent::NanachiReady).await;
    } else {
        let _ = event_tx
            .send(WorkerEvent::AccountReady { account_id })
            .await;
    }
}

async fn handle_connected(
    db: &TinaDb,
    event_tx: &mpsc::Sender<WorkerEvent>,
    account_id: String,
    phone_number: Option<String>,
    jid: Option<String>,
    push_name: Option<String>,
) -> Result<()> {
    db.save_account_identity(&account_id, phone_number.as_deref(), jid.as_deref())
        .await?;
    let _ = event_tx
        .send(WorkerEvent::Connected {
            account_id,
            phone_number,
            jid,
            push_name,
        })
        .await;
    Ok(())
}

async fn handle_media_downloaded(
    db: &TinaDb,
    event_tx: &mpsc::Sender<WorkerEvent>,
    account_id: String,
    message_id: String,
    path: String,
    sha256: Option<String>,
    mimetype: Option<String>,
) {
    let affected = db
        .apply_media_downloaded(
            &account_id,
            &message_id,
            &path,
            sha256.as_deref(),
            mimetype.as_deref(),
        )
        .await
        .unwrap_or_else(|e| {
            tracing::error!("apply_media_downloaded: {e}");
            vec![message_id.clone()]
        });
    let _ = event_tx
        .send(WorkerEvent::MediaReady {
            account_id,
            affected_message_ids: affected,
            path,
            mimetype,
        })
        .await;
}

#[allow(dead_code)]
fn _arc_marker(_: Arc<TinaDb>) {}
