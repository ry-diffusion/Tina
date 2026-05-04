// Media metadata: status flips, dedup-aware download apply, sha256
// lookup, and avatar path persistence.

use crate::error::Result;

use super::db::TinaDb;

impl TinaDb {
    /// Marca o status de mídia de uma mensagem (e opcionalmente seu
    /// path). Usado pelo download tardio: se a cache in-memory do
    /// nanachi não tem o proto da mensagem, o Rust passa este JSON na
    /// IPC `DownloadMedia` e o Go re-hidrata o `*waE2E.Message` antes
    /// de chamar `whatsmeow.Download`.
    pub async fn get_message_raw_json(
        &self,
        account_id: &str,
        message_id: &str,
    ) -> Result<Option<String>> {
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT raw_json FROM messages WHERE account_id = ? AND message_id = ?",
        )
        .bind(account_id)
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.and_then(|(j,)| j))
    }

    pub async fn set_media_status(
        &self,
        account_id: &str,
        message_id: &str,
        status: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE messages SET media_status = ? WHERE account_id = ? AND message_id = ?")
            .bind(status)
            .bind(account_id)
            .bind(message_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Persiste path + marca como done. Se um sha256 for fornecido,
    /// propaga o path para todas as mensagens com aquele hash (cache
    /// dedup). Retorna a lista de message_ids realmente atualizados
    /// (incluindo o originador).
    pub async fn apply_media_downloaded(
        &self,
        account_id: &str,
        message_id: &str,
        path: &str,
        sha256: Option<&str>,
        mimetype: Option<&str>,
    ) -> Result<Vec<String>> {
        let mut tx = self.pool.begin().await?;
        let mut ids: Vec<String> = vec![message_id.to_string()];

        sqlx::query(
            r#"UPDATE messages
               SET media_path = ?, media_status = 'done',
                   media_mimetype = COALESCE(media_mimetype, ?)
               WHERE account_id = ? AND message_id = ?"#,
        )
        .bind(path)
        .bind(mimetype)
        .bind(account_id)
        .bind(message_id)
        .execute(&mut *tx)
        .await?;

        if let Some(sha) = sha256 {
            let extra: Vec<String> = sqlx::query_scalar::<_, String>(
                "SELECT message_id FROM messages
                 WHERE account_id = ? AND media_sha256 = ?
                   AND media_path IS NULL AND message_id != ?",
            )
            .bind(account_id)
            .bind(sha)
            .bind(message_id)
            .fetch_all(&mut *tx)
            .await?;

            if !extra.is_empty() {
                sqlx::query(
                    r#"UPDATE messages
                       SET media_path = ?, media_status = 'done',
                           media_mimetype = COALESCE(media_mimetype, ?)
                       WHERE account_id = ? AND media_sha256 = ?
                         AND media_path IS NULL"#,
                )
                .bind(path)
                .bind(mimetype)
                .bind(account_id)
                .bind(sha)
                .execute(&mut *tx)
                .await?;
                ids.extend(extra);
            }
        }

        tx.commit().await?;
        Ok(ids)
    }

    /// Lookup auxiliar pra dedup pré-download: dado um sha256, devolve
    /// o path se já existe alguma cópia em outra mensagem.
    pub async fn find_existing_media_path(
        &self,
        account_id: &str,
        sha256: &str,
    ) -> Result<Option<String>> {
        let path: Option<String> = sqlx::query_scalar(
            "SELECT media_path FROM messages
             WHERE account_id = ? AND media_sha256 = ? AND media_path IS NOT NULL
             LIMIT 1",
        )
        .bind(account_id)
        .bind(sha256)
        .fetch_optional(&self.pool)
        .await?;
        Ok(path)
    }

    /// Persiste o caminho local do profile pic. Atualiza tanto `chats`
    /// (se a entidade for um chat de grupo/canal) quanto `contacts`
    /// (DM) — o resolver da chat list via JOIN ainda funciona em ambos.
    pub async fn set_avatar_path(&self, account_id: &str, jid: &str, path: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE chats SET avatar_path = ?, updated_at = strftime('%s','now')
             WHERE account_id = ? AND chat_id IN (
                SELECT chat_id FROM chat_aliases WHERE account_id = ? AND alias_jid = ?
             )",
        )
        .bind(path)
        .bind(account_id)
        .bind(account_id)
        .bind(jid)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE contacts SET avatar_path = ?, updated_at = strftime('%s','now')
             WHERE account_id = ? AND contact_id IN (
                SELECT contact_id FROM contact_aliases WHERE account_id = ? AND alias_jid = ?
             )",
        )
        .bind(path)
        .bind(account_id)
        .bind(account_id)
        .bind(jid)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}
