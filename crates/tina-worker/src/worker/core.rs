// `TinaWorker` itself + the methods that simply forward to the DB or
// the IPC manager. The dispatcher and DB-batch logic live in sibling
// modules.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{RwLock, mpsc};

use tina_core::IpcCommand;
use tina_db::{ChatRow, TinaDb};
use tina_ipc::NanachiManager;

use crate::error::Result;
use crate::events::WorkerEvent;

use super::dispatcher::dispatcher_loop;

pub struct TinaWorker {
    pub(super) db: Arc<TinaDb>,
    pub(super) nanachi: Arc<RwLock<NanachiManager>>,
    pub(super) event_tx: mpsc::Sender<WorkerEvent>,
    pub(super) event_rx: Option<mpsc::Receiver<WorkerEvent>>,
    /// Chats atualmente abertos como tab na UI, por conta. Apenas chats
    /// presentes aqui recebem `MessagesAppended` no flush — durante
    /// sync, dezenas de chats fechados receberiam eventos inúteis e a
    /// UI travava.
    pub(super) open_chats: Arc<RwLock<HashMap<String, HashSet<String>>>>,
}

impl TinaWorker {
    pub async fn new(nanachi_dir: PathBuf) -> Result<Self> {
        let db = TinaDb::new().await?;
        let nanachi = NanachiManager::new(nanachi_dir);
        let (event_tx, event_rx) = mpsc::channel(5000);
        Ok(Self {
            db: Arc::new(db),
            nanachi: Arc::new(RwLock::new(nanachi)),
            event_tx,
            event_rx: Some(event_rx),
            open_chats: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub fn take_event_receiver(&mut self) -> Option<mpsc::Receiver<WorkerEvent>> {
        self.event_rx.take()
    }

    pub async fn start(&self) -> Result<()> {
        let mut nanachi = self.nanachi.write().await;
        nanachi.start().await?;
        let ipc_rx = nanachi.take_event_receiver();
        let outstanding = nanachi.outstanding_handle();

        if let Some(rx) = ipc_rx {
            let db = self.db.clone();
            let event_tx = self.event_tx.clone();
            let open_chats = self.open_chats.clone();
            tokio::spawn(dispatcher_loop(db, event_tx, open_chats, outstanding, rx));
        }
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        let mut nanachi = self.nanachi.write().await;
        nanachi.stop().await?;
        Ok(())
    }

    // ---- Account management (delegado a tina-db) ----

    pub async fn create_account(
        &self,
        account_id: &str,
        name: Option<&str>,
    ) -> Result<tina_db::Account> {
        Ok(self.db.create_account(account_id, name).await?)
    }

    pub async fn list_accounts(&self) -> Result<Vec<tina_db::Account>> {
        Ok(self.db.list_accounts().await?)
    }

    pub async fn delete_account(&self, account_id: &str) -> Result<()> {
        Ok(self.db.delete_account(account_id).await?)
    }

    pub async fn start_account(&self, account_id: &str) -> Result<()> {
        let _ = self.db.get_account(account_id).await?;
        let nanachi = self.nanachi.read().await;
        nanachi
            .send_command(IpcCommand::StartAccount {
                account_id: account_id.to_string(),
            })
            .await?;
        Ok(())
    }

    pub async fn stop_account(&self, account_id: &str) -> Result<()> {
        let nanachi = self.nanachi.read().await;
        nanachi
            .send_command(IpcCommand::StopAccount {
                account_id: account_id.to_string(),
            })
            .await?;
        Ok(())
    }

    pub async fn logout_account(&self, account_id: &str) -> Result<()> {
        let nanachi = self.nanachi.read().await;
        nanachi
            .send_command(IpcCommand::Logout {
                account_id: account_id.to_string(),
            })
            .await?;
        Ok(())
    }

    /// Re-pesca tudo que o whatsmeow já tem em cache (contatos, grupos,
    /// newsletters) e emite eventos de upsert. Cura nomes faltando sem
    /// precisar de re-pareamento.
    pub async fn reconcile_account(&self, account_id: &str) -> Result<()> {
        let nanachi = self.nanachi.read().await;
        nanachi
            .send_command(IpcCommand::Reconcile {
                account_id: account_id.to_string(),
            })
            .await?;
        Ok(())
    }

    pub async fn send_message(&self, account_id: &str, to: &str, content: &str) -> Result<()> {
        let nanachi = self.nanachi.read().await;
        nanachi
            .send_command(IpcCommand::SendMessage {
                account_id: account_id.to_string(),
                to: tina_core::WaIdentity::parse(to),
                content: content.to_string(),
            })
            .await?;
        Ok(())
    }

    /// Send a Read receipt for `message_ids` in `chat_jid`. Groups
    /// require `sender_jid` (the participant who sent the message);
    /// DMs can pass the chat jid here. The Go side handles the
    /// actual `whatsmeow.Client.MarkRead` call.
    pub async fn mark_read(
        &self,
        account_id: &str,
        chat_jid: &str,
        sender_jid: &str,
        message_ids: Vec<String>,
    ) -> Result<()> {
        if message_ids.is_empty() {
            return Ok(());
        }
        let nanachi = self.nanachi.read().await;
        nanachi
            .send_command(IpcCommand::MarkRead {
                account_id: account_id.to_string(),
                chat_jid: tina_core::WaIdentity::parse(chat_jid),
                sender_jid: tina_core::WaIdentity::parse(sender_jid),
                message_ids,
            })
            .await?;
        Ok(())
    }

    pub async fn send_media(
        &self,
        account_id: &str,
        to: &str,
        kind: tina_core::MediaKind,
        path: &str,
        caption: Option<&str>,
        mimetype: Option<&str>,
        filename: Option<&str>,
    ) -> Result<()> {
        let nanachi = self.nanachi.read().await;
        nanachi
            .send_command(IpcCommand::SendMedia {
                account_id: account_id.to_string(),
                to: tina_core::WaIdentity::parse(to),
                kind,
                path: path.to_string(),
                caption: caption.map(|s| s.to_string()),
                mimetype: mimetype.map(|s| s.to_string()),
                filename: filename.map(|s| s.to_string()),
            })
            .await?;
        Ok(())
    }

    /// Solicita o profile picture de um JID. Sempre dispara IPC — o
    /// nanachi é quem faz dedup por sha256 do binário antes de baixar
    /// de novo.
    pub async fn fetch_avatar(&self, account_id: &str, jid: &str) -> Result<()> {
        let nanachi = self.nanachi.read().await;
        nanachi
            .send_command(IpcCommand::FetchAvatar {
                account_id: account_id.to_string(),
                jid: tina_core::WaIdentity::parse(jid),
            })
            .await?;
        Ok(())
    }

    /// Pull a chat's metadata from whatsmeow (newsletter or group).
    /// The Go side picks the right API based on the JID's server.
    pub async fn refresh_chat(&self, account_id: &str, chat_jid: &str) -> Result<()> {
        let nanachi = self.nanachi.read().await;
        nanachi
            .send_command(IpcCommand::RefreshChat {
                account_id: account_id.to_string(),
                chat_jid: tina_core::WaIdentity::parse(chat_jid),
            })
            .await?;
        Ok(())
    }

    // ---- Chat-list / messages para a UI ----

    pub async fn list_chat_rows(&self, account_id: &str) -> Result<Vec<ChatRow>> {
        Ok(self.db.list_chat_rows(account_id).await?)
    }

    pub async fn list_status_authors(
        &self,
        account_id: &str,
    ) -> Result<Vec<tina_db::StatusAuthorRow>> {
        Ok(self.db.list_status_authors(account_id).await?)
    }

    /// Reset `chats.unread_count` for a chat (called from open-chat
    /// + mark-read paths). Returns whether the count actually changed
    /// so callers can skip a redundant ChatsUpserted broadcast.
    pub async fn clear_chat_unread(&self, account_id: &str, chat_id: &str) -> Result<bool> {
        Ok(self.db.clear_chat_unread(account_id, chat_id).await? > 0)
    }

    pub async fn list_recent_sticker_paths(
        &self,
        account_id: &str,
        limit: i64,
    ) -> Result<Vec<(String, String)>> {
        Ok(self.db.list_recent_sticker_paths(account_id, limit).await?)
    }

    /// Persist a chat's pinned flag. The UI's `ChatsUpserted` push will
    /// pick up the new value on the next reconcile or chat-list reload.
    pub async fn set_chat_pinned(
        &self,
        account_id: &str,
        chat_id: &str,
        pinned: bool,
    ) -> Result<()> {
        self.db.set_chat_pinned(account_id, chat_id, pinned).await?;
        Ok(())
    }

    pub async fn get_messages(
        &self,
        account_id: &str,
        chat_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<tina_db::Message>> {
        Ok(self
            .db
            .get_messages_by_chat(account_id, chat_id, limit, offset)
            .await?)
    }

    /// Mensagens com `sender_name` já resolvido — pra renderização da
    /// janela.
    pub async fn get_message_rows(
        &self,
        account_id: &str,
        chat_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<tina_db::MessageRow>> {
        Ok(self
            .db
            .get_message_rows_by_chat(account_id, chat_id, limit, offset)
            .await?)
    }

    /// Página anterior: mensagens com timestamp estritamente menor que
    /// `before_ts`, em ordem ASC. Usado pela UI quando o usuário scrolla
    /// pro topo do thread e queremos carregar mais histórico.
    pub async fn get_message_rows_before(
        &self,
        account_id: &str,
        chat_id: &str,
        before_ts: i64,
        limit: i64,
    ) -> Result<Vec<tina_db::MessageRow>> {
        Ok(self
            .db
            .get_message_rows_before(account_id, chat_id, before_ts, limit)
            .await?)
    }

    pub async fn get_chat(
        &self,
        account_id: &str,
        chat_id: &str,
    ) -> Result<Option<tina_db::Chat>> {
        Ok(self.db.get_chat(account_id, chat_id).await?)
    }

    pub async fn get_chat_row(&self, account_id: &str, chat_id: &str) -> Result<Option<ChatRow>> {
        let rows = self
            .db
            .get_chat_rows(account_id, &[chat_id.to_string()])
            .await?;
        Ok(rows.into_iter().next())
    }

    /// UI registra um chat como aberto (tab nova). Enquanto presente,
    /// mensagens novas saem como `MessagesAppended` para renderização
    /// incremental; chats ausentes do set são silenciosamente ignorados
    /// no flush — a UI já tem o snapshot via `ChatsUpserted`.
    pub async fn add_open_chat(&self, account_id: &str, chat_id: &str) {
        let mut guard = self.open_chats.write().await;
        guard
            .entry(account_id.to_string())
            .or_default()
            .insert(chat_id.to_string());
    }

    pub async fn remove_open_chat(&self, account_id: &str, chat_id: &str) {
        let mut guard = self.open_chats.write().await;
        if let Some(set) = guard.get_mut(account_id) {
            set.remove(chat_id);
            if set.is_empty() {
                guard.remove(account_id);
            }
        }
    }

    pub async fn clear_open_chats(&self, account_id: &str) {
        let mut guard = self.open_chats.write().await;
        guard.remove(account_id);
    }

    // ---- Settings (key/value) ----

    pub async fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self.db.get_setting(key).await?)
    }

    pub async fn put_setting(&self, key: &str, value: &str) -> Result<()> {
        Ok(self.db.put_setting(key, value).await?)
    }

    pub async fn clear_all_media_paths(&self) -> Result<u64> {
        Ok(self.db.clear_all_media_paths().await?)
    }

    pub async fn clear_all_avatar_paths(&self) -> Result<u64> {
        Ok(self.db.clear_all_avatar_paths().await?)
    }

    /// Best-effort PID of the running nanachi subprocess, for memory
    /// readouts in the settings dialog. `None` while nanachi hasn't
    /// been started yet.
    pub async fn nanachi_pid(&self) -> Option<u32> {
        self.nanachi.read().await.child_pid()
    }
}
