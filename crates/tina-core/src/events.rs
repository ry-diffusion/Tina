use serde::{Deserialize, Serialize};

/// Go's `encoding/json` represents `[]byte` as a base64 string. Apply
/// the same transform from this side so the round-trip is symmetric.
mod thumbnail_base64 {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        match bytes {
            Some(b) => s.serialize_str(&STANDARD.encode(b)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
        let opt: Option<String> = Option::deserialize(d)?;
        match opt {
            Some(s) if !s.is_empty() => STANDARD
                .decode(&s)
                .map(Some)
                .map_err(serde::de::Error::custom),
            _ => Ok(None),
        }
    }
}

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
    /// Pede ao nanachi pra baixar+decryptar a mídia de uma mensagem.
    /// O nanachi prefere sua cache in-memory (populada quando a mensagem
    /// chegou nesta sessão); se ela não tiver o proto, faz fallback no
    /// `raw_json` que o Rust persistiu no DB e reconstrói o
    /// `*waE2E.Message` antes de chamar whatsmeow.Download. Por isso
    /// este campo é praticamente sempre passado.
    DownloadMedia {
        account_id: String,
        message_id: String,
        raw_json: Option<String>,
    },
    /// Pede ao nanachi pra obter (e baixar, se necessário) a profile
    /// picture de um JID. Resultado vira AvatarUpdated/Failed.
    FetchAvatar {
        account_id: String,
        jid: String,
    },
    /// Re-fetch metadata for a single chat (newsletter / group). The
    /// nanachi handler dispatches based on the JID server: routes
    /// `*@newsletter` to `GetNewsletterInfo`, `*@g.us` to
    /// `GetGroupInfo`. Used by the UI's `ChatInventory` to pull
    /// missing display names + avatars on demand.
    RefreshChat {
        account_id: String,
        chat_jid: String,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum IpcEvent {
    Ready { account_id: String },
    QrCode { account_id: String, qr: String },
    PairingCode { account_id: String, code: String },
    Connected {
        account_id: String,
        phone_number: Option<String>,
        jid: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        push_name: Option<String>,
    },
    Disconnected { account_id: String, reason: String },
    LoggedOut { account_id: String },

    ContactsUpsert { account_id: String, contacts: Vec<ContactData> },
    GroupsUpsert { account_id: String, groups: Vec<GroupData> },
    MessagesUpsert { account_id: String, messages: Vec<MessageData> },

    HistorySyncComplete { account_id: String, messages_count: usize },

    /// Pin state from `whatsmeow_chat_settings` (read out of the
    /// HistorySync conversation rows). `pinned = true` for any
    /// conversation with a non-zero pin timestamp; the UI uses this
    /// to mirror the WhatsApp-side pin order on first sync.
    ChatsPinUpdate {
        account_id: String,
        items: Vec<ChatPinItem>,
    },

    /// Per-chunk progress reported by whatsmeow during the initial
    /// `events.HistorySync` stream. `progress` is a 0..100 percent
    /// already calculated by the proto; `sync_type` is the enum name
    /// (INITIAL_BOOTSTRAP, RECENT, FULL, …) so the UI can show a
    /// meaningful stage label.
    HistorySyncProgress {
        account_id: String,
        sync_type: String,
        progress: u32,
    },

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

    /// Progresso de download de mídia. `total = 0` ⇒ desconhecido.
    MediaDownloadProgress {
        account_id: String,
        message_id: String,
        current: i64,
        total: i64,
    },
    /// Sucesso: arquivo persistido em `path`. `sha256` permite a worker
    /// propagar o mesmo path para outras mensagens com o mesmo conteúdo
    /// (dedup).
    MediaDownloaded {
        account_id: String,
        message_id: String,
        path: String,
        sha256: Option<String>,
        mimetype: Option<String>,
    },
    MediaDownloadFailed {
        account_id: String,
        message_id: String,
        error: String,
    },

    AvatarUpdated {
        account_id: String,
        jid: String,
        path: String,
    },
    AvatarFailed {
        account_id: String,
        jid: String,
        error: String,
    },

    CommandResult { command_id: String, success: bool, data: Option<serde_json::Value>, error: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatPinItem {
    pub chat_jid: String,
    pub pinned: bool,
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
    /// Inline preview bytes (JPEG / PNG) para image/video/sticker/document.
    /// Go envia como base64 (`[]byte` no JSON nativo do Go) e nós
    /// decodificamos pra Vec<u8> via `serde_with::base64`. Persistido
    /// como BLOB em `messages.media_thumbnail` e usado pela UI como
    /// placeholder antes do download completar.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "thumbnail_base64"
    )]
    pub thumbnail: Option<Vec<u8>>,
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
