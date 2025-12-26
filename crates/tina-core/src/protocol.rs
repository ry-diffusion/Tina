use serde::{Deserialize, Serialize};

use crate::events::{IpcCommand, IpcEvent};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcMessage {
    pub id: String,
    #[serde(flatten)]
    pub content: IpcMessageContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IpcMessageContent {
    Command(IpcCommand),
    Event(IpcEvent),
}

impl IpcMessage {
    pub fn new_command(command: IpcCommand) -> Self {
        Self {
            id: generate_id(),
            content: IpcMessageContent::Command(command),
        }
    }

    pub fn new_event(event: IpcEvent) -> Self {
        Self {
            id: generate_id(),
            content: IpcMessageContent::Event(event),
        }
    }

    pub fn to_line(&self) -> String {
        serde_json::to_string(self).unwrap_or_default() + "\n"
    }

    pub fn from_line(line: &str) -> Option<Self> {
        serde_json::from_str(line.trim()).ok()
    }
}

fn generate_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:x}", nanos)
}
