use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct AccountState {
    pub id: String,
    pub name: String,
    pub phone_number: Option<String>,
    pub is_connected: bool,
    pub is_syncing: bool,
}

#[derive(Debug, Clone)]
pub struct ChatState {
    pub jid: String,
    pub name: String,
    pub last_message: Option<String>,
    pub last_message_time: Option<i64>,
    pub unread_count: i32,
    pub is_group: bool,
}

#[derive(Debug, Clone)]
pub struct MessageState {
    pub id: String,
    pub sender_name: String,
    pub content: String,
    pub timestamp: i64,
    pub is_from_me: bool,
    pub message_type: String,
}

#[derive(Debug, Default)]
pub struct AppStateInner {
    pub current_account_id: Option<String>,
    pub current_chat_jid: Option<String>,
    pub current_chat_name: Option<String>,
    pub accounts: Vec<AccountState>,
    pub chats: Vec<ChatState>,
    pub messages: Vec<MessageState>,
    pub is_loading: bool,
    pub status_message: String,
    pub qr_code_data: Option<String>,
    pub show_qr_dialog: bool,
    pub sync_status: String,
}

impl AppStateInner {
    pub fn new() -> Self {
        Self {
            status_message: "Welcome to Tina".to_string(),
            ..Default::default()
        }
    }

    pub fn set_account_connected(&mut self, account_id: &str, phone_number: Option<String>) {
        if let Some(account) = self.accounts.iter_mut().find(|a| a.id == account_id) {
            account.is_connected = true;
            account.phone_number = phone_number;
        }
    }

    pub fn set_account_disconnected(&mut self, account_id: &str) {
        if let Some(account) = self.accounts.iter_mut().find(|a| a.id == account_id) {
            account.is_connected = false;
            account.is_syncing = false;
        }
    }

    pub fn add_account(&mut self, account: AccountState) {
        if !self.accounts.iter().any(|a| a.id == account.id) {
            self.accounts.push(account);
        }
    }

    pub fn set_chats(&mut self, chats: Vec<ChatState>) {
        self.chats = chats;
    }

    pub fn set_messages(&mut self, messages: Vec<MessageState>) {
        self.messages = messages;
    }

    pub fn select_chat(&mut self, jid: &str) {
        self.current_chat_jid = Some(jid.to_string());
        if let Some(chat) = self.chats.iter().find(|c| c.jid == jid) {
            self.current_chat_name = Some(chat.name.clone());
        }
    }
}

pub type SharedAppState = Arc<RwLock<AppStateInner>>;

pub fn create_app_state() -> SharedAppState {
    Arc::new(RwLock::new(AppStateInner::new()))
}
