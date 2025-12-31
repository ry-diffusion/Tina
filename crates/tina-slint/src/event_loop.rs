use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, error, debug, warn};
use tina_worker::{TinaWorker, WorkerEvent};

use crate::commands::{Command, CommandReceiver};
use crate::state::{SharedAppState, AccountState, ChatState, MessageState};
use crate::ui_bridge::UiBridge;

pub struct EventLoop {
    worker: Arc<TinaWorker>,
    state: SharedAppState,
    command_rx: CommandReceiver,
    worker_event_rx: mpsc::Receiver<WorkerEvent>,
    ui_bridge: UiBridge,
}

impl EventLoop {
    pub async fn new(
        nanachi_dir: PathBuf,
        state: SharedAppState,
        command_rx: CommandReceiver,
        ui_bridge: UiBridge,
    ) -> color_eyre::Result<Self> {
        let mut worker = TinaWorker::new(nanachi_dir).await?;
        let worker_event_rx = worker.take_event_receiver()
            .expect("Worker event receiver already taken");
        
        Ok(Self {
            worker: Arc::new(worker),
            state,
            command_rx,
            worker_event_rx,
            ui_bridge,
        })
    }

    pub async fn run(mut self) -> color_eyre::Result<()> {
        info!("Starting event loop");
        
        self.worker.start().await?;

        self.load_existing_accounts().await?;
        
        loop {
            tokio::select! {
                Some(cmd) = self.command_rx.recv() => {
                    if matches!(cmd, Command::Shutdown) {
                        info!("Shutdown command received");
                        break;
                    }
                    self.handle_command(cmd).await;
                }
                Some(event) = self.worker_event_rx.recv() => {
                    self.handle_worker_event(event).await;
                }
                else => break,
            }
        }
        
        info!("Stopping worker");
        self.worker.stop().await?;
        
        Ok(())
    }

    async fn load_existing_accounts(&mut self) -> color_eyre::Result<()> {
        let accounts = self.worker.list_accounts().await?;
        
        for account in accounts {
            let account_state = AccountState {
                id: account.id.clone(),
                name: account.name.unwrap_or_else(|| account.id.clone()),
                phone_number: account.phone_number,
                is_connected: false,
                is_syncing: false,
            };
            
            {
                let mut state = self.state.write().await;
                state.add_account(account_state);
            }
        }
        
        self.ui_bridge.sync_accounts(&self.state).await;
        
        Ok(())
    }

    async fn handle_command(&mut self, cmd: Command) {
        debug!(?cmd, "Handling command");
        
        match cmd {
            Command::CreateAccount { id, name } => {
                self.handle_create_account(&id, &name).await;
            }
            Command::StartAccount { account_id } => {
                self.handle_start_account(&account_id).await;
            }
            Command::StopAccount { account_id } => {
                self.handle_stop_account(&account_id).await;
            }
            Command::SelectAccount { account_id } => {
                self.handle_select_account(&account_id).await;
            }
            Command::SelectChat { chat_jid } => {
                self.handle_select_chat(&chat_jid).await;
            }
            Command::LoadMessages { account_id, chat_jid } => {
                self.handle_load_messages(&account_id, &chat_jid).await;
            }
            Command::SendMessage { account_id, to, content } => {
                self.handle_send_message(&account_id, &to, &content).await;
            }
            Command::RefreshChats => {
                self.handle_refresh_chats().await;
            }
            Command::Shutdown => {}
        }
    }

    async fn handle_create_account(&mut self, id: &str, name: &str) {
        match self.worker.create_account(id, Some(name)).await {
            Ok(account) => {
                let account_state = AccountState {
                    id: account.id.clone(),
                    name: account.name.unwrap_or_else(|| account.id.clone()),
                    phone_number: account.phone_number,
                    is_connected: false,
                    is_syncing: false,
                };
                
                {
                    let mut state = self.state.write().await;
                    state.add_account(account_state);
                }
                
                self.ui_bridge.sync_accounts(&self.state).await;
                
                if let Err(e) = self.worker.start_account(&account.id).await {
                    error!(?e, "Failed to start account");
                }
            }
            Err(e) => {
                error!(?e, "Failed to create account");
            }
        }
    }

    async fn handle_start_account(&mut self, account_id: &str) {
        if let Err(e) = self.worker.start_account(account_id).await {
            error!(?e, "Failed to start account");
        }
    }

    async fn handle_stop_account(&mut self, account_id: &str) {
        if let Err(e) = self.worker.stop_account(account_id).await {
            error!(?e, "Failed to stop account");
        }
    }

