use directories::ProjectDirs;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Pool, Row, Sqlite, Transaction};
use std::path::PathBuf;

use crate::error::{DbError, Result};
use crate::models::{Account, Chat, ChatKind, ChatRow, Contact, Message, MessageRow};
use crate::schema::{
    MIGRATION_V2_TO_V3, MIGRATION_V3_TO_V4, MIGRATION_V4_TO_V5, MIGRATION_V5_TO_V6, SCHEMA,
    SCHEMA_DROP, SCHEMA_VERSION,
};

pub struct TinaDb {
    pool: Pool<Sqlite>,
}

impl TinaDb {
    pub async fn new() -> Result<Self> {
        let db_path = Self::get_db_path()?;
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let url = format!("sqlite:{}?mode=rwc", db_path.display());
        Self::open(&url).await
    }

    pub async fn new_with_path(path: &str) -> Result<Self> {
        let url = format!("sqlite:{}?mode=rwc", path);
        Self::open(&url).await
    }

    /// Abre (ou cria) um pool, garantindo o schema na versão atual.
    /// Quando `user_version` não bate, dropamos tudo e recriamos.
    pub async fn open(url: &str) -> Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(url)
            .await?;

        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await?;
        // WAL: leitores não bloqueiam escritas e vice-versa. NORMAL: fsync
        // só em checkpoint (perda máxima ≈ último commit em queda de força,
        // aceitável p/ chat). Ganho ~5-10× em throughput de write.
        sqlx::query("PRAGMA journal_mode = WAL")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("PRAGMA synchronous = NORMAL")
            .execute(&pool)
            .await
            .ok();
        // Cache maior reduz IOPS em sync inicial.
        sqlx::query("PRAGMA cache_size = -65536") // 64MB
            .execute(&pool)
            .await
            .ok();
        sqlx::query("PRAGMA temp_store = MEMORY")
            .execute(&pool)
            .await
            .ok();

        let current: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(&pool)
            .await?;

        // Pra cada par (from, to) suportado, aplica ALTER TABLE in-place.
        // Versões mais antigas (sem migração escrita) caem no fallback de
        // drop+recreate.
        match current {
            0 => {
                // Banco novo — só cria.
                sqlx::raw_sql(SCHEMA).execute(&pool).await?;
            }
            v if v == SCHEMA_VERSION => {
                // Já na versão atual; garante que objetos novos (índices,
                // tabelas adicionadas via "IF NOT EXISTS") existam.
                sqlx::raw_sql(SCHEMA).execute(&pool).await?;
            }
            2 => {
                tracing::info!("Migrating tina.db from v2 → v6");
                sqlx::raw_sql(MIGRATION_V2_TO_V3).execute(&pool).await?;
                sqlx::raw_sql(MIGRATION_V3_TO_V4).execute(&pool).await?;
                sqlx::raw_sql(MIGRATION_V4_TO_V5).execute(&pool).await?;
                sqlx::raw_sql(MIGRATION_V5_TO_V6).execute(&pool).await?;
                sqlx::raw_sql(SCHEMA).execute(&pool).await?;
            }
            3 => {
                tracing::info!("Migrating tina.db from v3 → v6");
                sqlx::raw_sql(MIGRATION_V3_TO_V4).execute(&pool).await?;
                sqlx::raw_sql(MIGRATION_V4_TO_V5).execute(&pool).await?;
                sqlx::raw_sql(MIGRATION_V5_TO_V6).execute(&pool).await?;
                sqlx::raw_sql(SCHEMA).execute(&pool).await?;
            }
            4 => {
                tracing::info!("Migrating tina.db from v4 → v6");
                sqlx::raw_sql(MIGRATION_V4_TO_V5).execute(&pool).await?;
                sqlx::raw_sql(MIGRATION_V5_TO_V6).execute(&pool).await?;
                sqlx::raw_sql(SCHEMA).execute(&pool).await?;
            }
            5 => {
                tracing::info!("Migrating tina.db from v5 → v6 (media_thumbnail)");
                sqlx::raw_sql(MIGRATION_V5_TO_V6).execute(&pool).await?;
                sqlx::raw_sql(SCHEMA).execute(&pool).await?;
            }
            other => {
                tracing::warn!(
                    "Unsupported schema version (db={}, expected={}). Recreating from scratch.",
                    other,
                    SCHEMA_VERSION
                );
                sqlx::raw_sql(SCHEMA_DROP).execute(&pool).await?;
                sqlx::raw_sql(SCHEMA).execute(&pool).await?;
            }
        }
        sqlx::query(&format!("PRAGMA user_version = {}", SCHEMA_VERSION))
            .execute(&pool)
            .await?;

