use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use tokio::sync::mpsc;
use tracing::{info, debug};

use tina_core::{IpcCommand, IpcEvent, IpcMessage};

use crate::error::{IpcError, Result};
use crate::process::ProcessHandle;

pub struct NanachiManager {
    nanachi_dir: PathBuf,
    process: Option<ProcessHandle>,
    event_tx: mpsc::Sender<String>,
    event_rx: Option<mpsc::Receiver<String>>,
}

impl NanachiManager {
    pub fn new(nanachi_dir: PathBuf) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1000);
        Self {
            nanachi_dir,
            process: None,
            event_tx,
            event_rx: Some(event_rx),
        }
    }

    pub fn take_event_receiver(&mut self) -> Option<mpsc::Receiver<String>> {
        self.event_rx.take()
    }

    pub async fn ensure_dependencies(&self) -> Result<()> {
        let package_json = self.nanachi_dir.join("package.json");
        let node_modules = self.nanachi_dir.join("node_modules");

        if !package_json.exists() {
            return Err(IpcError::BunInstallFailed(
                "package.json not found".to_string(),
            ));
        }

        if !node_modules.exists() {
            info!("Installing nanachi dependencies with bun");
            self.run_bun_install().await?;
        }

        Ok(())
    }

    async fn run_bun_install(&self) -> Result<()> {
        let output = Command::new("bun")
            .arg("install")
            .current_dir(&self.nanachi_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| IpcError::BunInstallFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(IpcError::BunInstallFailed(stderr.to_string()));
        }

        info!("bun install completed successfully");
        Ok(())
    }

    pub async fn start(&mut self) -> Result<()> {
        if self.process.is_some() {
            return Ok(());
        }

        self.ensure_dependencies().await?;

        info!("Starting nanachi process");

        let handle = ProcessHandle::spawn(
            &self.nanachi_dir,
            "bun",
            &["run", "index.ts"],
            self.event_tx.clone(),
        )
        .await?;

        self.process = Some(handle);

        info!("Nanachi process started successfully");
        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        if let Some(mut process) = self.process.take() {
            info!("Stopping nanachi process");
            let _ = self.send_command(IpcCommand::Shutdown).await;
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            let _ = process.kill().await;
            info!("Nanachi process stopped");
        }
        Ok(())
    }

    pub async fn send_command(&self, command: IpcCommand) -> Result<()> {
        let process = self.process.as_ref().ok_or(IpcError::ProcessNotRunning)?;
        
        let (cmd_name, account_id) = match &command {
            IpcCommand::StartAccount { account_id } => ("StartAccount", Some(account_id.as_str())),
            IpcCommand::StopAccount { account_id } => ("StopAccount", Some(account_id.as_str())),
            IpcCommand::SetAuthState { account_id, .. } => ("SetAuthState", Some(account_id.as_str())),
            IpcCommand::SendMessage { account_id, .. } => ("SendMessage", Some(account_id.as_str())),
            IpcCommand::GetQrCode { account_id } => ("GetQrCode", Some(account_id.as_str())),
            IpcCommand::GetContacts { account_id } => ("GetContacts", Some(account_id.as_str())),
            IpcCommand::GetGroups { account_id } => ("GetGroups", Some(account_id.as_str())),
            IpcCommand::GetMessages { account_id, .. } => ("GetMessages", Some(account_id.as_str())),
            IpcCommand::Shutdown => ("Shutdown", None),
        };
        
        if let Some(acc_id) = account_id {
            debug!(command = cmd_name, account_id = %acc_id, "Sending IPC command");
        } else {
            debug!(command = cmd_name, "Sending IPC command");
        }
        
        let message = IpcMessage::new_command(command);
        let line = message.to_line();
        process.send(&line).await
    }

    pub fn is_running(&mut self) -> bool {
        if let Some(ref mut process) = self.process {
            match process.try_wait() {
                Ok(None) => true,
                _ => {
                    self.process = None;
                    false
                }
            }
        } else {
            false
        }
    }

    pub fn parse_event(line: &str) -> Option<IpcEvent> {
        let msg: IpcMessage = serde_json::from_str(line).ok()?;
        match msg.content {
            tina_core::IpcMessageContent::Event(event) => Some(event),
            _ => None,
        }
    }
}

impl Drop for NanachiManager {
    fn drop(&mut self) {
        if let Some(mut process) = self.process.take() {
            let _ = process.kill();
        }
    }
}
