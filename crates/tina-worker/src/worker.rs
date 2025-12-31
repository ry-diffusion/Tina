use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{info, info_span, debug, warn, error, Instrument};

use tina_core::{ChatMessage, ChatPreviewInfo, IpcCommand, IpcEvent};
use tina_db::TinaDb;
use tina_ipc::NanachiManager;

use crate::contacts::ContactResolver;
use crate::error::Result;
use crate::events::{WorkerEvent, SyncType};
use crate::message_parser::parse_db_message;

pub struct TinaWorker {
    db: Arc<TinaDb>,
    nanachi: Arc<RwLock<NanachiManager>>,
    event_tx: mpsc::Sender<WorkerEvent>,
    event_rx: Option<mpsc::Receiver<WorkerEvent>>,
    contact_resolver: Arc<RwLock<ContactResolver>>,
}

impl TinaWorker {
    pub async fn new(nanachi_dir: PathBuf) -> Result<Self> {
        info!("Initializing TinaWorker");
        
        let db = TinaDb::new().await?;
        let nanachi = NanachiManager::new(nanachi_dir);
        let (event_tx, event_rx) = mpsc::channel(1000);

        info!("TinaWorker initialized successfully");
        
        Ok(Self {
            db: Arc::new(db),
            nanachi: Arc::new(RwLock::new(nanachi)),
            event_tx,
            event_rx: Some(event_rx),
            contact_resolver: Arc::new(RwLock::new(ContactResolver::new())),
        })
    }

    pub fn take_event_receiver(&mut self) -> Option<mpsc::Receiver<WorkerEvent>> {
        self.event_rx.take()
    }

