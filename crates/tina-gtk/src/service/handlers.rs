// Implementations of every `Cmd` arm. The dispatcher in `runtime.rs`
// reads commands off the channel and calls into these.
//
// All handlers go through `state.read().await.active_account()` to
// resolve the target account. The `ServiceState` struct keeps a per-
// account registry keyed by `account_id` plus an `active` pointer; the
// UI flips the pointer when the user switches accounts. Handlers that
// silently no-op when no account is active also cover the boot window
// before `Initialize` has registered one.

use std::sync::Arc;

use relm4::Sender;
use tracing::{error, info};

use tina_worker::TinaWorker;

use crate::app::AppMsg;

use super::cmd::Cmd;
use super::state::SharedState;

async fn active_account(state: &SharedState) -> Option<String> {
    state.read().await.active_account()
}

pub(super) async fn initialize(
    worker: &Arc<TinaWorker>,
    app: &Sender<AppMsg>,
    state: &SharedState,
) -> color_eyre::Result<()> {
    let mut accounts = worker.list_accounts().await?;
    let account = if let Some(first) = accounts.drain(..).next() {
        first
    } else {
        // Auto-create one (UUID v7 keeps SQLite B-tree happy).
        let id = uuid::Uuid::now_v7().to_string();
        worker.create_account(&id, None).await?
    };

    state.write().await.set_active(account.id.clone());

    if account.phone_number.is_some() {
        // Returning user: skip QR + Syncing scenes and go straight to
        // the chat list. The whatsmeow auto-reconnect will still emit
        // HistorySync events in the background — they show up in logs
        // but don't visibly transition the UI.
        info!(
            account_id = %account.id,
            phone = %account.phone_number.as_deref().unwrap_or(""),
            "[sync] returning user — going straight to InApp; \
             whatsmeow HistorySync will run in the background",
        );
        let _ = app.send(AppMsg::ShowInApp);
        if let Ok(rows) = worker.list_chat_rows(&account.id).await {
            let _ = app.send(AppMsg::ChatsUpserted(rows));
        }
    } else {
        info!(
            account_id = %account.id,
            "[sync] new account — showing QR login",
        );
        let _ = app.send(AppMsg::ShowQrLogin);
    }
    worker.start_account(&account.id).await?;
    info!(account_id = %account.id, "[sync] start_account dispatched to nanachi");
    Ok(())
}

pub(super) async fn handle(
    cmd: Cmd,
    worker: &Arc<TinaWorker>,
    app: &Sender<AppMsg>,
    state: &SharedState,
) -> bool {
    match cmd {
        Cmd::Initialize => {
            if let Err(e) = initialize(worker, app, state).await {
                let _ = app.send(AppMsg::FatalError(format!("initialize: {e}")));
            }
        }
        Cmd::LoadChats => load_chats(worker, app, state).await,
        Cmd::LoadStatuses => load_statuses(worker, app, state).await,
        Cmd::OpenStatusAuthor { sender_jid, name } => {
            open_status_author(worker, app, state, sender_jid, name).await
        }
        Cmd::OpenChat(id) => open_chat(worker, app, state, id).await,
        Cmd::CloseChat(chat_id) => {
            if let Some(account_id) = active_account(state).await {
                worker.remove_open_chat(&account_id, &chat_id).await;
            }
        }
        Cmd::SendText { chat_id, text } => send_text(worker, app, state, chat_id, text).await,
        Cmd::Repair => repair(worker, app, state).await,
        Cmd::LoadOlder {
            chat_id,
            before_ts,
            limit,
        } => load_older(worker, app, state, chat_id, before_ts, limit).await,
        Cmd::FetchAvatar { jid } => fetch_avatar(worker, state, jid).await,
        Cmd::RefreshChat { chat_jid } => refresh_chat(worker, state, chat_jid).await,
        Cmd::DownloadMedia { message_id } => {
            download_media(worker, app, state, message_id).await
        }
        Cmd::SetChatPinned { chat_id, pinned } => {
            set_chat_pinned(worker, app, state, chat_id, pinned).await
        }
        Cmd::Logout => logout(worker, state).await,
        Cmd::LoadPreferences => load_preferences(worker, app).await,
        Cmd::SetDownloadMethod(m) => set_download_method(worker, m).await,
        Cmd::ClearMediaCache => clear_media_cache(worker, app).await,
        Cmd::ClearAvatarCache => clear_avatar_cache(worker, app).await,
        Cmd::Shutdown => return false,
    }
    true
}

