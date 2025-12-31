#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncType {
    Contacts,
    Groups,
    Messages,
    History,
    All,
}

impl std::fmt::Display for SyncType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncType::Contacts => write!(f, "contacts"),
            SyncType::Groups => write!(f, "groups"),
            SyncType::Messages => write!(f, "messages"),
            SyncType::History => write!(f, "history"),
            SyncType::All => write!(f, "all"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum WorkerEvent {
    NanachiReady,
    AccountReady { account_id: String },
    QrCode { account_id: String, qr: String },
    Connected { account_id: String, phone_number: Option<String> },
    Disconnected { account_id: String, reason: String },
    LoggedOut { account_id: String },
    
    SyncStarted { account_id: String, sync_type: SyncType },
    SyncProgress { account_id: String, sync_type: SyncType, current: usize, total: Option<usize> },
    SyncCompleted { account_id: String, sync_type: SyncType, count: usize },
    
    ContactsSynced { account_id: String, count: usize },
    GroupsSynced { account_id: String, count: usize },
    MessagesSynced { account_id: String, count: usize },
    HistorySyncComplete { account_id: String, messages_count: usize },
    
    Error { account_id: Option<String>, error: String },
}
