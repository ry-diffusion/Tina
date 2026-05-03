use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Account {
    pub id: String,
    pub name: Option<String>,
    pub phone_number: Option<String>,
    pub jid: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Tipo do chat. Inferido a partir do server do JID na criação.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatKind {
    Dm,
    Group,
    Newsletter,
    Broadcast,
    Status,
    Unknown,
}

impl ChatKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ChatKind::Dm => "dm",
            ChatKind::Group => "group",
            ChatKind::Newsletter => "newsletter",
            ChatKind::Broadcast => "broadcast",
            ChatKind::Status => "status",
            ChatKind::Unknown => "unknown",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "dm" => ChatKind::Dm,
            "group" => ChatKind::Group,
            "newsletter" => ChatKind::Newsletter,
            "broadcast" => ChatKind::Broadcast,
            "status" => ChatKind::Status,
            _ => ChatKind::Unknown,
        }
    }

    /// Inferência a partir da parte `@server` de um JID.
    pub fn infer_from_jid(jid: &str) -> Self {
        let server = jid.rsplit_once('@').map(|(_, s)| s).unwrap_or("");
        match server {
            "s.whatsapp.net" | "lid" | "c.us" | "hosted" => ChatKind::Dm,
            "g.us" => ChatKind::Group,
            "newsletter" => ChatKind::Newsletter,
            "broadcast" => {
                // status@broadcast tem JID literalmente "status@broadcast"
                if jid == "status@broadcast" {
                    ChatKind::Status
                } else {
                    ChatKind::Broadcast
                }
            }
            _ => ChatKind::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Contact {
    pub account_id: String,
    pub contact_id: String,
    pub pn_jid: Option<String>,
    pub lid_jid: Option<String>,
    pub phone_number: Option<String>,
    pub push_name: Option<String>,
    pub contact_name: Option<String>,
    pub business_name: Option<String>,
    pub verified_name: Option<String>,
    pub avatar_url: Option<String>,
    pub avatar_path: Option<String>,
    pub status: Option<String>,
    pub is_local: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Chat {
    pub account_id: String,
    pub chat_id: String,
    pub kind: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub avatar_path: Option<String>,
    pub last_message_id: Option<String>,
    pub last_message_preview: Option<String>,
    pub last_message_ts: Option<i64>,
    pub last_message_from_me: bool,
    pub last_sender_contact_id: Option<String>,
    pub last_message_type: Option<String>,
    pub last_message_duration_secs: Option<i64>,
    pub unread_count: i64,
    pub pinned: bool,
    pub archived: bool,
    pub muted_until: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Linha pronta para a UI: o `name` já vem resolvido via JOIN com `contacts`
/// quando o chat é DM. Para grupos/newsletters cai no `display_name` próprio.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ChatRow {
    pub chat_id: String,
    pub kind: String,
    pub name: String,
    pub avatar_url: Option<String>,
    pub avatar_path: Option<String>,
    pub last_message_preview: Option<String>,
    pub last_message_ts: Option<i64>,
    pub last_message_from_me: bool,
    pub last_message_type: Option<String>,
    pub last_message_duration_secs: Option<i64>,
    pub unread_count: i64,
    pub pinned: bool,
}

/// Input para `TinaDb::run_message_batch` — empréstimo direto dos campos
/// necessários, sem alocação extra.
#[derive(Debug, Clone, Copy)]
pub struct MessageBatchInput<'a> {
    pub message_id: &'a str,
    pub chat_jid: &'a str,
    pub sender_jid: Option<&'a str>,
    pub content: Option<&'a str>,
    pub message_type: &'a str,
    pub timestamp: i64,
    pub is_from_me: bool,
    pub raw_json: Option<&'a str>,
    /// Metadados de mídia (NULL para texto). Vêm do nanachi extraindo do
    /// proto sem baixar — o download real é separado e atualiza a linha.
    pub media_mimetype: Option<&'a str>,
    pub media_filename: Option<&'a str>,
    pub media_duration_secs: Option<i64>,
    pub media_width: Option<i64>,
    pub media_height: Option<i64>,
    pub media_size_bytes: Option<i64>,
    /// Hash sha256 do conteúdo claro. Mesmo arquivo enviado em vários chats
    /// gera o mesmo hash → cache deduplica.
    pub media_sha256: Option<&'a str>,
    /// JPEG/PNG inline preview pra image/video/sticker/document. Persisted
    /// como BLOB e renderizado pela UI antes do download.
    pub media_thumbnail: Option<&'a [u8]>,
}

#[derive(Debug, Clone)]
pub struct MessageBatchResult {
    pub affected_chat_ids: Vec<String>,
    pub active_chat_message_ids: Vec<String>,
    /// Per-chat list of message_ids that were genuinely inserted (not
    /// already in the table). Lets the dispatcher emit MessagesAppended
    /// for every chat whose tab might be open, avoiding alias-mismatch
    /// silent drops.
    pub new_message_ids_per_chat: std::collections::HashMap<String, Vec<String>>,
}

/// Input para `run_contacts_batch`. Borrowed pra zero alocação extra.
#[derive(Debug, Clone, Copy)]
pub struct ContactBatchInput<'a> {
    pub jid: &'a str,
    pub lid: Option<&'a str>,
    pub phone_number: Option<&'a str>,
    pub push_name: Option<&'a str>,
    pub contact_name: Option<&'a str>,
    pub verified_name: Option<&'a str>,
    pub avatar_url: Option<&'a str>,
    pub status: Option<&'a str>,
}

/// Input para `run_groups_batch`.
#[derive(Debug, Clone, Copy)]
pub struct GroupBatchInput<'a> {
    pub jid: &'a str,
    pub subject: Option<&'a str>,
    pub owner: Option<&'a str>,
    pub description: Option<&'a str>,
    pub participants_json: Option<&'a str>,
    pub participant_jids: &'a [&'a str],
}

