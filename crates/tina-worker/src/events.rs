#[derive(Debug, Clone)]
pub enum WorkerEvent {
    NanachiReady,
    AccountReady { account_id: String },
    QrCode { account_id: String, qr: String },
    Connected { account_id: String, phone_number: Option<String> },
    Disconnected { account_id: String, reason: String },
    LoggedOut { account_id: String },
    ContactsSynced { account_id: String, count: usize },
    GroupsSynced { account_id: String, count: usize },
    MessagesSynced { account_id: String, count: usize },
    HistorySyncComplete { account_id: String, messages_count: usize },
    Error { account_id: Option<String>, error: String },
}