    pub async fn start(&self) -> Result<()> {
        info!("Starting TinaWorker");
        
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
                            error!("Error handling IPC event: {}", e);
                        }
                    }
                }
            }.instrument(info_span!("ipc_event_loop")));
        }

        info!("TinaWorker started successfully");
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        info!("Stopping TinaWorker");
        
        let mut nanachi = self.nanachi.write().await;
        nanachi.stop().await?;
        
        info!("TinaWorker stopped");
        Ok(())
    }

    pub async fn create_account(&self, account_id: &str, name: Option<&str>) -> Result<tina_db::Account> {
        info!(account_id = %account_id, name = ?name, "Creating account");
        
        let account = self.db.create_account(account_id, name).await?;
        info!(account_id = %account_id, "Account created successfully");
        Ok(account)
    }

    pub async fn list_accounts(&self) -> Result<Vec<tina_db::Account>> {
        let accounts = self.db.list_accounts().await?;
        debug!(count = accounts.len(), "Listed accounts");
        Ok(accounts)
    }

    pub async fn delete_account(&self, account_id: &str) -> Result<()> {
        info!(account_id = %account_id, "Deleting account");
        
        self.db.delete_account(account_id).await?;
        info!(account_id = %account_id, "Account deleted");
        Ok(())
    }

    pub async fn start_account(&self, account_id: &str) -> Result<()> {
        info!(account_id = %account_id, "Starting account");
        
        let account = self.db.get_account(account_id).await?;

        let nanachi = self.nanachi.read().await;

        if let Some(ref auth_state) = account.auth_state {
            debug!(account_id = %account_id, "Setting auth state for account");
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

        info!(account_id = %account_id, "Account start command sent");
        Ok(())
    }

    pub async fn stop_account(&self, account_id: &str) -> Result<()> {
        info!(account_id = %account_id, "Stopping account");
        
        let nanachi = self.nanachi.read().await;
        nanachi
            .send_command(IpcCommand::StopAccount {
                account_id: account_id.to_string(),
            })
            .await?;
        
        info!(account_id = %account_id, "Account stop command sent");
        Ok(())
    }

    pub async fn get_contacts(&self, account_id: &str) -> Result<Vec<tina_db::Contact>> {
        let contacts = self.db.get_contacts(account_id).await?;
        debug!(account_id = %account_id, count = contacts.len(), "Retrieved contacts from DB");
        Ok(contacts)
    }

    pub async fn get_groups(&self, account_id: &str) -> Result<Vec<tina_db::Group>> {
        let groups = self.db.get_groups(account_id).await?;
        debug!(account_id = %account_id, count = groups.len(), "Retrieved groups from DB");
        Ok(groups)
    }

    pub async fn get_messages(
        &self,
        account_id: &str,
        chat_jid: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<tina_db::Message>> {
        let messages = self.db.get_messages(account_id, chat_jid, limit, offset).await?;
        debug!(account_id = %account_id, chat_jid = ?chat_jid, count = messages.len(), limit, offset, "Retrieved messages from DB");
        Ok(messages)
    }

    pub async fn get_chats(&self, account_id: &str) -> Result<Vec<String>> {
        let chats = self.db.get_chats(account_id).await?;
        debug!(account_id = %account_id, count = chats.len(), "Retrieved chats from DB");
        Ok(chats)
    }

    pub async fn get_chats_basic(&self, account_id: &str) -> Result<Vec<ChatPreviewInfo>> {
        debug!(account_id = %account_id, "Loading chats (basic, no previews)");
        
        let rows = self.db.get_chats_with_names(account_id).await?;
        
        let chats: Vec<ChatPreviewInfo> = rows
            .into_iter()
            .map(|row| {
                let is_group = row.is_group == 1;
                let name = row.resolved_name.unwrap_or_else(|| row.chat_jid.clone());

                ChatPreviewInfo {
                    jid: row.chat_jid,
                    name,
                    is_group,
                    last_message: None,
                    last_message_timestamp: Some(row.last_timestamp),
                    unread_count: 0,
                }
            })
            .collect();

        debug!(account_id = %account_id, count = chats.len(), "Chats loaded (basic)");
        Ok(chats)
    }

    pub async fn get_chat_previews(&self, account_id: &str) -> Result<Vec<ChatPreviewInfo>> {
        debug!(account_id = %account_id, "Loading chat previews (batch)");
        
        let rows = self.db.get_chat_previews_batch(account_id).await?;
        
        let previews: Vec<ChatPreviewInfo> = rows
            .into_iter()
            .map(|row| {
                let is_group = row.is_group == 1;
                let name = row.resolved_name.unwrap_or_else(|| row.chat_jid.clone());
                
                let last_message = row.last_message_content.map(|content| {
                    format_message_preview(&row.last_message_type, &content)
                });

                ChatPreviewInfo {
                    jid: row.chat_jid,
                    name,
                    is_group,
                    last_message,
                    last_message_timestamp: Some(row.last_message_timestamp),
                    unread_count: 0,
                }
            })
            .collect();

        debug!(account_id = %account_id, count = previews.len(), "Chat previews loaded (batch)");
        Ok(previews)
    }

    pub async fn get_chat_messages(
        &self,
        account_id: &str,
        chat_jid: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ChatMessage>> {
        debug!(account_id = %account_id, chat_jid = %chat_jid, limit, offset, "Loading chat messages");
        
        let db_messages = self.db.get_messages(account_id, Some(chat_jid), limit, offset).await?;
        
        let mut resolver = self.contact_resolver.write().await;
        resolver.load_from_db(&self.db, account_id).await?;

        let messages: Vec<ChatMessage> = db_messages
            .iter()
            .map(|msg| parse_db_message(msg, &mut *resolver, None))
            .collect();

        debug!(account_id = %account_id, chat_jid = %chat_jid, count = messages.len(), "Chat messages loaded and parsed");
        Ok(messages)
    }

    pub async fn send_message(&self, account_id: &str, to: &str, content: &str) -> Result<()> {
        info!(account_id = %account_id, to = %to, content_len = content.len(), "Sending message");
        
        let nanachi = self.nanachi.read().await;
        nanachi
            .send_command(IpcCommand::SendMessage {
                account_id: account_id.to_string(),
                to: to.to_string(),
                content: content.to_string(),
            })
            .await?;
        
        info!(account_id = %account_id, to = %to, "Message sent");
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
                info!("Nanachi ready (global)");
                let _ = event_tx.send(WorkerEvent::NanachiReady).await;
            } else {
                info!(account_id = %account_id, "Account ready");
                let _ = event_tx.send(WorkerEvent::AccountReady { account_id }).await;
            }
        }

        IpcEvent::QrCode { account_id, qr } => {
            info!(account_id = %account_id, qr_len = qr.len(), "QR code received");
            let _ = event_tx.send(WorkerEvent::QrCode { account_id, qr }).await;
        }

        IpcEvent::Connected { account_id, phone_number } => {
            info!(account_id = %account_id, phone_number = ?phone_number, "Account connected");
            let _ = event_tx
                .send(WorkerEvent::Connected {
                    account_id,
                    phone_number,
                })
                .await;
        }

        IpcEvent::Disconnected { account_id, reason } => {
            info!(account_id = %account_id, reason = %reason, "Account disconnected");
            let _ = event_tx
                .send(WorkerEvent::Disconnected { account_id, reason })
                .await;
        }

        IpcEvent::LoggedOut { account_id } => {
            info!(account_id = %account_id, "Account logged out");
            let _ = event_tx.send(WorkerEvent::LoggedOut { account_id }).await;
        }

        IpcEvent::AuthStateUpdated { account_id, auth_state } => {
            debug!(account_id = %account_id, auth_state_len = auth_state.len(), "Saving auth state");
            db.save_auth_state(&account_id, &auth_state).await?;
        }

        IpcEvent::ContactsUpsert { account_id, contacts } => {
            let count = contacts.len();
            info!(account_id = %account_id, count, "Upserting contacts");
            
            let _ = event_tx
                .send(WorkerEvent::SyncStarted { 
                    account_id: account_id.clone(), 
                    sync_type: SyncType::Contacts 
                })
                .await;
            
            for (i, contact) in contacts.iter().enumerate() {
                debug!(
                    account_id = %account_id,
                    jid = %contact.jid,
                    lid = ?contact.lid,
                    name = ?contact.name,
                    "Upserting contact"
                );
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
                
                if count > 10 && i % 50 == 0 {
                    let _ = event_tx
                        .send(WorkerEvent::SyncProgress { 
                            account_id: account_id.clone(), 
                            sync_type: SyncType::Contacts,
                            current: i + 1,
                            total: Some(count),
                        })
                        .await;
                }
            }
            
            info!(account_id = %account_id, count, "Contacts upserted successfully");
            let _ = event_tx
                .send(WorkerEvent::SyncCompleted { 
                    account_id: account_id.clone(), 
                    sync_type: SyncType::Contacts,
                    count,
                })
                .await;
            let _ = event_tx
                .send(WorkerEvent::ContactsSynced { account_id, count })
                .await;
        }

        IpcEvent::ContactsUpdate { account_id, contacts } => {
            let count = contacts.len();
            debug!(account_id = %account_id, count, "Updating contacts");
            
            for contact in &contacts {
                debug!(account_id = %account_id, jid = %contact.jid, "Updating contact");
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
            info!(account_id = %account_id, count, "Upserting groups");
            
            let _ = event_tx
                .send(WorkerEvent::SyncStarted { 
                    account_id: account_id.clone(), 
                    sync_type: SyncType::Groups 
                })
                .await;
            
            for group in groups {
                debug!(
                    account_id = %account_id,
                    jid = %group.jid,
                    subject = ?group.subject,
                    participants = group.participants.len(),
                    "Upserting group"
                );
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
            
            info!(account_id = %account_id, count, "Groups upserted successfully");
            let _ = event_tx
                .send(WorkerEvent::SyncCompleted { 
                    account_id: account_id.clone(), 
                    sync_type: SyncType::Groups,
                    count,
                })
                .await;
            let _ = event_tx
                .send(WorkerEvent::GroupsSynced { account_id, count })
                .await;
        }

        IpcEvent::GroupsUpdate { account_id, groups } => {
            let count = groups.len();
            debug!(account_id = %account_id, count, "Updating groups");
            
            for group in groups {
                debug!(account_id = %account_id, jid = %group.jid, "Updating group");
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
            info!(account_id = %account_id, count, "Upserting messages");
            
            let _ = event_tx
                .send(WorkerEvent::SyncStarted { 
                    account_id: account_id.clone(), 
                    sync_type: SyncType::Messages 
                })
                .await;
            
            for (i, msg) in messages.iter().enumerate() {
                debug!(
                    account_id = %account_id,
                    message_id = %msg.message_id,
                    chat_jid = %msg.chat_jid,
                    sender_jid = %msg.sender_jid,
                    message_type = %msg.message_type,
                    is_from_me = msg.is_from_me,
                    "Inserting message"
                );
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
                
                if count > 50 && i % 100 == 0 {
                    let _ = event_tx
                        .send(WorkerEvent::SyncProgress { 
                            account_id: account_id.clone(), 
                            sync_type: SyncType::Messages,
                            current: i + 1,
                            total: Some(count),
                        })
                        .await;
                }
            }
            
            info!(account_id = %account_id, count, "Messages upserted successfully");
            let _ = event_tx
                .send(WorkerEvent::SyncCompleted { 
                    account_id: account_id.clone(), 
                    sync_type: SyncType::Messages,
                    count,
                })
                .await;
            let _ = event_tx
                .send(WorkerEvent::MessagesSynced { account_id, count })
                .await;
        }

        IpcEvent::HistorySyncComplete { account_id, messages_count } => {
            info!(account_id = %account_id, messages_count, "History sync completed");
            let _ = event_tx
                .send(WorkerEvent::SyncCompleted { 
                    account_id: account_id.clone(), 
                    sync_type: SyncType::History,
                    count: messages_count,
                })
                .await;
            let _ = event_tx
                .send(WorkerEvent::HistorySyncComplete {
                    account_id,
                    messages_count,
                })
                .await;
        }

        IpcEvent::Error { account_id, error } => {
            warn!(account_id = ?account_id, error = %error, "IPC error received");
            let _ = event_tx.send(WorkerEvent::Error { account_id, error }).await;
        }

        IpcEvent::CommandResult { .. } => {}
    }

    Ok(())
}

fn format_message_preview(message_type: &str, content: &str) -> String {
    match message_type {
        "text" | "extendedText" => content.to_string(),
        "image" | "imageMessage" => "📷 Image".to_string(),
        "video" | "videoMessage" => "🎥 Video".to_string(),
        "audio" | "audioMessage" => "🎵 Audio".to_string(),
        "document" | "documentMessage" => "📄 Document".to_string(),
        "sticker" | "stickerMessage" => "🏷️ Sticker".to_string(),
        "location" | "locationMessage" => "📍 Location".to_string(),
        "contact" | "contactMessage" => "👤 Contact".to_string(),
        "poll" | "pollCreation" => "📊 Poll".to_string(),
        _ => content.to_string(),
    }
}
