// Forward `WorkerEvent`s coming off the tina-worker channel into
// `AppMsg`s sent to the UI thread.

use relm4::Sender;
use tokio::sync::mpsc;
use tracing::{error, info};

use tina_worker::WorkerEvent;

use crate::app::AppMsg;

pub(super) async fn forward_events(
    mut event_rx: mpsc::Receiver<WorkerEvent>,
    app: Sender<AppMsg>,
) {
    while let Some(event) = event_rx.recv().await {
        forward_one(&app, event);
    }
}

fn forward_one(app: &Sender<AppMsg>, event: WorkerEvent) {
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
            jid,
            push_name,
        } => {
            info!(%account_id, ?phone_number, ?jid, "worker reported Connected");
            let _ = app.send(AppMsg::Connected {
                account_id,
                phone_number,
                jid,
                push_name,
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
        WorkerEvent::StatusAuthorsUpserted { rows, .. } => {
            let _ = app.send(AppMsg::StatusAuthorsUpserted(rows));
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
        WorkerEvent::HistorySyncProgress {
            sync_type,
            progress,
            ..
        } => {
            info!(%sync_type, progress, "history sync progress");
            let _ = app.send(AppMsg::HistorySyncProgress {
                sync_type,
                progress,
            });
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
        WorkerEvent::Notice { message, .. } => {
            let _ = app.send(AppMsg::Toast(message));
        }
        WorkerEvent::ReceiptUpdate {
            message_ids, status, ..
        } => {
            let _ = app.send(AppMsg::ReceiptUpdate { message_ids, status });
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
        WorkerEvent::AvatarReady { jid, path, .. } => {
            let _ = app.send(AppMsg::AvatarReady { jid, path });
        }
        WorkerEvent::AvatarFailed { jid, error, .. } => {
            tracing::warn!(%jid, %error, "avatar fetch failed");
        }
    }
}
