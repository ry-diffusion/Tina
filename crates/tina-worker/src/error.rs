use thiserror::Error;

#[derive(Error, Debug)]
pub enum WorkerError {
    #[error("Database error: {0}")]
    Db(#[from] tina_db::DbError),

    #[error("IPC error: {0}")]
    Ipc(#[from] tina_ipc::IpcError),

    #[error("Worker not started")]
    NotStarted,

    #[error("Account not found: {0}")]
    AccountNotFound(String),

    #[error("Channel closed")]
    ChannelClosed,
}

pub type Result<T> = std::result::Result<T, WorkerError>;
