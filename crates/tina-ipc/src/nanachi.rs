use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::process::Command;
use tokio::sync::mpsc;

use tina_core::{IpcCommand, IpcEvent, IpcMessage};

use crate::error::{IpcError, Result};
use crate::process::ProcessHandle;

/// Metadata de comando em voo: nome do tipo (`StartAccount`, `Reconcile`, …)
/// e instante de envio. Usado pra calcular round-trip quando chega o
/// `CommandResult` correspondente.
#[derive(Debug, Clone)]
pub struct CommandTiming {
    pub kind: &'static str,
    pub sent_at: Instant,
}

pub struct NanachiManager {
    nanachi_dir: PathBuf,
    process: Option<ProcessHandle>,
    event_tx: mpsc::Sender<String>,
    event_rx: Option<mpsc::Receiver<String>>,
    /// `command_id` → metadata; cresce no `send_command`, drena no
    /// `take_command_timing`. `std::sync::Mutex` é ok aqui — locks são
    /// curtíssimos (insert/remove de uma entrada).
    outstanding: Arc<Mutex<HashMap<String, CommandTiming>>>,
}

impl NanachiManager {
    pub fn new(nanachi_dir: PathBuf) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1000);
        Self {
            nanachi_dir,
            process: None,
            event_tx,
            event_rx: Some(event_rx),
            outstanding: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn take_event_receiver(&mut self) -> Option<mpsc::Receiver<String>> {
        self.event_rx.take()
    }

    fn binary_path(&self) -> PathBuf {
        let name = if cfg!(windows) { "nanachi.exe" } else { "nanachi" };
        self.nanachi_dir.join(name)
    }

    pub async fn ensure_dependencies(&self) -> Result<()> {
        let go_mod = self.nanachi_dir.join("go.mod");
        if !go_mod.exists() {
            return Err(IpcError::BuildFailed(
                "go.mod not found in nanachi directory".to_string(),
            ));
        }

        let bin = self.binary_path();
        let needs_build = match (bin.metadata(), go_mod.metadata()) {
            (Ok(b), Ok(m)) => match (b.modified(), m.modified()) {
                (Ok(bt), Ok(mt)) => bt < mt,
                _ => false,
            },
            (Err(_), _) => true,
            _ => false,
        };

        if needs_build {
            tracing::info!("Building nanachi (whatsmeow) Go binary...");
            self.run_go_build().await?;
        }

        Ok(())
    }

    async fn run_go_build(&self) -> Result<()> {
        let bin_name = if cfg!(windows) { "nanachi.exe" } else { "nanachi" };

        let output = Command::new("go")
            .args(["build", "-o", bin_name, "."])
            .current_dir(&self.nanachi_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| IpcError::BuildFailed(format!("failed to invoke `go`: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(IpcError::BuildFailed(stderr.into_owned()));
        }

        tracing::info!("nanachi build completed");
        Ok(())
    }

    pub async fn start(&mut self) -> Result<()> {
        if self.process.is_some() {
            return Ok(());
        }

        self.ensure_dependencies().await?;

        tracing::info!("Starting nanachi process...");

        let bin = self.binary_path();
        let bin_str = bin.to_string_lossy().into_owned();

        let handle = ProcessHandle::spawn(
            &self.nanachi_dir,
            &bin_str,
            &[],
            self.event_tx.clone(),
        )
        .await?;

        self.process = Some(handle);

        tracing::info!("Nanachi process started");
        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        if let Some(mut process) = self.process.take() {
            let _ = self.send_command(IpcCommand::Shutdown).await;
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            let _ = process.kill().await;
            tracing::info!("Nanachi process stopped");
        }
        Ok(())
    }

    pub async fn send_command(&self, command: IpcCommand) -> Result<()> {
        let process = self.process.as_ref().ok_or(IpcError::ProcessNotRunning)?;
        let kind = command_kind(&command);
        let message = IpcMessage::new_command(command);
        let id = message.id.clone();
        let line = message.to_line();
        // Registra antes do write — se o write bloquear, o reloj já está rodando.
        if let Ok(mut map) = self.outstanding.lock() {
            map.insert(
                id,
                CommandTiming {
                    kind,
                    sent_at: Instant::now(),
                },
            );
        }
        process.send(&line).await
    }

    /// Drena a metadata de um comando completo (ao receber `CommandResult`).
    /// Retorna `None` se o id é desconhecido (provavelmente um Result de
    /// outra instância ou já consumido).
    pub fn take_command_timing(&self, command_id: &str) -> Option<CommandTiming> {
        self.outstanding
            .lock()
            .ok()
            .and_then(|mut m| m.remove(command_id))
    }

    /// Acessor read-only ao mapa de outstanding (pra compartilhar com tasks
    /// fora da read-lock do `RwLock<NanachiManager>` no worker).
    pub fn outstanding_handle(&self) -> Arc<Mutex<HashMap<String, CommandTiming>>> {
        self.outstanding.clone()
    }

    /// PID of the running nanachi subprocess; `None` if not started or
    /// already exited. Used by the settings dialog to read RSS from
    /// `/proc/<pid>/status`.
    pub fn child_pid(&self) -> Option<u32> {
        self.process.as_ref().and_then(|p| p.pid())
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

fn command_kind(c: &IpcCommand) -> &'static str {
    match c {
        IpcCommand::StartAccount { .. } => "StartAccount",
        IpcCommand::StopAccount { .. } => "StopAccount",
        IpcCommand::Logout { .. } => "Logout",
        IpcCommand::SendMessage { .. } => "SendMessage",
        IpcCommand::SendMedia { .. } => "SendMedia",
        IpcCommand::MarkRead { .. } => "MarkRead",
        IpcCommand::Reconcile { .. } => "Reconcile",
        IpcCommand::DownloadMedia { .. } => "DownloadMedia",
        IpcCommand::FetchAvatar { .. } => "FetchAvatar",
        IpcCommand::RefreshChat { .. } => "RefreshChat",
        IpcCommand::Shutdown => "Shutdown",
    }
}

impl Drop for NanachiManager {
    fn drop(&mut self) {
        // `ProcessHandle` wraps a `tokio::process::Child` built with
        // `kill_on_drop(true)`; dropping the handle sends SIGKILL on
        // Linux. The previous `let _ = process.kill()` pattern was a
        // bug — it produced an async Future that was dropped unpolled,
        // so the kill never happened. The kill_on_drop guard does the
        // right thing automatically; we just take the Option.
        let _ = self.process.take();
    }
}
