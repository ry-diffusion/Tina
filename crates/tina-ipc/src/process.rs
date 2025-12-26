use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

use crate::error::{IpcError, Result};

pub struct ProcessHandle {
    child: Child,
    stdin_tx: mpsc::Sender<String>,
}

impl ProcessHandle {
    pub async fn spawn(
        working_dir: &Path,
        command: &str,
        args: &[&str],
        event_tx: mpsc::Sender<String>,
    ) -> Result<Self> {
        let mut child = Command::new(command)
            .args(args)
            .current_dir(working_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| IpcError::SpawnFailed(e.to_string()))?;

        let stdout = child.stdout.take().ok_or(IpcError::ProcessNotRunning)?;
        let stderr = child.stderr.take().ok_or(IpcError::ProcessNotRunning)?;
        let stdin = child.stdin.take().ok_or(IpcError::ProcessNotRunning)?;

        let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(100);

        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(line) = stdin_rx.recv().await {
                if stdin.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
                if stdin.flush().await.is_err() {
                    break;
                }
            }
        });

        let event_tx_clone = event_tx.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if event_tx_clone.send(line).await.is_err() {
                    break;
                }
            }
        });

        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::warn!("nanachi stderr: {}", line);
            }
        });

        Ok(Self { child, stdin_tx })
    }

    pub async fn send(&self, line: &str) -> Result<()> {
        let msg = if line.ends_with('\n') {
            line.to_string()
        } else {
            format!("{}\n", line)
        };

        self.stdin_tx
            .send(msg)
            .await
            .map_err(|_| IpcError::ChannelClosed)
    }

    pub async fn kill(&mut self) -> Result<()> {
        self.child.kill().await.map_err(IpcError::Io)
    }

    pub fn try_wait(&mut self) -> Result<Option<std::process::ExitStatus>> {
        self.child.try_wait().map_err(IpcError::Io)
    }
}
