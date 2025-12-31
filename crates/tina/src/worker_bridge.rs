use std::path::PathBuf;
use std::sync::Arc;

use iced::Subscription;
use iced::futures::SinkExt;
use iced::stream;
use tina_worker::{TinaWorker, WorkerEvent};

#[derive(Debug, Clone)]
pub enum BridgeEvent {
    WorkerReady(WorkerHandle),
    WorkerEvent(WorkerEvent),
    Error(String),
}

#[derive(Clone)]
pub struct WorkerHandle {
    worker: Arc<TinaWorker>,
}

impl std::fmt::Debug for WorkerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerHandle").finish()
    }
}

impl WorkerHandle {
    /// Get reference to the inner worker
    pub fn worker(&self) -> Arc<TinaWorker> {
        self.worker.clone()
    }
}

fn find_nanachi_dir() -> Result<PathBuf, String> {
    let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;

    let mut current = exe_path.parent();
    while let Some(dir) = current {
        let nanachi = dir.join("nanachi");
        if nanachi.join("package.json").exists() {
            return Ok(nanachi);
        }
        current = dir.parent();
    }

    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let nanachi = cwd.join("nanachi");
    if nanachi.join("package.json").exists() {
        return Ok(nanachi);
    }

    Err("Could not find nanachi directory".into())
}

pub fn worker_subscription() -> Subscription<BridgeEvent> {
    Subscription::run(worker_stream)
}

fn worker_stream() -> impl iced::futures::Stream<Item = BridgeEvent> {
    stream::channel(
        100,
        |mut output: iced::futures::channel::mpsc::Sender<BridgeEvent>| async move {
            let nanachi_dir = match find_nanachi_dir() {
                Ok(dir) => dir,
                Err(e) => {
                    let _ = output.send(BridgeEvent::Error(e)).await;
                    loop {
                        tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
                    }
                }
            };

            tracing::info!("Nanachi directory: {}", nanachi_dir.display());

            let mut worker = match TinaWorker::new(nanachi_dir).await {
                Ok(w) => w,
                Err(e) => {
                    let _ = output
                        .send(BridgeEvent::Error(format!("Failed to create worker: {}", e)))
                        .await;
                    loop {
                        tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
                    }
                }
            };

            let mut event_rx = match worker.take_event_receiver() {
                Some(rx) => rx,
                None => {
                    let _ = output
                        .send(BridgeEvent::Error("Failed to get event receiver".into()))
                        .await;
                    loop {
                        tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
                    }
                }
            };

            if let Err(e) = worker.start().await {
                let _ = output
                    .send(BridgeEvent::Error(format!("Failed to start worker: {}", e)))
                    .await;
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
                }
            }

            let worker = Arc::new(worker);
            let handle = WorkerHandle {
                worker: worker.clone(),
            };

            let _ = output.send(BridgeEvent::WorkerReady(handle)).await;

            // Stream worker events (push events like messages received)
            while let Some(event) = event_rx.recv().await {
                let _ = output.send(BridgeEvent::WorkerEvent(event)).await;
            }

            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
            }
        },
    )
}
