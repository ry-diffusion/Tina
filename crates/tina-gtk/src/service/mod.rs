// Service worker bridge.
//
// Owns a tokio runtime on a dedicated OS thread, where it instantiates a
// `TinaWorker`. The UI sends `Cmd`s over a `tokio::sync::mpsc` channel;
// the worker pushes `WorkerEvent`s back into the relm4 component as
// `AppMsg`.

mod cmd;
mod events;
mod handlers;
mod runtime;
mod state;

pub use cmd::{Cmd, ServiceHandle};
pub use runtime::ServiceWorker;
