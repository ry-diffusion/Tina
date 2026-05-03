use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum IpcCommand {
    StartAccount { account_id: String },
    StopAccount { account_id: String },
    Logout { account_id: String },
    SendMessage { account_id: String, to: String, content: String },
    /// Re-pesca contatos/grupos/newsletters do whatsmeow e re-emite eventos
    /// de upsert. Usado pra reconstruir a tabela do tina a partir do que o
    /// whatsmeow.db já sabe — sem precisar de re-pareamento.
    Reconcile { account_id: String },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum IpcEvent {
    Ready { account_id: String },
    QrCode { account_id: String, qr: String },
    PairingCode { account_id: String, code: String },
    Connected { account_id: String, phone_number: Option<String>, jid: Option<String> },
    Disconnected { account_id: String, reason: String },
    LoggedOut { account_id: String },

    ContactsUpsert { account_id: String, contacts: Vec<ContactData> },
    GroupsUpsert { account_id: String, groups: Vec<GroupData> },
    MessagesUpsert { account_id: String, messages: Vec<MessageData> },

    HistorySyncComplete { account_id: String, messages_count: usize },

    /// Progresso da reconciliação. `total = 0` significa indeterminado
    /// (mostra spinner, sem barra). Stage é texto pronto pra UI.
    ReconcileProgress {
        account_id: String,
        stage: String,
        current: i64,
        total: i64,
        indeterminate: bool,
    },

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
    /// Metadados de mídia extraídos do proto. Vêm preenchidos pra
    /// image/audio/video/sticker/document; ausentes pra texto.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_mimetype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_duration_secs: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_width: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_height: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_size_bytes: Option<i64>,
    /// SHA256 hex (64 chars) do conteúdo claro. Usado pra deduplicar
    /// downloads (mesmo arquivo enviado em vários chats vira 1 file).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_sha256: Option<String>,
}
