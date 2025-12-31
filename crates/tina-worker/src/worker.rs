use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

use tina_core::{IpcCommand, IpcEvent};
use tina_db::TinaDb;
use tina_ipc::NanachiManager;

use crate::error::Result;
use crate::events::WorkerEvent;

pub struct TinaWorker {
    db: Arc<TinaDb>,
    nanachi: Arc<RwLock<NanachiManager>>,
    event_tx: mpsc::Sender<WorkerEvent>,
    event_rx: Option<mpsc::Receiver<WorkerEvent>>,
}

impl TinaWorker {
    pub async fn new(nanachi_dir: PathBuf) -> Result<Self> {
        let db = TinaDb::new().await?;
        let nanachi = NanachiManager::new(nanachi_dir);
        let (event_tx, event_rx) = mpsc::channel(1000);

        Ok(Self {
            db: Arc::new(db),
            nanachi: Arc::new(RwLock::new(nanachi)),
            event_tx,
            event_rx: Some(event_rx),
        })
    }

    pub fn take_event_receiver(&mut self) -> Option<mpsc::Receiver<WorkerEvent>> {
        self.event_rx.take()
    }

    pub async fn start(&self) -> Result<()> {
        let mut nanachi = self.nanachi.write().await;
        nanachi.start().await?;

        let ipc_rx = nanachi.take_event_receiver();

        if let Some(mut rx) = ipc_rx {
            let db = self.db.clone();
            let event_tx = self.event_tx.clone();

            tokio::spawn(async move {
                while let Some(line) = rx.recv().await {
                    if let Some(event) = NanachiManager::parse_event(&line) {
                        if let Err(e) = handle_ipc_event(&db, &event_tx, event).await {
                            tracing::error!("Error handling IPC event: {}", e);
                        }
                    }
                }
            });
        }

        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        let mut nanachi = self.nanachi.write().await;
        nanachi.stop().await?;
        Ok(())
    }

    pub async fn create_account(&self, account_id: &str, name: Option<&str>) -> Result<tina_db::Account> {
        Ok(self.db.create_account(account_id, name).await?)
    }

    pub async fn list_accounts(&self) -> Result<Vec<tina_db::Account>> {
        Ok(self.db.list_accounts().await?)
    }

    pub async fn delete_account(&self, account_id: &str) -> Result<()> {
        Ok(self.db.delete_account(account_id).await?)
    }

