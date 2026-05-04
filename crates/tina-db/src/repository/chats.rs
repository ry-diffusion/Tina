// Chat resolver + display-name / pin / last-message updates +
// the SELECT clause shared with sidebar row queries.

use crate::error::Result;
use crate::models::{Chat, ChatKind, ChatRow};

use super::aliases::{link_alias_tx, register_chat_alias_tx};
use super::db::TinaDb;
use super::util::now_ts;

impl TinaDb {
    /// Registra (ou recupera) um chat para um JID/LID. Idempotente.
    pub async fn register_chat_alias(
        &self,
        account_id: &str,
        alias_jid: &str,
        kind: ChatKind,
    ) -> Result<String> {
        let mut tx = self.pool.begin().await?;
        let id = register_chat_alias_tx(&mut tx, account_id, alias_jid, kind).await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Liga duas formas (PN/LID, ou primário+alt) ao mesmo chat. Mescla
    /// se elas estavam apontando pra chats diferentes. Retorna o chat_id
    /// final.
    pub async fn link_chat(
        &self,
        account_id: &str,
        primary_jid: &str,
        alt_jid: Option<&str>,
        kind: ChatKind,
    ) -> Result<String> {
        let mut tx = self.pool.begin().await?;
        let winner = register_chat_alias_tx(&mut tx, account_id, primary_jid, kind).await?;
        if let Some(alt) = alt_jid {
            link_alias_tx(&mut tx, account_id, alt, &winner, kind, /*chat=*/ true).await?;
        }
        tx.commit().await?;
        Ok(winner)
    }

    pub async fn get_chat_by_alias(
        &self,
        account_id: &str,
        alias_jid: &str,
    ) -> Result<Option<Chat>> {
        Ok(sqlx::query_as::<_, Chat>(
            r#"SELECT c.* FROM chats c
               JOIN chat_aliases ca ON ca.account_id = c.account_id AND ca.chat_id = c.chat_id
               WHERE c.account_id = ? AND ca.alias_jid = ?
               LIMIT 1"#,
        )
        .bind(account_id)
        .bind(alias_jid)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn get_chat(&self, account_id: &str, chat_id: &str) -> Result<Option<Chat>> {
        Ok(
            sqlx::query_as::<_, Chat>("SELECT * FROM chats WHERE account_id = ? AND chat_id = ?")
                .bind(account_id)
                .bind(chat_id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn set_chat_display_name(
        &self,
        account_id: &str,
        chat_id: &str,
        name: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE chats SET display_name = ?, updated_at = ? WHERE account_id = ? AND chat_id = ?",
        )
        .bind(name)
        .bind(now_ts())
        .bind(account_id)
        .bind(chat_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_chat_pinned(
        &self,
        account_id: &str,
        chat_id: &str,
        pinned: bool,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE chats SET pinned = ?, updated_at = ? WHERE account_id = ? AND chat_id = ?",
        )
        .bind(if pinned { 1 } else { 0 })
        .bind(now_ts())
        .bind(account_id)
        .bind(chat_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_chat_last_message(
        &self,
        account_id: &str,
        chat_id: &str,
        message_id: &str,
        preview: Option<&str>,
        timestamp: i64,
        from_me: bool,
        sender_contact_id: Option<&str>,
    ) -> Result<()> {
        // Só atualiza se a mensagem é mais recente que a atual.
        sqlx::query(
            r#"UPDATE chats
               SET last_message_id = ?,
                   last_message_preview = ?,
                   last_message_ts = ?,
                   last_message_from_me = ?,
                   last_sender_contact_id = ?,
                   updated_at = ?
               WHERE account_id = ? AND chat_id = ?
                 AND (last_message_ts IS NULL OR last_message_ts <= ?)"#,
        )
        .bind(message_id)
        .bind(preview)
        .bind(timestamp)
        .bind(from_me)
        .bind(sender_contact_id)
        .bind(now_ts())
        .bind(account_id)
        .bind(chat_id)
        .bind(timestamp)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Linhas prontas pra UI: nome de DM resolvido via JOIN com `contacts`,
    /// nome de grupo/newsletter pegando `chats.display_name`. Ordenação por
    /// timestamp da última mensagem desc.
    /// One row per contact who's posted to `status@broadcast` for this
    /// account. Aggregated on the fly from the `messages` table —
    /// status posts are stored alongside regular messages, just with
    /// the broadcast chat_jid. The sender is resolved via
    /// `sender_contact_id` (already populated by `run_message_batch`)
    /// so per-author rows pick up the contact's display name + avatar.
    pub async fn list_status_authors(&self, account_id: &str) -> Result<Vec<crate::models::StatusAuthorRow>> {
        let q = r#"
            WITH posts AS (
                SELECT
                    sender_contact_id,
                    timestamp,
                    message_type,
                    content,
                    ROW_NUMBER() OVER (
                        PARTITION BY sender_contact_id ORDER BY timestamp DESC
                    ) AS row_num
                FROM messages
                WHERE account_id = ?1
                  AND chat_id = 'status@broadcast'
                  AND sender_contact_id IS NOT NULL
                  AND sender_contact_id != ''
            ),
            agg AS (
                SELECT
                    sender_contact_id,
                    MAX(timestamp) AS last_ts,
                    COUNT(*) AS post_count
                FROM posts
                GROUP BY sender_contact_id
            ),
            tip AS (
                SELECT sender_contact_id, message_type, content
                FROM posts
                WHERE row_num = 1
            )
            SELECT
                COALESCE(c.pn_jid, c.lid_jid, agg.sender_contact_id) AS sender_jid,
                COALESCE(NULLIF(c.push_name, ''),
                         NULLIF(c.contact_name, ''),
                         NULLIF(c.verified_name, ''),
                         NULLIF(c.phone_number, ''),
                         agg.sender_contact_id) AS name,
                c.avatar_path AS avatar_path,
                agg.last_ts AS last_ts,
                COALESCE(tip.message_type, 'text') AS last_message_type,
                tip.content AS last_preview,
                agg.post_count AS post_count
            FROM agg
            JOIN tip ON tip.sender_contact_id = agg.sender_contact_id
            LEFT JOIN contacts c
                ON c.account_id = ?1 AND c.contact_id = agg.sender_contact_id
            ORDER BY agg.last_ts DESC
        "#;
        Ok(sqlx::query_as::<_, crate::models::StatusAuthorRow>(q)
            .bind(account_id)
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn list_chat_rows(&self, account_id: &str) -> Result<Vec<ChatRow>> {
        let q = chat_row_select_clause(false);
        Ok(sqlx::query_as::<_, ChatRow>(&q)
            .bind(account_id)
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn get_chat_rows(
        &self,
        account_id: &str,
        chat_ids: &[String],
    ) -> Result<Vec<ChatRow>> {
        if chat_ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat_n("?", chat_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let q = chat_row_select_clause(true).replace("__IDS__", &placeholders);
        let mut query = sqlx::query_as::<_, ChatRow>(&q).bind(account_id);
        for id in chat_ids {
            query = query.bind(id);
        }
        Ok(query.fetch_all(&self.pool).await?)
    }
}

pub(super) fn chat_row_select_clause(filter_by_ids: bool) -> String {
    let where_clause = if filter_by_ids {
        "WHERE c.account_id = ? AND c.chat_id IN (__IDS__)".to_string()
    } else {
        "WHERE c.account_id = ?".to_string()
    };
    // Two contact JOINs: one resolves the chat itself (DM name +
    // avatar), the second resolves the *sender* of the chat's last
    // message so group rows can render "Author: preview" without a
    // round-trip per row. `cs` shares the contacts table; the
    // `last_sender_contact_id` already lives on chats.
    format!(
        r#"SELECT
            c.chat_id AS chat_id,
            c.kind AS kind,
            COALESCE(
                c.display_name,
                ct.contact_name,
                ct.push_name,
                ct.verified_name,
                ct.business_name,
                ct.phone_number,
                c.chat_id
            ) AS name,
            COALESCE(c.avatar_url, ct.avatar_url) AS avatar_url,
            COALESCE(c.avatar_path, ct.avatar_path) AS avatar_path,
            c.last_message_preview,
            c.last_message_ts,
            c.last_message_from_me,
            c.last_message_type,
            c.last_message_duration_secs,
            c.unread_count,
            c.pinned,
            CASE
                WHEN c.last_sender_contact_id IS NULL THEN NULL
                ELSE COALESCE(
                    NULLIF(cs.contact_name, ''),
                    NULLIF(cs.push_name, ''),
                    NULLIF(cs.verified_name, ''),
                    NULLIF(cs.business_name, ''),
                    NULLIF(cs.phone_number, ''),
                    c.last_sender_contact_id
                )
            END AS last_sender_name
           FROM chats c
           LEFT JOIN contact_aliases ca
                  ON ca.account_id = c.account_id AND ca.alias_jid = c.chat_id
           LEFT JOIN contacts ct
                  ON ct.account_id = c.account_id AND ct.contact_id = ca.contact_id
           LEFT JOIN contacts cs
                  ON cs.account_id = c.account_id
                 AND cs.contact_id = c.last_sender_contact_id
           {where_clause}
           ORDER BY c.last_message_ts DESC NULLS LAST, c.updated_at DESC"#,
    )
}
