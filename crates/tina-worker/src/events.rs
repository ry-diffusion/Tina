use tina_db::{ChatRow, MessageRow};

#[derive(Debug, Clone)]
pub enum WorkerEvent {
    NanachiReady,
    AccountReady { account_id: String },

    QrCode { account_id: String, qr: String },
    Connected {
        account_id: String,
        phone_number: Option<String>,
        jid: Option<String>,
        push_name: Option<String>,
    },
    Disconnected { account_id: String, reason: String },
    LoggedOut { account_id: String },

    /// Snapshot completo (lista inicial) ou parcial (após batch) de chats.
    ChatsUpserted { account_id: String, rows: Vec<ChatRow> },

    /// Mensagens novas para um chat com tab aberta na UI (registrado via
    /// `add_open_chat`). Chats fechados não geram este evento durante sync —
    /// a UI lê os snapshots via `ChatsUpserted` e re-fetch ao abrir.
    /// `messages` já vêm com `sender_name` resolvido.
    MessagesAppended {
        account_id: String,
        chat_id: String,
        messages: Vec<MessageRow>,
    },

    HistorySyncComplete { account_id: String, messages_count: usize },

    /// Live percentage from whatsmeow's `HistorySync.Progress` (0..100),
    /// emitted per chunk. Drives the syncing-scene progress bar.
    HistorySyncProgress {
        account_id: String,
        sync_type: String,
        progress: u32,
    },

    /// Atualização de progresso de uma reconciliação em andamento.
    /// `total = 0` ⇒ indeterminado (spinner).
    ReconcileProgress {
        account_id: String,
        stage: String,
        current: i64,
        total: i64,
        indeterminate: bool,
    },

    Error { account_id: Option<String>, error: String },

    /// Progresso ao vivo de um download de mídia.
    MediaDownloadProgress {
        account_id: String,
        message_id: String,
        current: i64,
        total: i64,
    },
    /// Mídia disponível em disco. `affected_message_ids` carrega todos os
    /// IDs cujo bubble agora deve apontar pra `path` (dedup por sha256
    /// resolvido pelo worker).
    MediaReady {
        account_id: String,
        affected_message_ids: Vec<String>,
        path: String,
        mimetype: Option<String>,
    },
    MediaDownloadFailed {
        account_id: String,
        message_id: String,
        error: String,
    },

    /// Profile picture finished downloading (or was found in cache).
    AvatarReady {
        account_id: String,
        jid: String,
        path: String,
    },
    AvatarFailed {
        account_id: String,
        jid: String,
        error: String,
    },
}