        tracing::info!("Database ready at: {}", url);
        Ok(Self { pool })
    }

    /// Construtor para testes — banco em memória sem checagem de versão.
    pub async fn in_memory() -> Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await?;
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await?;
        sqlx::raw_sql(SCHEMA).execute(&pool).await?;
        Ok(Self { pool })
    }

    fn get_db_path() -> Result<PathBuf> {
        let dirs = ProjectDirs::from("com.br", "zesmoi", "tina")
            .ok_or_else(|| DbError::AccountNotFound("Could not find project dirs".into()))?;
        Ok(dirs.data_dir().join("tina.db"))
    }

    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }

    // ---- Accounts ----

    pub async fn create_account(&self, id: &str, name: Option<&str>) -> Result<Account> {
        let now = now_ts();
        sqlx::query(
            "INSERT INTO accounts (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, updated_at = excluded.updated_at",
        )
        .bind(id)
        .bind(name)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.get_account(id).await
    }

    pub async fn get_account(&self, id: &str) -> Result<Account> {
        sqlx::query_as::<_, Account>("SELECT * FROM accounts WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(|_| DbError::AccountNotFound(id.to_string()))
    }

    pub async fn list_accounts(&self) -> Result<Vec<Account>> {
        Ok(
            sqlx::query_as::<_, Account>("SELECT * FROM accounts ORDER BY created_at")
                .fetch_all(&self.pool)
                .await?,
        )
    }

    pub async fn delete_account(&self, account_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM accounts WHERE id = ?")
            .bind(account_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn save_account_identity(
        &self,
        account_id: &str,
        phone_number: Option<&str>,
        jid: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE accounts SET phone_number = COALESCE(?, phone_number),
                                jid = COALESCE(?, jid),
                                updated_at = ? WHERE id = ?",
        )
        .bind(phone_number)
        .bind(jid)
        .bind(now_ts())
        .bind(account_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn clear_account_identity(&self, account_id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE accounts SET phone_number = NULL, jid = NULL, updated_at = ? WHERE id = ?",
        )
        .bind(now_ts())
        .bind(account_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ---- Resolver: chats ----

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

    /// Liga duas formas (PN/LID, ou primário+alt) ao mesmo chat. Mescla se
    /// elas estavam apontando pra chats diferentes. Retorna o chat_id final.
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
        let placeholders = std::iter::repeat("?")
            .take(chat_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let q = chat_row_select_clause(true).replace("__IDS__", &placeholders);
        let mut query = sqlx::query_as::<_, ChatRow>(&q).bind(account_id);
        for id in chat_ids {
            query = query.bind(id);
        }
        Ok(query.fetch_all(&self.pool).await?)
    }

    // ---- Resolver: contacts ----

    pub async fn register_contact_alias(
        &self,
        account_id: &str,
        alias_jid: &str,
    ) -> Result<String> {
        let mut tx = self.pool.begin().await?;
        let id = register_contact_alias_tx(&mut tx, account_id, alias_jid).await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn link_contact(
        &self,
        account_id: &str,
        primary_jid: &str,
        alt_jid: Option<&str>,
    ) -> Result<String> {
        let mut tx = self.pool.begin().await?;
        let winner = register_contact_alias_tx(&mut tx, account_id, primary_jid).await?;
        if let Some(alt) = alt_jid {
            link_alias_tx(&mut tx, account_id, alt, &winner, ChatKind::Unknown, false).await?;
        }
        tx.commit().await?;
        Ok(winner)
    }

    /// Atualiza campos do contato preservando valores não-nulos pré-existentes.
    pub async fn upsert_contact_fields(
        &self,
        account_id: &str,
        contact_id: &str,
        pn_jid: Option<&str>,
        lid_jid: Option<&str>,
        phone_number: Option<&str>,
        push_name: Option<&str>,
        contact_name: Option<&str>,
        business_name: Option<&str>,
        verified_name: Option<&str>,
        avatar_url: Option<&str>,
        status: Option<&str>,
        is_local: bool,
    ) -> Result<()> {
        sqlx::query(
            r#"UPDATE contacts SET
                 pn_jid = COALESCE(?, pn_jid),
                 lid_jid = COALESCE(?, lid_jid),
                 phone_number = COALESCE(?, phone_number),
                 push_name = COALESCE(?, push_name),
                 contact_name = COALESCE(?, contact_name),
                 business_name = COALESCE(?, business_name),
                 verified_name = COALESCE(?, verified_name),
                 avatar_url = COALESCE(?, avatar_url),
                 status = COALESCE(?, status),
                 is_local = ?,
                 updated_at = ?
               WHERE account_id = ? AND contact_id = ?"#,
        )
        .bind(pn_jid)
        .bind(lid_jid)
        .bind(phone_number)
        .bind(push_name)
        .bind(contact_name)
        .bind(business_name)
        .bind(verified_name)
        .bind(avatar_url)
        .bind(status)
        .bind(is_local)
        .bind(now_ts())
        .bind(account_id)
        .bind(contact_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_contact(&self, account_id: &str, contact_id: &str) -> Result<Option<Contact>> {
        Ok(sqlx::query_as::<_, Contact>(
            "SELECT * FROM contacts WHERE account_id = ? AND contact_id = ?",
        )
        .bind(account_id)
        .bind(contact_id)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn get_contact_by_alias(
        &self,
        account_id: &str,
        alias_jid: &str,
    ) -> Result<Option<Contact>> {
        Ok(sqlx::query_as::<_, Contact>(
            r#"SELECT c.* FROM contacts c
               JOIN contact_aliases ca ON ca.account_id = c.account_id AND ca.contact_id = c.contact_id
               WHERE c.account_id = ? AND ca.alias_jid = ?
               LIMIT 1"#,
        )
        .bind(account_id)
        .bind(alias_jid)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn list_contacts(&self, account_id: &str) -> Result<Vec<Contact>> {
        Ok(sqlx::query_as::<_, Contact>(
            "SELECT * FROM contacts WHERE account_id = ? ORDER BY COALESCE(contact_name, push_name, phone_number, contact_id)",
        )
        .bind(account_id)
        .fetch_all(&self.pool)
        .await?)
    }

    // ---- Groups ----

    pub async fn upsert_group(
        &self,
        account_id: &str,
        chat_id: &str,
        subject: Option<&str>,
        owner_contact_id: Option<&str>,
        description: Option<&str>,
        participants_json: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO groups (account_id, chat_id, subject, owner_contact_id, description, participants_json)
               VALUES (?, ?, ?, ?, ?, ?)
               ON CONFLICT(account_id, chat_id) DO UPDATE SET
                 subject = COALESCE(excluded.subject, subject),
                 owner_contact_id = COALESCE(excluded.owner_contact_id, owner_contact_id),
                 description = COALESCE(excluded.description, description),
                 participants_json = COALESCE(excluded.participants_json, participants_json)"#,
        )
        .bind(account_id)
        .bind(chat_id)
        .bind(subject)
        .bind(owner_contact_id)
        .bind(description)
        .bind(participants_json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ---- Messages ----

    /// Processa um lote de mensagens numa única transação. Resolve chat e
    /// sender (deduplicados em memória dentro do batch), insere todas, e
    /// agrega `update_chat_last_message` para emitir um UPDATE por chat
    /// afetado em vez de um por mensagem.
    ///
    /// Retorna `(affected_chat_ids, inserted_message_ids_per_chat_active)`,
    /// onde o segundo valor lista message_ids inseridos do `active_chat_id`
    /// para o caller emitir `MessagesAppended`.
    pub async fn run_message_batch(
        &self,
        account_id: &str,
        active_chat: Option<&str>,
        messages: &[crate::MessageBatchInput<'_>],
    ) -> Result<crate::MessageBatchResult> {
        use std::collections::{HashMap, HashSet};

        let mut tx = self.pool.begin().await?;

        // Cache local p/ não rerregistrar o mesmo chat/contact várias vezes.
        let mut chat_cache: HashMap<&str, String> = HashMap::new();
        let mut contact_cache: HashMap<&str, String> = HashMap::new();

        // Latest message por chat (pra um único UPDATE no fim).
        struct Latest<'a> {
            ts: i64,
            message_id: &'a str,
            preview: Option<&'a str>,
            placeholder: &'static str,
            from_me: bool,
            sender_contact_id: Option<String>,
            message_type: &'a str,
            duration_secs: Option<i64>,
        }
        let mut latest: HashMap<String, Latest<'_>> = HashMap::new();

        let mut affected_chats: HashSet<String> = HashSet::new();
        let mut active_inserted: Vec<String> = Vec::new();

        struct PendingInsert {
            idx: usize,
            chat_id: String,
            sender_contact_id: Option<String>,
        }
        let mut pending: Vec<PendingInsert> = Vec::with_capacity(messages.len());

        for (idx, msg) in messages.iter().enumerate() {
            // Resolve chat (cached).
            let chat_id = if let Some(c) = chat_cache.get(msg.chat_jid) {
                c.clone()
            } else {
                let kind = ChatKind::infer_from_jid(msg.chat_jid);
                let id = register_chat_alias_tx(&mut tx, account_id, msg.chat_jid, kind).await?;
                chat_cache.insert(msg.chat_jid, id.clone());
                id
            };

            // Resolve sender (cached) — pula se from_me.
            let sender_contact_id: Option<String> = if msg.is_from_me {
                None
            } else if let Some(s) = msg.sender_jid {
                if s.is_empty() {
                    None
                } else if let Some(c) = contact_cache.get(s) {
                    Some(c.clone())
                } else {
                    let id = register_contact_alias_tx(&mut tx, account_id, s).await?;
                    contact_cache.insert(s, id.clone());
                    Some(id)
                }
            } else {
                None
            };

            // Acumula pra INSERT multi-row no fim do loop.
            let placeholder: &'static str = match msg.message_type {
                "image" => "[Image]",
                "video" => "[Video]",
                "audio" => "[Audio]",
                "document" => "[Document]",
                "sticker" => "[Sticker]",
                "contact" => "[Contact]",
                "location" => "[Location]",
                _ => "[Media]",
            };
            // Otimisticamente assume insert; usaremos `INSERT OR IGNORE`
            // no statement bulk, e duplicatas serão filtradas pelo
            // `ROWS_AFFECTED` no agregador (não temos rowid por linha em
            // multi-row insert, então usamos uma SELECT pós-insert pra
            // saber quais entraram). Veja `pending` abaixo.
            let entry = latest.entry(chat_id.clone());
            use std::collections::hash_map::Entry;
            match entry {
                Entry::Vacant(v) => {
                    v.insert(Latest {
                        ts: msg.timestamp,
                        message_id: msg.message_id,
                        preview: msg.content,
                        placeholder,
                        from_me: msg.is_from_me,
                        sender_contact_id: sender_contact_id.clone(),
                        message_type: msg.message_type,
                        duration_secs: msg.media_duration_secs,
                    });
                }
                Entry::Occupied(mut o) => {
                    if msg.timestamp > o.get().ts {
                        o.insert(Latest {
                            ts: msg.timestamp,
                            message_id: msg.message_id,
                            preview: msg.content,
                            placeholder,
                            from_me: msg.is_from_me,
                            sender_contact_id: sender_contact_id.clone(),
                            message_type: msg.message_type,
                            duration_secs: msg.media_duration_secs,
                        });
                    }
                }
            }
            pending.push(PendingInsert {
                idx,
                chat_id: chat_id.clone(),
                sender_contact_id,
            });
        }

        // ---- Bulk INSERT OR IGNORE em messages, chunks de 200. ----
        // Antes do INSERT, descobre quais message_ids JÁ existem (pra não
        // marcar affected_chat sobre duplicatas).
        let mut existing_ids: HashSet<&str> = HashSet::new();
        const ID_LOOKUP_CHUNK: usize = 500;
        let all_ids: Vec<&str> = messages.iter().map(|m| m.message_id).collect();
        for chunk in all_ids.chunks(ID_LOOKUP_CHUNK) {
            let placeholders = repeat_csv("?", chunk.len());
            let sql = format!(
                "SELECT message_id FROM messages WHERE account_id = ? AND message_id IN ({})",
                placeholders
            );
            let mut q = sqlx::query(&sql).bind(account_id);
            for id in chunk {
                q = q.bind(*id);
            }
            for row in q.fetch_all(&mut *tx).await? {
                let id_str: String = row.get(0);
                // Encontra o &str equivalente em all_ids (zero alocação).
                if let Some(s) = all_ids.iter().find(|x| **x == id_str.as_str()) {
                    existing_ids.insert(*s);
                }
            }
        }

        const MSG_INSERT_CHUNK: usize = 200;
        let now = now_ts();
        for chunk in pending.chunks(MSG_INSERT_CHUNK) {
            // 18 binds/row: 10 base + 8 media (status default vem do schema).
            let row_tpl = "(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)";
            let mut sql = String::from(
                "INSERT OR IGNORE INTO messages (\
                    account_id, message_id, chat_id, sender_contact_id, content, \
                    message_type, timestamp, is_from_me, raw_json, created_at, \
                    media_mimetype, media_filename, media_duration_secs, \
                    media_width, media_height, media_size_bytes, media_sha256, \
                    media_thumbnail\
                 ) VALUES ",
            );
            sql.push_str(&repeat_csv(row_tpl, chunk.len()));
            let mut q = sqlx::query(&sql);
            for p in chunk {
                let m = &messages[p.idx];
                q = q
                    .bind(account_id)
                    .bind(m.message_id)
                    .bind(&p.chat_id)
                    .bind(&p.sender_contact_id)
                    .bind(m.content)
                    .bind(m.message_type)
                    .bind(m.timestamp)
                    .bind(m.is_from_me)
                    .bind(m.raw_json)
                    .bind(now)
                    .bind(m.media_mimetype)
                    .bind(m.media_filename)
                    .bind(m.media_duration_secs)
                    .bind(m.media_width)
                    .bind(m.media_height)
                    .bind(m.media_size_bytes)
                    .bind(m.media_sha256)
                    .bind(m.media_thumbnail);
            }
            q.execute(&mut *tx).await?;
        }

        // Marca affected_chat e active_inserted só p/ as que de fato entraram.
        // Também acumula por chat para que o dispatcher possa emitir
        // MessagesAppended para qualquer chat com tab aberta.
        let mut new_per_chat: HashMap<String, Vec<String>> = HashMap::new();
        for p in &pending {
            let m = &messages[p.idx];
            if existing_ids.contains(&m.message_id) {
                continue; // já existia, não conta
            }
            affected_chats.insert(p.chat_id.clone());
            new_per_chat
                .entry(p.chat_id.clone())
                .or_default()
                .push(m.message_id.to_string());
            if let Some(active) = active_chat {
                if active == p.chat_id {
                    active_inserted.push(m.message_id.to_string());
                }
            }
        }

        // Um UPDATE por chat afetado, agregado.
        for (chat_id, l) in &latest {
            let preview_str: Option<&str> = l.preview.or(Some(l.placeholder));
            sqlx::query(
                r#"UPDATE chats
                   SET last_message_id = ?,
                       last_message_preview = ?,
                       last_message_ts = ?,
                       last_message_from_me = ?,
                       last_sender_contact_id = ?,
                       last_message_type = ?,
                       last_message_duration_secs = ?,
                       updated_at = ?
                   WHERE account_id = ? AND chat_id = ?
                     AND (last_message_ts IS NULL OR last_message_ts <= ?)"#,
            )
            .bind(l.message_id)
            .bind(preview_str)
            .bind(l.ts)
            .bind(l.from_me)
            .bind(&l.sender_contact_id)
            .bind(l.message_type)
            .bind(l.duration_secs)
            .bind(now_ts())
            .bind(account_id)
            .bind(chat_id)
            .bind(l.ts)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        Ok(crate::MessageBatchResult {
            affected_chat_ids: affected_chats.into_iter().collect(),
            active_chat_message_ids: active_inserted,
            new_message_ids_per_chat: new_per_chat,
        })
    }

    /// Páginação para trás: mensagens com `timestamp < before_ts` (ou todas
    /// as mais antigas se for None), em ordem ASC. Usado para virtualização
    /// na UI — quando o usuário scrolla pro topo, pedimos o próximo lote
    /// mais antigo.
    pub async fn get_message_rows_before(
        &self,
        account_id: &str,
        chat_id: &str,
        before_ts: i64,
        limit: i64,
    ) -> Result<Vec<MessageRow>> {
        let rows = sqlx::query_as::<_, MessageRow>(
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
               WHERE m.account_id = ? AND m.chat_id = ? AND m.timestamp < ?
               ORDER BY m.timestamp DESC
               LIMIT ?"#,
        )
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

    /// Aplica todos os contatos em UMA transação **com multi-row INSERT**.
    /// Antes: 4 statements/contato (lookup + maybe insert + update). Aqui:
    /// 1 SELECT bulk pra mapear aliases existentes + 1 INSERT…UPSERT chunked
    /// pros contatos + 1 INSERT…DO NOTHING chunked pros aliases.
    pub async fn run_contacts_batch(
        &self,
        account_id: &str,
        contacts: &[crate::ContactBatchInput<'_>],
    ) -> Result<Vec<String>> {
        use std::collections::HashMap;
        if contacts.is_empty() {
            return Ok(Vec::new());
        }

        // 1. Coleta todos os aliases (jid + lid) pra busca bulk.
        let mut all_aliases: Vec<String> = Vec::with_capacity(contacts.len() * 2);
        for c in contacts {
            all_aliases.push(c.jid.to_string());
            if let Some(l) = c.lid.filter(|l| *l != c.jid) {
                all_aliases.push(l.to_string());
            }
        }

        let mut tx = self.pool.begin().await?;

        // 2. Pré-fetch dos aliases já mapeados — 1 SELECT bulk em chunks
        //    pra respeitar SQLITE_MAX_VARIABLE_NUMBER.
        let mut existing: HashMap<String, String> = HashMap::new();
        const LOOKUP_CHUNK: usize = 500;
        for chunk in all_aliases.chunks(LOOKUP_CHUNK) {
            let placeholders = repeat_csv("?", chunk.len());
            let sql = format!(
                "SELECT alias_jid, contact_id FROM contact_aliases WHERE account_id = ? AND alias_jid IN ({})",
                placeholders
            );
            let mut q = sqlx::query(&sql).bind(account_id);
            for a in chunk {
                q = q.bind(a);
            }
            for row in q.fetch_all(&mut *tx).await? {
                let alias: String = row.get(0);
                let cid: String = row.get(1);
                existing.insert(alias, cid);
            }
        }

        // 3. Resolve contact_id por input — usa alias existente OU c.jid novo.
        let mut resolved_ids: Vec<String> = Vec::with_capacity(contacts.len());
        for c in contacts {
            let cid = existing
                .get(c.jid)
                .cloned()
                .or_else(|| c.lid.and_then(|l| existing.get(l).cloned()))
                .unwrap_or_else(|| c.jid.to_string());
            resolved_ids.push(cid);
        }

        // 4. Bulk UPSERT em `contacts` (chunks de 200 — 13 binds/row × 200 = 2.6k).
        const CONTACT_CHUNK: usize = 200;
        let now = now_ts();
        for (chunk_idx, chunk) in contacts.chunks(CONTACT_CHUNK).enumerate() {
            let row_tpl = "(?,?,?,?,?,?,?,?,?,?,?,?,?)";
            let mut sql = String::from(
                "INSERT INTO contacts (account_id, contact_id, pn_jid, lid_jid, phone_number, push_name, contact_name, verified_name, avatar_url, status, is_local, created_at, updated_at) VALUES ",
            );
            sql.push_str(&repeat_csv(row_tpl, chunk.len()));
            sql.push_str(
                r#" ON CONFLICT(account_id, contact_id) DO UPDATE SET
                    pn_jid = COALESCE(excluded.pn_jid, contacts.pn_jid),
                    lid_jid = COALESCE(excluded.lid_jid, contacts.lid_jid),
                    phone_number = COALESCE(excluded.phone_number, contacts.phone_number),
                    push_name = COALESCE(excluded.push_name, contacts.push_name),
                    contact_name = COALESCE(excluded.contact_name, contacts.contact_name),
                    verified_name = COALESCE(excluded.verified_name, contacts.verified_name),
                    avatar_url = COALESCE(excluded.avatar_url, contacts.avatar_url),
                    status = COALESCE(excluded.status, contacts.status),
                    updated_at = excluded.updated_at"#,
            );
            let mut q = sqlx::query(&sql);
            for (i, c) in chunk.iter().enumerate() {
                let cid = &resolved_ids[chunk_idx * CONTACT_CHUNK + i];
                let (pn, lid_jid) = derive_pn_lid(c.jid, c.lid);
                let phone_fallback = c.phone_number.map(|p| p.to_string()).or_else(|| {
                    pn.as_deref()
                        .and_then(|p| p.split('@').next().map(|u| u.to_string()))
                });
                q = q
                    .bind(account_id)
                    .bind(cid)
                    .bind(pn)
                    .bind(lid_jid)
                    .bind(phone_fallback)
                    .bind(c.push_name)
                    .bind(c.contact_name)
                    .bind(c.verified_name)
                    .bind(c.avatar_url)
                    .bind(c.status)
                    .bind(false) // is_local
                    .bind(now)
                    .bind(now);
            }
            q.execute(&mut *tx).await?;
        }

        // 5. Bulk INSERT em `contact_aliases` (chunks de 500).
        let mut alias_rows: Vec<(String, String)> = Vec::with_capacity(contacts.len() * 2);
        for (i, c) in contacts.iter().enumerate() {
            let cid = &resolved_ids[i];
            alias_rows.push((c.jid.to_string(), cid.clone()));
            if let Some(l) = c.lid.filter(|l| *l != c.jid) {
                alias_rows.push((l.to_string(), cid.clone()));
            }
        }
        const ALIAS_CHUNK: usize = 500;
        for chunk in alias_rows.chunks(ALIAS_CHUNK) {
            let mut sql = String::from(
                "INSERT INTO contact_aliases (account_id, alias_jid, contact_id) VALUES ",
            );
            sql.push_str(&repeat_csv("(?,?,?)", chunk.len()));
            sql.push_str(" ON CONFLICT(account_id, alias_jid) DO NOTHING");
            let mut q = sqlx::query(&sql);
            for (alias, cid) in chunk {
                q = q.bind(account_id).bind(alias).bind(cid);
            }
            q.execute(&mut *tx).await?;
        }

        tx.commit().await?;
        Ok(all_aliases)
    }

    /// Aplica grupos/newsletters em UMA transação **com multi-row INSERT**.
    /// Cada grupo gera operações em 5 tabelas (chats, chat_aliases, groups,
    /// contacts pra participantes, contact_aliases pra participantes).
    /// Tudo em statements bulk chunked.
    pub async fn run_groups_batch(
        &self,
        account_id: &str,
        groups: &[crate::GroupBatchInput<'_>],
    ) -> Result<Vec<String>> {
        if groups.is_empty() {
            return Ok(Vec::new());
        }
        let mut tx = self.pool.begin().await?;
        let now = now_ts();

        // 1. Bulk UPSERT em `chats` (chunks de 200 — 5 binds × 200 = 1k).
        const CHATS_CHUNK: usize = 200;
        for chunk in groups.chunks(CHATS_CHUNK) {
            let row_tpl = "(?,?,?,?,?)";
            let mut sql = String::from(
                "INSERT INTO chats (account_id, chat_id, kind, display_name, updated_at) VALUES ",
            );
            sql.push_str(&repeat_csv(row_tpl, chunk.len()));
            sql.push_str(
                r#" ON CONFLICT(account_id, chat_id) DO UPDATE SET
                    display_name = COALESCE(excluded.display_name, chats.display_name),
                    updated_at = excluded.updated_at"#,
            );
            let mut q = sqlx::query(&sql);
            for g in chunk {
                let kind = ChatKind::infer_from_jid(g.jid);
                q = q
                    .bind(account_id)
                    .bind(g.jid)
                    .bind(kind.as_str())
                    .bind(g.subject)
                    .bind(now);
            }
            q.execute(&mut *tx).await?;
        }

        // 2. Bulk INSERT em `chat_aliases` (self-aliases — chunks de 500).
        const CHAT_ALIAS_CHUNK: usize = 500;
        for chunk in groups.chunks(CHAT_ALIAS_CHUNK) {
            let mut sql =
                String::from("INSERT INTO chat_aliases (account_id, alias_jid, chat_id) VALUES ");
            sql.push_str(&repeat_csv("(?,?,?)", chunk.len()));
            sql.push_str(" ON CONFLICT(account_id, alias_jid) DO NOTHING");
            let mut q = sqlx::query(&sql);
            for g in chunk {
                q = q.bind(account_id).bind(g.jid).bind(g.jid);
            }
            q.execute(&mut *tx).await?;
        }

        // 3. Bulk UPSERT em `groups` (chunks de 200 — 6 binds × 200 = 1.2k).
        for chunk in groups.chunks(CHATS_CHUNK) {
            let row_tpl = "(?,?,?,?,?,?)";
            let mut sql = String::from(
                "INSERT INTO groups (account_id, chat_id, subject, owner_contact_id, description, participants_json) VALUES ",
            );
            sql.push_str(&repeat_csv(row_tpl, chunk.len()));
            sql.push_str(
                r#" ON CONFLICT(account_id, chat_id) DO UPDATE SET
                    subject = COALESCE(excluded.subject, subject),
                    owner_contact_id = COALESCE(excluded.owner_contact_id, owner_contact_id),
                    description = COALESCE(excluded.description, description),
                    participants_json = COALESCE(excluded.participants_json, participants_json)"#,
            );
            let mut q = sqlx::query(&sql);
            for g in chunk {
                q = q
                    .bind(account_id)
                    .bind(g.jid)
                    .bind(g.subject)
                    .bind(g.owner)
                    .bind(g.description)
                    .bind(g.participants_json);
            }
            q.execute(&mut *tx).await?;
        }

        // 4. Coleta todos os JIDs de participantes + owners pra criar contatos
        //    skeleton em bulk.
        let mut contact_jids: Vec<&str> = Vec::new();
        for g in groups {
            if let Some(o) = g.owner {
                contact_jids.push(o);
            }
            contact_jids.extend(g.participant_jids.iter().copied());
        }

        // Dedup. Preserva ordem só por estética.
        {
            use std::collections::HashSet;
            let mut seen: HashSet<&str> = HashSet::with_capacity(contact_jids.len());
            contact_jids.retain(|j| seen.insert(*j));
        }

        // 5. Bulk INSERT em `contacts` (skeleton — só identidade) chunks 200.
        const PART_CONTACT_CHUNK: usize = 200;
        for chunk in contact_jids.chunks(PART_CONTACT_CHUNK) {
            let row_tpl = "(?,?,?,?,?,?,?)";
            let mut sql = String::from(
                "INSERT INTO contacts (account_id, contact_id, pn_jid, lid_jid, phone_number, created_at, updated_at) VALUES ",
            );
            sql.push_str(&repeat_csv(row_tpl, chunk.len()));
            sql.push_str(" ON CONFLICT(account_id, contact_id) DO NOTHING");
            let mut q = sqlx::query(&sql);
            for jid in chunk {
                let (pn, lid) = derive_pn_lid(jid, None);
                let phone = pn
                    .as_deref()
                    .and_then(|p| p.split('@').next().map(|u| u.to_string()));
                q = q
                    .bind(account_id)
                    .bind(jid)
                    .bind(pn)
                    .bind(lid)
                    .bind(phone)
                    .bind(now)
                    .bind(now);
            }
            q.execute(&mut *tx).await?;
        }

        // 6. Bulk INSERT em `contact_aliases` (self-aliases pros participantes).
        for chunk in contact_jids.chunks(CHAT_ALIAS_CHUNK) {
            let mut sql = String::from(
                "INSERT INTO contact_aliases (account_id, alias_jid, contact_id) VALUES ",
            );
            sql.push_str(&repeat_csv("(?,?,?)", chunk.len()));
            sql.push_str(" ON CONFLICT(account_id, alias_jid) DO NOTHING");
            let mut q = sqlx::query(&sql);
            for jid in chunk {
                q = q.bind(account_id).bind(jid).bind(jid);
            }
            q.execute(&mut *tx).await?;
        }

        tx.commit().await?;
        Ok(groups.iter().map(|g| g.jid.to_string()).collect())
    }

    /// Bulk: dado um conjunto de aliases (PN/LID), retorna chat_ids dos DMs
    /// associados. Substitui N×SELECT por uma query só.
    pub async fn find_dm_chat_ids_for_aliases(
        &self,
        account_id: &str,
        aliases: &[&str],
    ) -> Result<Vec<String>> {
        if aliases.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat("?")
            .take(aliases.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            r#"SELECT DISTINCT c.chat_id
               FROM chats c
               JOIN chat_aliases a
                 ON a.account_id = c.account_id AND a.chat_id = c.chat_id
               WHERE c.account_id = ? AND c.kind = 'dm' AND a.alias_jid IN ({})"#,
            placeholders
        );
        let mut q = sqlx::query_scalar::<_, String>(&sql).bind(account_id);
        for a in aliases {
            q = q.bind(*a);
        }
        Ok(q.fetch_all(&self.pool).await?)
    }

    /// Insere mensagem já com chat_id/sender resolvidos. Retorna `true` se
    /// foi nova (ON CONFLICT DO NOTHING).
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

    /// Mensagens com nome de remetente resolvido (pra renderização). Ordem
    /// cronológica ascendente pra a UI mostrar do mais antigo pro mais novo.
    pub async fn get_message_rows_by_chat(
        &self,
        account_id: &str,
        chat_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<MessageRow>> {
        let rows = sqlx::query_as::<_, MessageRow>(
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
               WHERE m.account_id = ? AND m.chat_id = ?
               ORDER BY m.timestamp DESC
               LIMIT ? OFFSET ?"#,
        )
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

    pub async fn get_message_rows_by_ids(
        &self,
        account_id: &str,
        message_ids: &[String],
    ) -> Result<Vec<MessageRow>> {
        if message_ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat("?")
            .take(message_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
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
               WHERE m.account_id = ? AND m.message_id IN ({})
               ORDER BY m.timestamp ASC"#,
            placeholders
        );
        let mut q = sqlx::query_as::<_, MessageRow>(&sql).bind(account_id);
        for id in message_ids {
            q = q.bind(id);
        }
        Ok(q.fetch_all(&self.pool).await?)
    }

    /// Marca o status de mídia de uma mensagem (e opcionalmente seu path).
    /// Devolve o `raw_json` persistido pela coluna correspondente. Usado
    /// pelo download tardio: se a cache in-memory do nanachi não tem o
    /// proto da mensagem, o Rust passa este JSON na IPC `DownloadMedia`
    /// e o Go re-hidrata o `*waE2E.Message` antes de chamar
    /// `whatsmeow.Download`.
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

    /// Persiste path + marca como done. Se um sha256 for fornecido, propaga
    /// o path para todas as mensagens com aquele hash (cache dedup).
    /// Retorna a lista de message_ids realmente atualizados (incluindo o
    /// originador).
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

    /// Lookup auxiliar pra dedup pré-download: dado um sha256, devolve o
    /// path se já existe alguma cópia em outra mensagem.
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
    /// (se a entidade for um chat de grupo/canal) quanto `contacts` (DM)
    /// — o resolver da chat list via JOIN ainda funciona em ambos.
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

// ============================================================================
// Resolver internals (transaction-scoped)
// ============================================================================

/// Resolve um JID para um chat_id, criando o chat e o alias se necessário.
async fn register_chat_alias_tx(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    alias_jid: &str,
    kind: ChatKind,
) -> Result<String> {
    if let Some(existing) = lookup_alias(&mut **tx, account_id, alias_jid, true).await? {
        return Ok(existing);
    }
    // Cria chat com chat_id = alias_jid (forma "primária" de referência).
    sqlx::query(
        r#"INSERT INTO chats (account_id, chat_id, kind) VALUES (?, ?, ?)
           ON CONFLICT(account_id, chat_id) DO NOTHING"#,
    )
    .bind(account_id)
    .bind(alias_jid)
    .bind(kind.as_str())
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO chat_aliases (account_id, alias_jid, chat_id) VALUES (?, ?, ?)
         ON CONFLICT(account_id, alias_jid) DO NOTHING",
    )
    .bind(account_id)
    .bind(alias_jid)
    .bind(alias_jid)
    .execute(&mut **tx)
    .await?;
    Ok(alias_jid.to_string())
}

async fn register_contact_alias_tx(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    alias_jid: &str,
) -> Result<String> {
    if let Some(existing) = lookup_alias(&mut **tx, account_id, alias_jid, false).await? {
        return Ok(existing);
    }
    sqlx::query(
        r#"INSERT INTO contacts (account_id, contact_id) VALUES (?, ?)
           ON CONFLICT(account_id, contact_id) DO NOTHING"#,
    )
    .bind(account_id)
    .bind(alias_jid)
    .execute(&mut **tx)
    .await?;
    // Pré-popula pn_jid/lid_jid baseado no server.
    let server = alias_jid.rsplit_once('@').map(|(_, s)| s).unwrap_or("");
    match server {
        "lid" => {
            sqlx::query(
                "UPDATE contacts SET lid_jid = COALESCE(lid_jid, ?) WHERE account_id = ? AND contact_id = ?",
            )
            .bind(alias_jid)
            .bind(account_id)
            .bind(alias_jid)
            .execute(&mut **tx)
            .await?;
        }
        "s.whatsapp.net" | "c.us" | "hosted" => {
            let phone = alias_jid.split('@').next().unwrap_or("");
            sqlx::query(
                "UPDATE contacts SET pn_jid = COALESCE(pn_jid, ?), phone_number = COALESCE(phone_number, ?) WHERE account_id = ? AND contact_id = ?",
            )
            .bind(alias_jid)
            .bind(phone)
            .bind(account_id)
            .bind(alias_jid)
            .execute(&mut **tx)
            .await?;
        }
        _ => {}
    }
    sqlx::query(
        "INSERT INTO contact_aliases (account_id, alias_jid, contact_id) VALUES (?, ?, ?)
         ON CONFLICT(account_id, alias_jid) DO NOTHING",
    )
    .bind(account_id)
    .bind(alias_jid)
    .bind(alias_jid)
    .execute(&mut **tx)
    .await?;
    Ok(alias_jid.to_string())
}

async fn lookup_alias<'e, E>(
    executor: E,
    account_id: &str,
    alias_jid: &str,
    is_chat: bool,
) -> Result<Option<String>>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let table = if is_chat {
        "chat_aliases"
    } else {
        "contact_aliases"
    };
    let col = if is_chat { "chat_id" } else { "contact_id" };
    let sql = format!("SELECT {col} FROM {table} WHERE account_id = ? AND alias_jid = ?");
    let row = sqlx::query(&sql)
        .bind(account_id)
        .bind(alias_jid)
        .fetch_optional(executor)
        .await?;
    Ok(row.map(|r| r.get::<String, _>(0)))
}

/// Liga `alias_jid` ao `winner_id`, mesclando o chat/contact existente caso
/// já estivesse apontando pra outro id.
async fn link_alias_tx(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    alias_jid: &str,
    winner_id: &str,
    kind: ChatKind,
    is_chat: bool,
) -> Result<()> {
    let existing = lookup_alias(&mut **tx, account_id, alias_jid, is_chat).await?;
    match existing {
        Some(ref id) if id == winner_id => Ok(()),
        Some(loser_id) => {
            // Mescla loser → winner.
            if is_chat {
                merge_chats_tx(tx, account_id, winner_id, &loser_id).await
            } else {
                merge_contacts_tx(tx, account_id, winner_id, &loser_id).await
            }
        }
        None => {
            // Garante que o registro do winner já existe (defensivo).
            if is_chat {
                sqlx::query(
                    r#"INSERT INTO chats (account_id, chat_id, kind) VALUES (?, ?, ?)
                       ON CONFLICT(account_id, chat_id) DO NOTHING"#,
                )
                .bind(account_id)
                .bind(winner_id)
                .bind(kind.as_str())
                .execute(&mut **tx)
                .await?;
                sqlx::query(
                    "INSERT INTO chat_aliases (account_id, alias_jid, chat_id) VALUES (?, ?, ?)
                     ON CONFLICT(account_id, alias_jid) DO NOTHING",
                )
                .bind(account_id)
                .bind(alias_jid)
                .bind(winner_id)
                .execute(&mut **tx)
                .await?;
            } else {
                sqlx::query(
                    r#"INSERT INTO contacts (account_id, contact_id) VALUES (?, ?)
                       ON CONFLICT(account_id, contact_id) DO NOTHING"#,
                )
                .bind(account_id)
                .bind(winner_id)
                .execute(&mut **tx)
                .await?;
                sqlx::query(
                    "INSERT INTO contact_aliases (account_id, alias_jid, contact_id) VALUES (?, ?, ?)
                     ON CONFLICT(account_id, alias_jid) DO NOTHING",
                )
                .bind(account_id)
                .bind(alias_jid)
                .bind(winner_id)
                .execute(&mut **tx)
                .await?;
                // Atualiza pn_jid/lid_jid do winner com base no novo alias.
                let server = alias_jid.rsplit_once('@').map(|(_, s)| s).unwrap_or("");
                match server {
                    "lid" => {
                        sqlx::query(
                            "UPDATE contacts SET lid_jid = COALESCE(lid_jid, ?) WHERE account_id = ? AND contact_id = ?",
                        )
                        .bind(alias_jid)
                        .bind(account_id)
                        .bind(winner_id)
                        .execute(&mut **tx)
                        .await?;
                    }
                    "s.whatsapp.net" | "c.us" | "hosted" => {
                        let phone = alias_jid.split('@').next().unwrap_or("");
                        sqlx::query(
                            "UPDATE contacts SET pn_jid = COALESCE(pn_jid, ?), phone_number = COALESCE(phone_number, ?) WHERE account_id = ? AND contact_id = ?",
                        )
                        .bind(alias_jid)
                        .bind(phone)
                        .bind(account_id)
                        .bind(winner_id)
                        .execute(&mut **tx)
                        .await?;
                    }
                    _ => {}
                }
            }
            Ok(())
        }
    }
}

async fn merge_chats_tx(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    winner: &str,
    loser: &str,
) -> Result<()> {
    sqlx::query("UPDATE chat_aliases SET chat_id = ? WHERE account_id = ? AND chat_id = ?")
        .bind(winner)
        .bind(account_id)
        .bind(loser)
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE messages SET chat_id = ? WHERE account_id = ? AND chat_id = ?")
        .bind(winner)
        .bind(account_id)
        .bind(loser)
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE groups SET chat_id = ? WHERE account_id = ? AND chat_id = ?")
        .bind(winner)
        .bind(account_id)
        .bind(loser)
        .execute(&mut **tx)
        .await?;
    // Reaproveita campos do loser que estejam faltando no winner. Todos
    // os campos `last_message_*` precisam coerir COMO BLOCO: se o loser
    // tem a mensagem mais nova, todos passam pro loser; senão todos
    // ficam do winner. Antes a gente atualizava só id/preview/ts, e
    // `from_me` / `sender_contact_id` / `type` ficavam do winner — a
    // sidebar mostrava preview do loser com flag `Você:` do winner.
    sqlx::query(
        r#"UPDATE chats SET
            display_name = COALESCE(display_name, (SELECT display_name FROM chats WHERE account_id = ?1 AND chat_id = ?2)),
            avatar_url = COALESCE(avatar_url, (SELECT avatar_url FROM chats WHERE account_id = ?1 AND chat_id = ?2)),
            last_message_id = CASE WHEN COALESCE(last_message_ts,0) >= COALESCE((SELECT last_message_ts FROM chats WHERE account_id = ?1 AND chat_id = ?2), 0) THEN last_message_id ELSE (SELECT last_message_id FROM chats WHERE account_id = ?1 AND chat_id = ?2) END,
            last_message_preview = CASE WHEN COALESCE(last_message_ts,0) >= COALESCE((SELECT last_message_ts FROM chats WHERE account_id = ?1 AND chat_id = ?2), 0) THEN last_message_preview ELSE (SELECT last_message_preview FROM chats WHERE account_id = ?1 AND chat_id = ?2) END,
            last_message_from_me = CASE WHEN COALESCE(last_message_ts,0) >= COALESCE((SELECT last_message_ts FROM chats WHERE account_id = ?1 AND chat_id = ?2), 0) THEN last_message_from_me ELSE COALESCE((SELECT last_message_from_me FROM chats WHERE account_id = ?1 AND chat_id = ?2), 0) END,
            last_sender_contact_id = CASE WHEN COALESCE(last_message_ts,0) >= COALESCE((SELECT last_message_ts FROM chats WHERE account_id = ?1 AND chat_id = ?2), 0) THEN last_sender_contact_id ELSE (SELECT last_sender_contact_id FROM chats WHERE account_id = ?1 AND chat_id = ?2) END,
            last_message_type = CASE WHEN COALESCE(last_message_ts,0) >= COALESCE((SELECT last_message_ts FROM chats WHERE account_id = ?1 AND chat_id = ?2), 0) THEN last_message_type ELSE (SELECT last_message_type FROM chats WHERE account_id = ?1 AND chat_id = ?2) END,
            last_message_duration_secs = CASE WHEN COALESCE(last_message_ts,0) >= COALESCE((SELECT last_message_ts FROM chats WHERE account_id = ?1 AND chat_id = ?2), 0) THEN last_message_duration_secs ELSE (SELECT last_message_duration_secs FROM chats WHERE account_id = ?1 AND chat_id = ?2) END,
            last_message_ts = MAX(COALESCE(last_message_ts,0), COALESCE((SELECT last_message_ts FROM chats WHERE account_id = ?1 AND chat_id = ?2), 0)),
            unread_count = unread_count + COALESCE((SELECT unread_count FROM chats WHERE account_id = ?1 AND chat_id = ?2), 0)
           WHERE account_id = ?1 AND chat_id = ?3"#,
    )
    .bind(account_id)
    .bind(loser)
    .bind(winner)
    .execute(&mut **tx)
    .await?;
    sqlx::query("DELETE FROM chats WHERE account_id = ? AND chat_id = ?")
        .bind(account_id)
        .bind(loser)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn merge_contacts_tx(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    winner: &str,
    loser: &str,
) -> Result<()> {
    // Reaponta aliases.
    sqlx::query(
        "UPDATE contact_aliases SET contact_id = ? WHERE account_id = ? AND contact_id = ?",
    )
    .bind(winner)
    .bind(account_id)
    .bind(loser)
    .execute(&mut **tx)
    .await?;
    // Reaponta sender de mensagens.
    sqlx::query(
        "UPDATE messages SET sender_contact_id = ? WHERE account_id = ? AND sender_contact_id = ?",
    )
    .bind(winner)
    .bind(account_id)
    .bind(loser)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "UPDATE chats SET last_sender_contact_id = ? WHERE account_id = ? AND last_sender_contact_id = ?",
    )
    .bind(winner)
    .bind(account_id)
    .bind(loser)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "UPDATE groups SET owner_contact_id = ? WHERE account_id = ? AND owner_contact_id = ?",
    )
    .bind(winner)
    .bind(account_id)
    .bind(loser)
    .execute(&mut **tx)
    .await?;
    // Mescla campos do contato (o que o winner não tem, herda do loser).
    sqlx::query(
        r#"UPDATE contacts SET
            pn_jid = COALESCE(pn_jid, (SELECT pn_jid FROM contacts WHERE account_id = ?1 AND contact_id = ?2)),
            lid_jid = COALESCE(lid_jid, (SELECT lid_jid FROM contacts WHERE account_id = ?1 AND contact_id = ?2)),
            phone_number = COALESCE(phone_number, (SELECT phone_number FROM contacts WHERE account_id = ?1 AND contact_id = ?2)),
            push_name = COALESCE(push_name, (SELECT push_name FROM contacts WHERE account_id = ?1 AND contact_id = ?2)),
            contact_name = COALESCE(contact_name, (SELECT contact_name FROM contacts WHERE account_id = ?1 AND contact_id = ?2)),
            business_name = COALESCE(business_name, (SELECT business_name FROM contacts WHERE account_id = ?1 AND contact_id = ?2)),
            verified_name = COALESCE(verified_name, (SELECT verified_name FROM contacts WHERE account_id = ?1 AND contact_id = ?2)),
            avatar_url = COALESCE(avatar_url, (SELECT avatar_url FROM contacts WHERE account_id = ?1 AND contact_id = ?2)),
            status = COALESCE(status, (SELECT status FROM contacts WHERE account_id = ?1 AND contact_id = ?2)),
            updated_at = ?4
           WHERE account_id = ?1 AND contact_id = ?3"#,
    )
    .bind(account_id)
    .bind(loser)
    .bind(winner)
    .bind(now_ts())
    .execute(&mut **tx)
    .await?;
    sqlx::query("DELETE FROM contacts WHERE account_id = ? AND contact_id = ?")
        .bind(account_id)
        .bind(loser)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

fn chat_row_select_clause(filter_by_ids: bool) -> String {
    let where_clause = if filter_by_ids {
        "WHERE c.account_id = ? AND c.chat_id IN (__IDS__)".to_string()
    } else {
        "WHERE c.account_id = ?".to_string()
    };
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
            c.pinned
           FROM chats c
           LEFT JOIN contact_aliases ca
                  ON ca.account_id = c.account_id AND ca.alias_jid = c.chat_id
           LEFT JOIN contacts ct
                  ON ct.account_id = c.account_id AND ct.contact_id = ca.contact_id
           {where_clause}
           ORDER BY c.last_message_ts DESC NULLS LAST, c.updated_at DESC"#,
    )
}

/// "?,?,?" repetido N vezes (com vírgulas) — pra placeholders dinâmicos.
fn repeat_csv(token: &str, n: usize) -> String {
    let mut s = String::with_capacity(token.len() * n + n);
    for i in 0..n {
        if i > 0 {
            s.push(',');
        }
        s.push_str(token);
    }
    s
}

/// Deriva (pn_jid, lid_jid) a partir do par (jid, alt_lid) usando o server.
fn server_of(j: &str) -> &str {
    j.rsplit_once('@').map(|(_, s)| s).unwrap_or("")
}

fn derive_pn_lid(jid: &str, alt: Option<&str>) -> (Option<String>, Option<String>) {
    let mut pn = None;
    let mut lid = None;
    match server_of(jid) {
        "lid" => lid = Some(jid.to_string()),
        "s.whatsapp.net" | "c.us" | "hosted" => pn = Some(jid.to_string()),
        _ => {}
    }
    if let Some(a) = alt {
        match server_of(a) {
            "lid" if lid.is_none() => lid = Some(a.to_string()),
            "s.whatsapp.net" | "c.us" | "hosted" if pn.is_none() => pn = Some(a.to_string()),
            _ => {}
        }
    }
    (pn, lid)
}

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
