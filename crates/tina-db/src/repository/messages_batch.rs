// Bulk message insertion: resolves chat + sender per row, inserts via
// multi-row INSERT, aggregates the per-chat last-message UPDATE.

use std::collections::{HashMap, HashSet};

use sqlx::{Row, Sqlite, Transaction};

use crate::error::Result;
use crate::models::ChatKind;

use super::aliases::{register_chat_alias_tx, register_contact_alias_tx};
use super::db::TinaDb;
use super::util::{now_ts, repeat_csv};

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

struct PendingInsert {
    idx: usize,
    chat_id: String,
    sender_contact_id: Option<String>,
}

impl TinaDb {
    /// Processa um lote de mensagens numa única transação. Resolve chat
    /// e sender (deduplicados em memória dentro do batch), insere todas,
    /// e agrega `update_chat_last_message` para emitir um UPDATE por
    /// chat afetado em vez de um por mensagem.
    pub async fn run_message_batch(
        &self,
        account_id: &str,
        active_chat: Option<&str>,
        messages: &[crate::MessageBatchInput<'_>],
    ) -> Result<crate::MessageBatchResult> {
        let mut tx = self.pool.begin().await?;

        let (pending, latest) = resolve_chats_and_senders(&mut tx, account_id, messages).await?;
        let existing_ids = lookup_existing_message_ids(&mut tx, account_id, messages).await?;

        bulk_insert_messages(&mut tx, account_id, messages, &pending).await?;

        let (affected_chats, active_inserted, new_per_chat) =
            tally_inserted(messages, &pending, &existing_ids, active_chat);

        flush_chat_last_message(&mut tx, account_id, &latest).await?;

        tx.commit().await?;

        Ok(crate::MessageBatchResult {
            affected_chat_ids: affected_chats.into_iter().collect(),
            active_chat_message_ids: active_inserted,
            new_message_ids_per_chat: new_per_chat,
        })
    }
}

async fn resolve_chats_and_senders<'a>(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    messages: &'a [crate::MessageBatchInput<'a>],
) -> Result<(Vec<PendingInsert>, HashMap<String, Latest<'a>>)> {
    let mut chat_cache: HashMap<&str, String> = HashMap::new();
    let mut contact_cache: HashMap<&str, String> = HashMap::new();
    let mut latest: HashMap<String, Latest<'_>> = HashMap::new();
    let mut pending: Vec<PendingInsert> = Vec::with_capacity(messages.len());

    for (idx, msg) in messages.iter().enumerate() {
        let chat_id = if let Some(c) = chat_cache.get(msg.chat_jid) {
            c.clone()
        } else {
            let kind = ChatKind::infer_from_jid(msg.chat_jid);
            let id = register_chat_alias_tx(tx, account_id, msg.chat_jid, kind).await?;
            chat_cache.insert(msg.chat_jid, id.clone());
            id
        };

        let sender_contact_id: Option<String> = if msg.is_from_me {
            None
        } else if let Some(s) = msg.sender_jid {
            if s.is_empty() {
                None
            } else if let Some(c) = contact_cache.get(s) {
                Some(c.clone())
            } else {
                let id = register_contact_alias_tx(tx, account_id, s).await?;
                contact_cache.insert(s, id.clone());
                Some(id)
            }
        } else {
            None
        };

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

        update_latest(
            &mut latest,
            chat_id.clone(),
            msg,
            placeholder,
            sender_contact_id.clone(),
        );

        pending.push(PendingInsert {
            idx,
            chat_id,
            sender_contact_id,
        });
    }
    Ok((pending, latest))
}

fn update_latest<'a>(
    latest: &mut HashMap<String, Latest<'a>>,
    chat_id: String,
    msg: &crate::MessageBatchInput<'a>,
    placeholder: &'static str,
    sender_contact_id: Option<String>,
) {
    use std::collections::hash_map::Entry;
    match latest.entry(chat_id) {
        Entry::Vacant(v) => {
            v.insert(Latest {
                ts: msg.timestamp,
                message_id: msg.message_id,
                preview: msg.content,
                placeholder,
                from_me: msg.is_from_me,
                sender_contact_id,
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
                    sender_contact_id,
                    message_type: msg.message_type,
                    duration_secs: msg.media_duration_secs,
                });
            }
        }
    }
}

async fn lookup_existing_message_ids<'a>(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    messages: &'a [crate::MessageBatchInput<'a>],
) -> Result<HashSet<&'a str>> {
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
        for row in q.fetch_all(&mut **tx).await? {
            let id_str: String = row.get(0);
            if let Some(s) = all_ids.iter().find(|x| **x == id_str.as_str()) {
                existing_ids.insert(*s);
            }
        }
    }
    Ok(existing_ids)
}

async fn bulk_insert_messages(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    messages: &[crate::MessageBatchInput<'_>],
    pending: &[PendingInsert],
) -> Result<()> {
    const MSG_INSERT_CHUNK: usize = 200;
    let now = now_ts();
    for chunk in pending.chunks(MSG_INSERT_CHUNK) {
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
        q.execute(&mut **tx).await?;
    }
    Ok(())
}

fn tally_inserted(
    messages: &[crate::MessageBatchInput<'_>],
    pending: &[PendingInsert],
    existing_ids: &HashSet<&str>,
    active_chat: Option<&str>,
) -> (HashSet<String>, Vec<String>, HashMap<String, Vec<String>>) {
    let mut affected_chats: HashSet<String> = HashSet::new();
    let mut active_inserted: Vec<String> = Vec::new();
    let mut new_per_chat: HashMap<String, Vec<String>> = HashMap::new();

    for p in pending {
        let m = &messages[p.idx];
        if existing_ids.contains(&m.message_id) {
            continue;
        }
        affected_chats.insert(p.chat_id.clone());
        new_per_chat
            .entry(p.chat_id.clone())
            .or_default()
            .push(m.message_id.to_string());
        if let Some(active) = active_chat
            && active == p.chat_id {
                active_inserted.push(m.message_id.to_string());
            }
    }
    (affected_chats, active_inserted, new_per_chat)
}

async fn flush_chat_last_message(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    latest: &HashMap<String, Latest<'_>>,
) -> Result<()> {
    for (chat_id, l) in latest {
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
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}
