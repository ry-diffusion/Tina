// Single-message insertion and read-paths used by the worker / UI.

use crate::error::Result;
use crate::models::{Message, MessageRow};

use super::db::TinaDb;
use super::util::now_ts;

impl TinaDb {
    /// Insere mensagem já com chat_id/sender resolvidos. Retorna `true`
    /// se foi nova (ON CONFLICT DO NOTHING).
    pub async fn insert_message(
        &self,
        account_id: &str,
        message_id: &str,
        chat_id: &str,
        sender_contact_id: Option<&str>,
        content: Option<&str>,
        message_type: &str,
        timestamp: i64,
        is_from_me: bool,
        raw_json: Option<&str>,
    ) -> Result<bool> {
        let res = sqlx::query(
            r#"INSERT OR IGNORE INTO messages
               (account_id, message_id, chat_id, sender_contact_id, content, message_type, timestamp, is_from_me, raw_json, created_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(account_id)
        .bind(message_id)
        .bind(chat_id)
        .bind(sender_contact_id)
        .bind(content)
        .bind(message_type)
        .bind(timestamp)
        .bind(is_from_me)
        .bind(raw_json)
        .bind(now_ts())
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn get_messages_by_chat(
        &self,
        account_id: &str,
        chat_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Message>> {
        Ok(sqlx::query_as::<_, Message>(
            "SELECT * FROM messages WHERE account_id = ? AND chat_id = ? ORDER BY timestamp DESC LIMIT ? OFFSET ?",
        )
        .bind(account_id)
        .bind(chat_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Mensagens com nome de remetente resolvido (pra renderização).
    /// Ordem cronológica ascendente pra a UI mostrar do mais antigo pro
    /// mais novo.
    pub async fn get_message_rows_by_chat(
        &self,
        account_id: &str,
        chat_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<MessageRow>> {
        let rows = sqlx::query_as::<_, MessageRow>(MESSAGE_ROWS_BY_CHAT_SQL)
            .bind(account_id)
            .bind(chat_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;
        // Reverte para ordem cronológica ascendente.
        let mut rows = rows;
        rows.reverse();
        Ok(rows)
    }

    /// Páginação para trás: mensagens com `timestamp < before_ts` (ou
    /// todas as mais antigas se for None), em ordem ASC. Usado para
    /// virtualização na UI — quando o usuário scrolla pro topo, pedimos
    /// o próximo lote mais antigo.
    pub async fn get_message_rows_before(
        &self,
        account_id: &str,
        chat_id: &str,
        before_ts: i64,
        limit: i64,
    ) -> Result<Vec<MessageRow>> {
        let rows = sqlx::query_as::<_, MessageRow>(MESSAGE_ROWS_BEFORE_SQL)
            .bind(account_id)
            .bind(chat_id)
            .bind(before_ts)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        let mut rows = rows;
        rows.reverse();
        Ok(rows)
    }

    pub async fn get_message_rows_by_ids(
        &self,
        account_id: &str,
        message_ids: &[String],
    ) -> Result<Vec<MessageRow>> {
        if message_ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat_n("?", message_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "{}\nWHERE m.account_id = ? AND m.message_id IN ({})\nORDER BY m.timestamp ASC",
            message_rows_by_ids_select(),
            placeholders,
        );
        let mut q = sqlx::query_as::<_, MessageRow>(&sql).bind(account_id);
        for id in message_ids {
            q = q.bind(id);
        }
        Ok(q.fetch_all(&self.pool).await?)
    }

    pub async fn count_messages_for_chat(&self, account_id: &str, chat_id: &str) -> Result<i64> {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM messages WHERE account_id = ? AND chat_id = ?",
        )
        .bind(account_id)
        .bind(chat_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(n)
    }
}

fn message_rows_by_ids_select() -> &'static str {
    r#"SELECT
         m.message_id,
         m.chat_id,
         m.sender_contact_id,
         COALESCE(ct.contact_name, ct.push_name, ct.verified_name, ct.business_name, ct.phone_number) AS sender_name,
         COALESCE(ct.pn_jid, ct.lid_jid) AS sender_jid,
         ct.avatar_path AS sender_avatar_path,
         m.content,
         m.message_type,
         m.timestamp,
         m.is_from_me,
         m.media_mimetype,
         m.media_filename,
         m.media_duration_secs,
         m.media_width,
         m.media_height,
         m.media_size_bytes,
         m.media_sha256,
         m.media_path,
         m.media_status,
         m.media_thumbnail
       FROM messages m
       LEFT JOIN contacts ct
         ON ct.account_id = m.account_id AND ct.contact_id = m.sender_contact_id
    "#
}

const MESSAGE_ROWS_BY_CHAT_SQL: &str = r#"SELECT
     m.message_id,
     m.chat_id,
     m.sender_contact_id,
     COALESCE(ct.contact_name, ct.push_name, ct.verified_name, ct.business_name, ct.phone_number) AS sender_name,
     COALESCE(ct.pn_jid, ct.lid_jid) AS sender_jid,
     ct.avatar_path AS sender_avatar_path,
     m.content,
     m.message_type,
     m.timestamp,
     m.is_from_me,
     m.media_mimetype,
     m.media_filename,
     m.media_duration_secs,
     m.media_width,
     m.media_height,
     m.media_size_bytes,
     m.media_sha256,
     m.media_path,
     m.media_status,
     m.media_thumbnail
   FROM messages m
   LEFT JOIN contacts ct
     ON ct.account_id = m.account_id AND ct.contact_id = m.sender_contact_id
   WHERE m.account_id = ? AND m.chat_id = ?
   ORDER BY m.timestamp DESC
   LIMIT ? OFFSET ?"#;

const MESSAGE_ROWS_BEFORE_SQL: &str = r#"SELECT
     m.message_id,
     m.chat_id,
     m.sender_contact_id,
     COALESCE(ct.contact_name, ct.push_name, ct.verified_name, ct.business_name, ct.phone_number) AS sender_name,
     COALESCE(ct.pn_jid, ct.lid_jid) AS sender_jid,
     ct.avatar_path AS sender_avatar_path,
     m.content,
     m.message_type,
     m.timestamp,
     m.is_from_me,
     m.media_mimetype,
     m.media_filename,
     m.media_duration_secs,
     m.media_width,
     m.media_height,
     m.media_size_bytes,
     m.media_sha256,
     m.media_path,
     m.media_status,
     m.media_thumbnail
   FROM messages m
   LEFT JOIN contacts ct
     ON ct.account_id = m.account_id AND ct.contact_id = m.sender_contact_id
   WHERE m.account_id = ? AND m.chat_id = ? AND m.timestamp < ?
   ORDER BY m.timestamp DESC
   LIMIT ?"#;
