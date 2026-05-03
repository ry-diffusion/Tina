use tina_db::{ChatRow, MessageRow};

#[derive(Debug, Clone)]
pub enum WorkerEvent {
    NanachiReady,
    AccountReady { account_id: String },

    QrCode { account_id: String, qr: String },
    Connected { account_id: String, phone_number: Option<String> },
    Disconnected { account_id: String, reason: String },
    LoggedOut { account_id: String },

    /// Snapshot completo (lista inicial) ou parcial (após batch) de chats.
    ChatsUpserted { account_id: String, rows: Vec<ChatRow> },

    /// Mensagens novas para o chat ativo (subscrito via `set_active_chat`).
    /// `messages` já vêm com `sender_name` resolvido.
    MessagesAppended {
        account_id: String,
        chat_id: String,
        messages: Vec<MessageRow>,
    },

    HistorySyncComplete { account_id: String, messages_count: usize },

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
}