    async fn handle_select_account(&mut self, account_id: &str) {
        {
            let mut state = self.state.write().await;
            state.current_account_id = Some(account_id.to_string());
            state.current_chat_jid = None;
            state.current_chat_name = None;
            state.chats.clear();
            state.messages.clear();
        }
        
        self.ui_bridge.sync_current_account(&self.state).await;
        self.ui_bridge.sync_chats(&self.state).await;
        self.ui_bridge.sync_messages(&self.state).await;
        
        let is_connected = {
            let state = self.state.read().await;
            state.accounts.iter()
                .find(|a| a.id == account_id)
                .map(|a| a.is_connected)
                .unwrap_or(false)
        };
        
        if !is_connected {
            self.handle_start_account(account_id).await;
        }
        
        self.handle_load_chats(account_id).await;
    }

    async fn handle_load_chats(&mut self, account_id: &str) {
        {
            let mut state = self.state.write().await;
            state.is_loading = true;
        }
        self.ui_bridge.sync_loading(&self.state).await;
        
        let chat_jids = match self.worker.get_chats(account_id).await {
            Ok(jids) => jids,
            Err(e) => {
                error!(?e, "Failed to load chats");
                let mut state = self.state.write().await;
                state.is_loading = false;
                self.ui_bridge.sync_loading(&self.state).await;
                return;
            }
        };

        let contacts = self.worker.get_contacts(account_id).await.unwrap_or_default();
        let groups = self.worker.get_groups(account_id).await.unwrap_or_default();

        let chats: Vec<ChatState> = chat_jids
            .iter()
            .map(|jid| {
                let is_group = jid.ends_with("@g.us");
                let name = if is_group {
                    groups.iter()
                        .find(|g| &g.jid == jid)
                        .and_then(|g| g.subject.clone())
                        .unwrap_or_else(|| jid.clone())
                } else {
                    contacts.iter()
                        .find(|c| &c.jid == jid)
                        .and_then(|c| c.name.clone().or(c.notify_name.clone()))
                        .unwrap_or_else(|| jid.clone())
                };
                
                ChatState {
                    jid: jid.clone(),
                    name,
                    last_message: None,
                    last_message_time: None,
                    unread_count: 0,
                    is_group,
                }
            })
            .collect();
        
        {
            let mut state = self.state.write().await;
            state.set_chats(chats);
            state.is_loading = false;
        }
        
        self.ui_bridge.sync_chats(&self.state).await;
        self.ui_bridge.sync_loading(&self.state).await;
    }

    async fn handle_select_chat(&mut self, chat_jid: &str) {
        let account_id = {
            let mut state = self.state.write().await;
            state.select_chat(chat_jid);
            state.current_account_id.clone()
        };
        
        self.ui_bridge.sync_current_chat(&self.state).await;
        
        if let Some(account_id) = account_id {
            self.handle_load_messages(&account_id, chat_jid).await;
        }
    }

    async fn handle_load_messages(&mut self, account_id: &str, chat_jid: &str) {
        match self.worker.get_messages(account_id, Some(chat_jid), 50, 0).await {
            Ok(messages) => {
                let message_states: Vec<MessageState> = messages
                    .into_iter()
                    .map(|m| MessageState {
                        id: m.message_id,
                        sender_name: m.sender_jid.clone(),
                        content: m.content.unwrap_or_default(),
                        timestamp: m.timestamp,
                        is_from_me: m.is_from_me,
                        message_type: m.message_type,
                    })
                    .collect();
                
                {
                    let mut state = self.state.write().await;
                    state.set_messages(message_states);
                }
                
                self.ui_bridge.sync_messages(&self.state).await;
            }
            Err(e) => {
                error!(?e, "Failed to load messages");
            }
        }
    }

    async fn handle_send_message(&mut self, account_id: &str, to: &str, content: &str) {
        if let Err(e) = self.worker.send_message(account_id, to, content).await {
            error!(?e, "Failed to send message");
        }
    }

    async fn handle_refresh_chats(&mut self) {
        let account_id = {
            let state = self.state.read().await;
            state.current_account_id.clone()
        };
        
        if let Some(account_id) = account_id {
            self.handle_load_chats(&account_id).await;
        }
    }

    fn spawn_refresh_chats_silent(&self) {
        let worker = self.worker.clone();
        let state = self.state.clone();
        let ui_bridge = self.ui_bridge.clone();
        
        tokio::spawn(async move {
            let account_id = {
                let state = state.read().await;
                state.current_account_id.clone()
            };
            
            let Some(account_id) = account_id else { return };
            
            let chat_jids = match worker.get_chats(&account_id).await {
                Ok(jids) => jids,
                Err(e) => {
                    error!(?e, "Failed to load chats silently");
                    return;
                }
            };

            let contacts = worker.get_contacts(&account_id).await.unwrap_or_default();
            let groups = worker.get_groups(&account_id).await.unwrap_or_default();

            let chats: Vec<ChatState> = chat_jids
                .iter()
                .map(|jid| {
                    let is_group = jid.ends_with("@g.us");
                    let name = if is_group {
                        groups.iter()
                            .find(|g| &g.jid == jid)
                            .and_then(|g| g.subject.clone())
                            .unwrap_or_else(|| jid.clone())
                    } else {
                        contacts.iter()
                            .find(|c| &c.jid == jid)
                            .and_then(|c| c.name.clone().or(c.notify_name.clone()))
                            .unwrap_or_else(|| jid.clone())
                    };
                    
                    ChatState {
                        jid: jid.clone(),
                        name,
                        last_message: None,
                        last_message_time: None,
                        unread_count: 0,
                        is_group,
                    }
                })
                .collect();
            
            {
                let mut state = state.write().await;
                state.set_chats(chats);
            }
            
            ui_bridge.sync_chats(&state).await;
        });
    }