/// Linha de mensagem pronta pra UI: nome do remetente já resolvido via JOIN.
/// Para mensagens "from me", `sender_name` vai estar `None` (UI usa "Você").
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MessageRow {
    pub message_id: String,
    pub chat_id: String,
    pub sender_contact_id: Option<String>,
    pub sender_name: Option<String>,
    /// Resolved JID of the sender (from contacts JOIN). Used to ask the
    /// worker to fetch a profile picture and to dedupe per-sender
    /// avatar requests in the chat tab.
    pub sender_jid: Option<String>,
    /// Local cached path of the sender's profile picture, if the worker
    /// has previously fetched it. Empty until `AvatarReady` arrives for
    /// `sender_jid`.
    pub sender_avatar_path: Option<String>,
    pub content: Option<String>,
    pub message_type: String,
    pub timestamp: i64,
    pub is_from_me: bool,
    pub media_mimetype: Option<String>,
    pub media_filename: Option<String>,
    pub media_duration_secs: Option<i64>,
    pub media_width: Option<i64>,
    pub media_height: Option<i64>,
    pub media_size_bytes: Option<i64>,
    pub media_sha256: Option<String>,
    pub media_path: Option<String>,
    pub media_status: String,
    pub media_thumbnail: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Message {
    pub id: i64,
    pub account_id: String,
    pub message_id: String,
    pub chat_id: String,
    pub sender_contact_id: Option<String>,
    pub content: Option<String>,
    pub message_type: String,
    pub timestamp: i64,
    pub is_from_me: bool,
    pub raw_json: Option<String>,
    pub media_mimetype: Option<String>,
    pub media_filename: Option<String>,
    pub media_duration_secs: Option<i64>,
    pub media_width: Option<i64>,
    pub media_height: Option<i64>,
    pub media_size_bytes: Option<i64>,
    pub media_sha256: Option<String>,
    pub media_path: Option<String>,
    pub media_status: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Group {
    pub account_id: String,
    pub chat_id: String,
    pub subject: Option<String>,
    pub owner_contact_id: Option<String>,
    pub description: Option<String>,
    pub participants_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupParticipant {
    pub id: String,
    pub admin: Option<String>,
    pub phone_number: Option<String>,
}
