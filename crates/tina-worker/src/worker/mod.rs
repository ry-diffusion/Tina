// `TinaWorker`: bridges the GTK UI (sync, mpsc-style) to the
// `NanachiManager` (async, IPC over a Go subprocess) and the
// `TinaDb` (async, sqlx pool).
//
// Submodules:
//   * `core`        — `TinaWorker` struct + small forwarding methods
//   * `download`    — `download_media` with cache/dedup short-circuits
//   * `dispatcher`  — IPC reader → DirtyBuffer → flush
//   * `realtime`    — handlers for low-volume events (Connected, QR, …)
//   * `batch`       — pure DB-batch helpers (contacts/groups)
//   * `flush`       — apply buffer + emit `ChatsUpserted`
//   * `buffer`      — the buffer struct + flush thresholds

mod batch;
mod buffer;
mod core;
mod dispatcher;
mod download;
mod flush;
mod realtime;

pub use core::TinaWorker;
