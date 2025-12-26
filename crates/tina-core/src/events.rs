use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum IpcCommand {
    StartAccount { account_id: String },
    StopAccount { account_id: String },
    GetQrCode { account_id: String },
    SendMessage { account_id: String, to: String, content: String },
    GetContacts { account_id: String },
    GetGroups { account_id: String },
    GetMessages { account_id: String, chat_jid: Option<String>, limit: i64 },
    SetAuthState { account_id: String, auth_state: String },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum IpcEvent {
    Ready { account_id: String },
    QrCode { account_id: String, qr: String },
    Connected { account_id: String, phone_number: Option<String> },
    Disconnected { account_id: String, reason: String },
    LoggedOut { account_id: String },
    
    AuthStateUpdated { account_id: String, auth_state: String },
    
    ContactsUpsert { account_id: String, contacts: Vec<ContactData> },
    ContactsUpdate { account_id: String, contacts: Vec<ContactData> },
    
    GroupsUpsert { account_id: String, groups: Vec<GroupData> },
    GroupsUpdate { account_id: String, groups: Vec<GroupData> },
    
    MessagesUpsert { account_id: String, messages: Vec<MessageData> },
    
    HistorySyncComplete { account_id: String, messages_count: usize },
    
    Error { account_id: Option<String>, error: String },
    
    CommandResult { command_id: String, success: bool, data: Option<serde_json::Value>, error: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactData {
    pub jid: String,
    pub lid: Option<String>,
    pub phone_number: Option<String>,
    pub name: Option<String>,
    pub notify: Option<String>,
    pub verified_name: Option<String>,
    pub img_url: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupData {
    pub jid: String,
    pub subject: Option<String>,
    pub owner: Option<String>,
    pub description: Option<String>,
    pub participants: Vec<ParticipantData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantData {
    pub id: String,
    pub admin: Option<String>,
    pub phone_number: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageData {
    pub message_id: String,
    pub chat_jid: String,
    pub sender_jid: String,
    pub content: Option<String>,
    pub message_type: String,
    pub timestamp: i64,
    pub is_from_me: bool,
    pub raw_json: Option<String>,
}
