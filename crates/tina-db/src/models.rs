use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Account {
    pub id: String,
    pub name: Option<String>,
    pub phone_number: Option<String>,
    pub auth_state: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Contact {
    pub id: i64,
    pub account_id: String,
    pub jid: String,
    pub lid: Option<String>,
    pub phone_number: Option<String>,
    pub name: Option<String>,
    pub notify_name: Option<String>,
    pub verified_name: Option<String>,
    pub img_url: Option<String>,
    pub status: Option<String>,
    pub is_local: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Group {
    pub id: i64,
    pub account_id: String,
    pub jid: String,
    pub subject: Option<String>,
    pub owner: Option<String>,
    pub description: Option<String>,
    pub participants_json: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Message {
    pub id: i64,
    pub account_id: String,
    pub message_id: String,
    pub chat_jid: String,
    pub sender_jid: String,
    pub content: Option<String>,
    pub message_type: String,
    pub timestamp: i64,
    pub is_from_me: bool,
    pub raw_json: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupParticipant {
    pub id: String,
    pub admin: Option<String>,
    pub phone_number: Option<String>,
}