    fn spawn_refresh_messages_silent(&self) {
        let worker = self.worker.clone();
        let state = self.state.clone();
        let ui_bridge = self.ui_bridge.clone();
        
        tokio::spawn(async move {
            let (account_id, chat_jid) = {
                let state = state.read().await;
                (state.current_account_id.clone(), state.current_chat_jid.clone())
            };
            
            let Some(account_id) = account_id else { return };
            let Some(chat_jid) = chat_jid else { return };
            
            if let Ok(messages) = worker.get_messages(&account_id, Some(&chat_jid), 50, 0).await {
                let message_states: Vec<MessageState> = messages
                    .into_iter()
                    .map(|m| MessageState {
                        id: m.message_id,
                        sender_name: m.sender_jid.clone(),
                        content: m.content.unwrap_or_default(),
                        timestamp: m.timestamp,
                        is_from_me: m.is_from_me,
                        message_type: m.message_type,
                    })
                    .collect();
                
                {
                    let mut state = state.write().await;
                    state.set_messages(message_states);
                }
                
                ui_bridge.sync_messages(&state).await;
            }
        });
    }

    async fn handle_worker_event(&mut self, event: WorkerEvent) {
        debug!(?event, "Handling worker event");
        
        match event {
            WorkerEvent::NanachiReady => {
                info!("Nanachi is ready");
            }
            WorkerEvent::AccountReady { account_id } => {
                info!(%account_id, "Account ready");
            }
            WorkerEvent::QrCode { account_id, qr } => {
                info!(%account_id, "QR code received");
                {
                    let mut state = self.state.write().await;
                    state.qr_code_data = Some(qr);
                    state.show_qr_dialog = true;
                }
                self.ui_bridge.sync_qr_dialog(&self.state).await;
            }
            WorkerEvent::Connected { account_id, phone_number } => {
                info!(%account_id, ?phone_number, "Account connected");
                {
                    let mut state = self.state.write().await;
                    state.set_account_connected(&account_id, phone_number);
                    state.show_qr_dialog = false;
                }
                self.ui_bridge.sync_accounts(&self.state).await;
                self.ui_bridge.sync_qr_dialog(&self.state).await;
            }
            WorkerEvent::Disconnected { account_id, reason } => {
                warn!(%account_id, %reason, "Account disconnected");
                {
                    let mut state = self.state.write().await;
                    state.set_account_disconnected(&account_id);
                }
                self.ui_bridge.sync_accounts(&self.state).await;
            }
            WorkerEvent::LoggedOut { account_id } => {
                info!(%account_id, "Account logged out");
                {
                    let mut state = self.state.write().await;
                    state.set_account_disconnected(&account_id);
                }
                self.ui_bridge.sync_accounts(&self.state).await;
            }
            WorkerEvent::ContactsSynced { account_id, count } => {
                info!(%account_id, count, "Contacts synced");
                {
                    let mut state = self.state.write().await;
                    state.sync_status = format!("Contacts synced: {}", count);
                }
                self.ui_bridge.sync_status(&self.state).await;
            }
            WorkerEvent::GroupsSynced { account_id, count } => {
                info!(%account_id, count, "Groups synced");
                {
                    let mut state = self.state.write().await;
                    state.sync_status = format!("Groups synced: {}", count);
                }
                self.ui_bridge.sync_status(&self.state).await;
            }
            WorkerEvent::MessagesSynced { account_id, count } => {
                info!(%account_id, count, "Messages synced");
                {
                    let mut state = self.state.write().await;
                    state.sync_status = format!("Messages synced: {}", count);
                }
                self.ui_bridge.sync_status(&self.state).await;
                self.spawn_refresh_chats_silent();
                self.spawn_refresh_messages_silent();
            }
            WorkerEvent::HistorySyncComplete { account_id, messages_count } => {
                info!(%account_id, messages_count, "History sync complete");
                {
                    let mut state = self.state.write().await;
                    state.sync_status = format!("History synced: {} messages", messages_count);
                }
                self.ui_bridge.sync_status(&self.state).await;
                self.spawn_refresh_chats_silent();
                self.spawn_refresh_messages_silent();
            }
            WorkerEvent::Error { account_id, error } => {
                error!(?account_id, %error, "Worker error");
                {
                    let mut state = self.state.write().await;
                    state.status_message = format!("Error: {}", error);
                }
                self.ui_bridge.sync_status(&self.state).await;
            }
        }
    }
}