async fn load_chats(worker: &Arc<TinaWorker>, app: &Sender<AppMsg>, state: &SharedState) {
    let Some(account_id) = active_account(state).await else {
        return;
    };
    match worker.list_chat_rows(&account_id).await {
        Ok(rows) => {
            let _ = app.send(AppMsg::ChatsUpserted(rows));
        }
        Err(e) => error!("list_chat_rows: {e}"),
    }
}

async fn load_statuses(worker: &Arc<TinaWorker>, app: &Sender<AppMsg>, state: &SharedState) {
    let Some(account_id) = active_account(state).await else {
        return;
    };
    match worker.list_status_authors(&account_id).await {
        Ok(rows) => {
            let _ = app.send(AppMsg::StatusAuthorsUpserted(rows));
        }
        Err(e) => error!("list_status_authors: {e}"),
    }
}

async fn open_status_author(
    worker: &Arc<TinaWorker>,
    app: &Sender<AppMsg>,
    state: &SharedState,
    sender_jid: String,
    name: String,
) {
    let Some(account_id) = active_account(state).await else {
        return;
    };
    info!(%sender_jid, %name, "[stories] fetching posts for author");
    match worker
        .get_message_rows(&account_id, "status@broadcast", 200, 0)
        .await
    {
        Ok(rows) => {
            let total = rows.len();
            let posts: Vec<_> = rows
                .into_iter()
                .filter(|r| {
                    r.sender_jid.as_deref() == Some(sender_jid.as_str())
                        || r.sender_contact_id.as_deref() == Some(sender_jid.as_str())
                })
                .collect();
            info!(
                total,
                matched = posts.len(),
                "[stories] filter result",
            );
            // `get_message_rows` returns newest-first for the chat
            // tab; the carousel reads oldest-first so the user's
            // "back" gesture moves into older posts.
            let mut posts = posts;
            posts.sort_by_key(|r| r.timestamp);
            let _ = app.send(AppMsg::ShowStoriesViewer { name, posts });
        }
        Err(e) => {
            error!("get_message_rows for status author: {e}");
        }
    }
}

