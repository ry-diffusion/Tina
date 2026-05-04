// Implementations of every `Cmd` arm. The dispatcher in `runtime.rs`
// reads commands off the channel and calls into these.

use std::sync::Arc;

use relm4::Sender;
use tokio::sync::Mutex;
use tracing::error;

use tina_worker::TinaWorker;

use crate::app::AppMsg;

use super::cmd::Cmd;

pub(super) async fn initialize(
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

pub(super) async fn handle(
    cmd: Cmd,
    worker: &Arc<TinaWorker>,
    app: &Sender<AppMsg>,
    selected: &Arc<Mutex<Option<String>>>,
) -> bool {
    match cmd {
        Cmd::Initialize => {
            if let Err(e) = initialize(worker, app, selected).await {
                let _ = app.send(AppMsg::FatalError(format!("initialize: {e}")));
            }
        }
        Cmd::LoadChats => load_chats(worker, app, selected).await,
        Cmd::OpenChat(id) => open_chat(worker, app, selected, id).await,
        Cmd::CloseChat(chat_id) => {
            if let Some(account_id) = selected.lock().await.clone() {
                worker.remove_open_chat(&account_id, &chat_id).await;
            }
        }
        Cmd::SendText { chat_id, text } => send_text(worker, app, selected, chat_id, text).await,
        Cmd::Repair => repair(worker, app, selected).await,
        Cmd::LoadOlder {
            chat_id,
            before_ts,
            limit,
        } => load_older(worker, app, selected, chat_id, before_ts, limit).await,
        Cmd::FetchAvatar { jid } => fetch_avatar(worker, selected, jid).await,
        Cmd::DownloadMedia { message_id } => {
            download_media(worker, app, selected, message_id).await
        }
        Cmd::SetChatPinned { chat_id, pinned } => {
            set_chat_pinned(worker, app, selected, chat_id, pinned).await
        }
        Cmd::Logout => logout(worker, selected).await,
        Cmd::Shutdown => return false,
    }
    true
}

async fn load_chats(
    worker: &Arc<TinaWorker>,
    app: &Sender<AppMsg>,
    selected: &Arc<Mutex<Option<String>>>,
) {
    let Some(account_id) = selected.lock().await.clone() else {
        return;
    };
    match worker.list_chat_rows(&account_id).await {
        Ok(rows) => {
            let _ = app.send(AppMsg::ChatsUpserted(rows));
        }
        Err(e) => error!("list_chat_rows: {e}"),
    }
}

async fn open_chat(
    worker: &Arc<TinaWorker>,
    app: &Sender<AppMsg>,
    selected: &Arc<Mutex<Option<String>>>,
    id: String,
) {
    let Some(account_id) = selected.lock().await.clone() else {
        return;
    };
    worker.add_open_chat(&account_id, &id).await;
    let row = worker.get_chat_row(&account_id, &id).await.ok().flatten();
    let (name, kind) = row
        .as_ref()
        .map(|r| (r.name.clone(), r.kind.clone()))
        .unwrap_or_else(|| (id.clone(), "unknown".into()));
    // Initial page is small (50). The chat tab will lazy-load older
    // batches as the user scrolls up; keeping the first paint cheap
    // matters more than guaranteeing the whole history is in memory.
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

async fn send_text(
    worker: &Arc<TinaWorker>,
    app: &Sender<AppMsg>,
    selected: &Arc<Mutex<Option<String>>>,
    chat_id: String,
    text: String,
) {
    let Some(account_id) = selected.lock().await.clone() else {
        return;
    };
    if let Err(e) = worker.send_message(&account_id, &chat_id, &text).await {
        error!("send_message: {e}");
        return;
    }
    // Belt-and-suspenders: wait for the dispatcher's 100ms flush window
    // to elapse, then re-fetch the tail of the chat and re-emit
    // MessagesAppended ourselves. ChatTab dedups by message_id, so
    // this is a no-op when the dispatcher already routed the
    // synthetic echo through.
    let app = app.clone();
    let worker = worker.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        match worker
            .get_message_rows(&account_id, &chat_id, 20, 0)
            .await
        {
            Ok(messages) if !messages.is_empty() => {
                let _ = app.send(AppMsg::MessagesAppended {
                    chat_id,
                    messages,
                });
            }
            Ok(_) => {}
            Err(e) => error!("post-send fetch: {e}"),
        }
    });
}

async fn repair(
    worker: &Arc<TinaWorker>,
    app: &Sender<AppMsg>,
    selected: &Arc<Mutex<Option<String>>>,
) {
    let Some(account_id) = selected.lock().await.clone() else {
        return;
    };
    let _ = app.send(AppMsg::RepairStarted);
    if let Err(e) = worker.reconcile_account(&account_id).await {
        error!("reconcile: {e}");
        let _ = app.send(AppMsg::RepairEnded);
    }
}

async fn load_older(
    worker: &Arc<TinaWorker>,
    app: &Sender<AppMsg>,
    selected: &Arc<Mutex<Option<String>>>,
    chat_id: String,
    before_ts: i64,
    limit: i64,
) {
    let Some(account_id) = selected.lock().await.clone() else {
        return;
    };
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

async fn fetch_avatar(
    worker: &Arc<TinaWorker>,
    selected: &Arc<Mutex<Option<String>>>,
    jid: String,
) {
    let Some(account_id) = selected.lock().await.clone() else {
        return;
    };
    if let Err(e) = worker.fetch_avatar(&account_id, &jid).await {
        error!("fetch_avatar: {e}");
    }
}

async fn download_media(
    worker: &Arc<TinaWorker>,
    app: &Sender<AppMsg>,
    selected: &Arc<Mutex<Option<String>>>,
    message_id: String,
) {
    let Some(account_id) = selected.lock().await.clone() else {
        return;
    };
    if let Err(e) = worker.download_media(&account_id, &message_id).await {
        error!("download_media: {e}");
        let _ = app.send(AppMsg::MediaDownloadFailed {
            message_id,
            error: e.to_string(),
        });
    }
}

async fn set_chat_pinned(
    worker: &Arc<TinaWorker>,
    app: &Sender<AppMsg>,
    selected: &Arc<Mutex<Option<String>>>,
    chat_id: String,
    pinned: bool,
) {
    let Some(account_id) = selected.lock().await.clone() else {
        return;
    };
    if let Err(e) = worker.set_chat_pinned(&account_id, &chat_id, pinned).await {
        error!("set_chat_pinned: {e}");
        return;
    }
    // Re-emit the chat list so the sidebar picks up the updated
    // `pinned` flag (drives both the row's pin icon and its sort
    // position).
    match worker.list_chat_rows(&account_id).await {
        Ok(rows) => {
            let _ = app.send(AppMsg::ChatsUpserted(rows));
        }
        Err(e) => error!("list_chat_rows after pin: {e}"),
    }
}

async fn logout(worker: &Arc<TinaWorker>, selected: &Arc<Mutex<Option<String>>>) {
    if let Some(account_id) = selected.lock().await.clone() {
        worker.clear_open_chats(&account_id).await;
        if let Err(e) = worker.logout_account(&account_id).await {
            error!("logout: {e}");
        }
    }
}
