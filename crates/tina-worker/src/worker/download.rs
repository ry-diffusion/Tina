// Media-download orchestration on the worker side. Handles dedup
// (skip the IPC call if another message already has the same sha256
// downloaded) and pre-marks the row as `downloading` so the UI can
// flip its spinner before the IPC roundtrip.

use tina_core::IpcCommand;

use crate::error::Result;
use crate::events::WorkerEvent;

use super::core::TinaWorker;

impl TinaWorker {
    /// Solicita download de mídia. Faz dedup local primeiro: se outra
    /// mensagem com o mesmo sha256 já tem `media_path`, reaproveita
    /// esse caminho sem chamar o nanachi.
    pub async fn download_media(&self, account_id: &str, message_id: &str) -> Result<()> {
        if self.try_serve_from_cache(account_id, message_id).await? {
            return Ok(());
        }

        // Marca como downloading pra UI exibir spinner enquanto IPC volta.
        self.db
            .set_media_status(account_id, message_id, "downloading")
            .await?;

        // Read the persisted proto JSON so the Go side can re-hydrate
        // it when its in-memory cache misses (which is always for any
        // chat row from before the current process started).
        let raw_json = self.db.get_message_raw_json(account_id, message_id).await?;

        let nanachi = self.nanachi.read().await;
        nanachi
            .send_command(IpcCommand::DownloadMedia {
                account_id: account_id.to_string(),
                message_id: message_id.to_string(),
                raw_json,
            })
            .await?;
        Ok(())
    }

    /// Returns `true` when we served the request from cache (path on
    /// disk OR another message with the same sha256). Caller skips the
    /// IPC roundtrip in that case.
    async fn try_serve_from_cache(&self, account_id: &str, message_id: &str) -> Result<bool> {
        let Some(row) = self
            .db
            .get_message_rows_by_ids(account_id, &[message_id.to_string()])
            .await?
            .into_iter()
            .next()
        else {
            return Ok(false);
        };
        // (1) The row already has a local copy.
        if let Some(path) = row.media_path.as_deref()
            && std::path::Path::new(path).exists() {
                let _ = self
                    .event_tx
                    .send(WorkerEvent::MediaReady {
                        account_id: account_id.to_string(),
                        affected_message_ids: vec![message_id.to_string()],
                        path: path.to_string(),
                        mimetype: row.media_mimetype.clone(),
                    })
                    .await;
                return Ok(true);
            }
        // (2) Another message has the same content already on disk.
        if let Some(sha) = row.media_sha256.as_deref()
            && let Some(existing_path) =
                self.db.find_existing_media_path(account_id, sha).await?
            {
                let affected = self
                    .db
                    .apply_media_downloaded(
                        account_id,
                        message_id,
                        &existing_path,
                        Some(sha),
                        row.media_mimetype.as_deref(),
                    )
                    .await?;
                let _ = self
                    .event_tx
                    .send(WorkerEvent::MediaReady {
                        account_id: account_id.to_string(),
                        affected_message_ids: affected,
                        path: existing_path,
                        mimetype: row.media_mimetype.clone(),
                    })
                    .await;
                return Ok(true);
            }
        Ok(false)
    }
}