async fn open_chat(
    worker: &Arc<TinaWorker>,
    app: &Sender<AppMsg>,
    state: &SharedState,
    id: String,
) {
    let Some(account_id) = active_account(state).await else {
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
    state: &SharedState,
    chat_id: String,
    text: String,
) {
    let Some(account_id) = active_account(state).await else {
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

async fn repair(worker: &Arc<TinaWorker>, app: &Sender<AppMsg>, state: &SharedState) {
    let Some(account_id) = active_account(state).await else {
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
    state: &SharedState,
    chat_id: String,
    before_ts: i64,
    limit: i64,
) {
    let Some(account_id) = active_account(state).await else {
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

async fn fetch_avatar(worker: &Arc<TinaWorker>, state: &SharedState, jid: String) {
    let Some(account_id) = active_account(state).await else {
        return;
    };
    if let Err(e) = worker.fetch_avatar(&account_id, &jid).await {
        error!("fetch_avatar: {e}");
    }
}

async fn refresh_chat(worker: &Arc<TinaWorker>, state: &SharedState, chat_jid: String) {
    let Some(account_id) = active_account(state).await else {
        return;
    };
    if let Err(e) = worker.refresh_chat(&account_id, &chat_jid).await {
        error!("refresh_chat: {e}");
    }
}

async fn download_media(
    worker: &Arc<TinaWorker>,
    app: &Sender<AppMsg>,
    state: &SharedState,
    message_id: String,
) {
    let Some(account_id) = active_account(state).await else {
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
    state: &SharedState,
    chat_id: String,
    pinned: bool,
) {
    let Some(account_id) = active_account(state).await else {
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

async fn logout(worker: &Arc<TinaWorker>, state: &SharedState) {
    if let Some(account_id) = active_account(state).await {
        worker.clear_open_chats(&account_id).await;
        if let Err(e) = worker.logout_account(&account_id).await {
            error!("logout: {e}");
        }
    }
}

async fn load_preferences(worker: &Arc<TinaWorker>, app: &Sender<AppMsg>) {
    use crate::components::settings::DownloadMethod;
    let method = worker
        .get_setting(DownloadMethod::KEY)
        .await
        .ok()
        .flatten()
        .map(|s| DownloadMethod::from_str(&s))
        .unwrap_or(DownloadMethod::OnDemand);
    let pid = worker.nanachi_pid().await;
    let _ = app.send(AppMsg::PreferencesLoaded { method, pid });
}

async fn set_download_method(
    worker: &Arc<TinaWorker>,
    m: crate::components::settings::DownloadMethod,
) {
    if let Err(e) = worker.put_setting(
        crate::components::settings::DownloadMethod::KEY,
        m.as_str(),
    )
    .await {
        error!("put_setting download_method: {e}");
    }
}

/// Walk a directory tree and remove every regular file. We keep the
/// top-level directory itself so the Go side doesn't need to recreate
/// it on the next download.
fn rm_files_in(path: &std::path::Path) -> std::io::Result<u64> {
    let mut count = 0u64;
    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if entry.file_type()?.is_dir() {
            count += rm_files_in(&p)?;
            // Try to drop the now-empty shard dir (`media/aa/`); the
            // failure is non-fatal — Go will recreate it on demand.
            let _ = std::fs::remove_dir(&p);
        } else if std::fs::remove_file(&p).is_ok() {
            count += 1;
        }
    }
    Ok(count)
}

fn data_dir() -> std::path::PathBuf {
    if let Some(dirs) = directories::ProjectDirs::from("com.br", "zesmoi", "tina") {
        dirs.data_dir().to_path_buf()
    } else if let Some(home) = std::env::var_os("HOME") {
        std::path::PathBuf::from(home).join(".local/share/tina")
    } else {
        std::path::PathBuf::from(".")
    }
}

async fn clear_media_cache(worker: &Arc<TinaWorker>, app: &Sender<AppMsg>) {
    let path = data_dir().join("media");
    let n = match rm_files_in(&path) {
        Ok(n) => n,
        Err(e) => {
            error!("clear_media_cache rm: {e}");
            let _ = app.send(AppMsg::Toast(format!("Failed to clear media: {e}")));
            return;
        }
    };
    if let Err(e) = worker.clear_all_media_paths().await {
        error!("clear_all_media_paths: {e}");
    }
    let _ = app.send(AppMsg::Toast(format!("Cleared {n} media file(s)")));
}

async fn clear_avatar_cache(worker: &Arc<TinaWorker>, app: &Sender<AppMsg>) {
    let path = data_dir().join("avatars");
    let n = match rm_files_in(&path) {
        Ok(n) => n,
        Err(e) => {
            error!("clear_avatar_cache rm: {e}");
            let _ = app.send(AppMsg::Toast(format!("Failed to clear avatars: {e}")));
            return;
        }
    };
    if let Err(e) = worker.clear_all_avatar_paths().await {
        error!("clear_all_avatar_paths: {e}");
    }
    let _ = app.send(AppMsg::Toast(format!("Cleared {n} avatar file(s)")));
}
