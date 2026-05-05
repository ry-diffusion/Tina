// Owns the tokio runtime + the worker lifetime. The UI sends `Cmd`s
// over an mpsc channel; the worker pushes `WorkerEvent`s back into the
// relm4 component as `AppMsg`.

use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;

use relm4::Sender;
use tokio::sync::RwLock;
use tokio::sync::mpsc;

use tina_worker::TinaWorker;

use crate::app::AppMsg;

use super::cmd::{Cmd, ServiceHandle};
use super::events::forward_events;
use super::handlers::handle;
use super::state::ServiceState;

pub struct ServiceWorker {
    pub handle: ServiceHandle,
    _thread: JoinHandle<()>,
}

impl ServiceWorker {
    pub fn spawn(nanachi_dir: PathBuf, app_sender: Sender<AppMsg>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let app_sender_thread = app_sender.clone();
        let thread = std::thread::Builder::new()
            .name("Tina Worker".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .name("Tina Worker")
                    .enable_all()
                    .build()
                    .expect("tokio runtime");
                rt.block_on(run(nanachi_dir, rx, app_sender_thread));
            })
            .expect("spawn service thread");
        Self {
            handle: ServiceHandle { tx },
            _thread: thread,
        }
    }
}

async fn run(nanachi_dir: PathBuf, mut rx: mpsc::UnboundedReceiver<Cmd>, app: Sender<AppMsg>) {
    let mut worker = match TinaWorker::new(nanachi_dir).await {
        Ok(w) => w,
        Err(e) => {
            let _ = app.send(AppMsg::FatalError(format!("worker init: {e}")));
            return;
        }
    };
    let event_rx = match worker.take_event_receiver() {
        Some(rx) => rx,
        None => {
            let _ = app.send(AppMsg::FatalError("event channel taken".into()));
            return;
        }
    };

    let worker = Arc::new(worker);
    if let Err(e) = worker.start().await {
        let _ = app.send(AppMsg::FatalError(format!("worker start: {e}")));
        return;
    }

    let app_evt = app.clone();
    let event_pump = tokio::spawn(forward_events(event_rx, app_evt));

    // Per-account state map keyed by account_id, plus an `active`
    // pointer for the currently focused account. Single-account today
    // — only one entry, `active` always populated — but the shape
    // already supports multi-account; widening is a Cmd-side change
    // away (each Cmd carries an account_id, handlers prefer it over
    // the active pointer).
    let state = Arc::new(RwLock::new(ServiceState::default()));

    while let Some(cmd) = rx.recv().await {
        if !handle(cmd, &worker, &app, &state).await {
            break;
        }
    }

    event_pump.abort();
    let _ = worker.stop().await;
}