    pub async fn start_account(&self, account_id: &str) -> Result<()> {
        let account = self.db.get_account(account_id).await?;

        let nanachi = self.nanachi.read().await;

        if let Some(ref auth_state) = account.auth_state {
            nanachi
                .send_command(IpcCommand::SetAuthState {
                    account_id: account_id.to_string(),
                    auth_state: auth_state.clone(),
                })
                .await?;
        }

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

    pub async fn get_contacts(&self, account_id: &str) -> Result<Vec<tina_db::Contact>> {
        Ok(self.db.get_contacts(account_id).await?)
    }

    pub async fn get_groups(&self, account_id: &str) -> Result<Vec<tina_db::Group>> {
        Ok(self.db.get_groups(account_id).await?)
    }

    pub async fn get_messages(
        &self,
        account_id: &str,
        chat_jid: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<tina_db::Message>> {
        Ok(self.db.get_messages(account_id, chat_jid, limit, offset).await?)
    }

    pub async fn get_chats(&self, account_id: &str) -> Result<Vec<String>> {
        Ok(self.db.get_chats(account_id).await?)
    }

    pub async fn send_message(&self, account_id: &str, to: &str, content: &str) -> Result<()> {
        let nanachi = self.nanachi.read().await;
        nanachi
            .send_command(IpcCommand::SendMessage {
                account_id: account_id.to_string(),
                to: to.to_string(),
                content: content.to_string(),
            })
            .await?;
        Ok(())
    }
}

async fn handle_ipc_event(
    db: &TinaDb,
    event_tx: &mpsc::Sender<WorkerEvent>,
    event: IpcEvent,
) -> Result<()> {
    match event {
        IpcEvent::Ready { account_id } => {
            if account_id.is_empty() {
                let _ = event_tx.send(WorkerEvent::NanachiReady).await;
            } else {
                let _ = event_tx.send(WorkerEvent::AccountReady { account_id }).await;
            }
        }

        IpcEvent::QrCode { account_id, qr } => {
            let _ = event_tx.send(WorkerEvent::QrCode { account_id, qr }).await;
        }

        IpcEvent::Connected { account_id, phone_number } => {
            let _ = event_tx
                .send(WorkerEvent::Connected {
                    account_id,
                    phone_number,
                })
                .await;
        }

        IpcEvent::Disconnected { account_id, reason } => {
            let _ = event_tx
                .send(WorkerEvent::Disconnected { account_id, reason })
                .await;
        }

        IpcEvent::LoggedOut { account_id } => {
            let _ = event_tx.send(WorkerEvent::LoggedOut { account_id }).await;
        }

        IpcEvent::AuthStateUpdated { account_id, auth_state } => {
            db.save_auth_state(&account_id, &auth_state).await?;
        }

        IpcEvent::ContactsUpsert { account_id, contacts } => {
            let count = contacts.len();
            for contact in contacts {
                db.upsert_contact(
                    &account_id,
                    &contact.jid,
                    contact.lid.as_deref(),
                    contact.phone_number.as_deref(),
                    contact.name.as_deref(),
                    contact.notify.as_deref(),
                    contact.verified_name.as_deref(),
                    contact.img_url.as_deref(),
                    contact.status.as_deref(),
                    false,
                )
                .await?;
            }
            let _ = event_tx
                .send(WorkerEvent::ContactsSynced { account_id, count })
                .await;
        }

        IpcEvent::ContactsUpdate { account_id, contacts } => {
            for contact in &contacts {
                db.upsert_contact(
                    &account_id,
                    &contact.jid,
                    contact.lid.as_deref(),
                    contact.phone_number.as_deref(),
                    contact.name.as_deref(),
                    contact.notify.as_deref(),
                    contact.verified_name.as_deref(),
                    contact.img_url.as_deref(),
                    contact.status.as_deref(),
                    false,
                )
                .await?;
            }
        }

        IpcEvent::GroupsUpsert { account_id, groups } => {
            let count = groups.len();
            for group in groups {
                let participants_json = serde_json::to_string(&group.participants).ok();
                db.upsert_group(
                    &account_id,
                    &group.jid,
                    group.subject.as_deref(),
                    group.owner.as_deref(),
                    group.description.as_deref(),
                    participants_json.as_deref(),
                )
                .await?;
            }
            let _ = event_tx
                .send(WorkerEvent::GroupsSynced { account_id, count })
                .await;
        }

        IpcEvent::GroupsUpdate { account_id, groups } => {
            for group in groups {
                let participants_json = if group.participants.is_empty() {
                    None
                } else {
                    serde_json::to_string(&group.participants).ok()
                };
                db.upsert_group(
                    &account_id,
                    &group.jid,
                    group.subject.as_deref(),
                    group.owner.as_deref(),
                    group.description.as_deref(),
                    participants_json.as_deref(),
                )
                .await?;
            }
        }

        IpcEvent::MessagesUpsert { account_id, messages } => {
            let count = messages.len();
            for msg in messages {
                db.insert_message(
                    &account_id,
                    &msg.message_id,
                    &msg.chat_jid,
                    &msg.sender_jid,
                    msg.content.as_deref(),
                    &msg.message_type,
                    msg.timestamp,
                    msg.is_from_me,
                    msg.raw_json.as_deref(),
                )
                .await?;
            }
            let _ = event_tx
                .send(WorkerEvent::MessagesSynced { account_id, count })
                .await;
        }

        IpcEvent::HistorySyncComplete { account_id, messages_count } => {
            let _ = event_tx
                .send(WorkerEvent::HistorySyncComplete {
                    account_id,
                    messages_count,
                })
                .await;
        }

        IpcEvent::Error { account_id, error } => {
            let _ = event_tx.send(WorkerEvent::Error { account_id, error }).await;
        }

        IpcEvent::CommandResult { .. } => {}
    }

    Ok(())
}
