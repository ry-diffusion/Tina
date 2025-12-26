use thiserror::Error;

#[derive(Error, Debug)]
pub enum IpcError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Process not running")]
    ProcessNotRunning,

    #[error("Failed to spawn process: {0}")]
    SpawnFailed(String),

    #[error("Bun install failed: {0}")]
    BunInstallFailed(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Channel closed")]
    ChannelClosed,

    #[error("Timeout")]
    Timeout,
}

pub type Result<T> = std::result::Result<T, IpcError>;
