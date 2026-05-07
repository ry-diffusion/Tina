// Flush: applies the entire `DirtyBuffer` as a single transaction
// per account, then emits exactly one `ChatsUpserted` per affected
// account.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{RwLock, mpsc};

use tina_db::TinaDb;
use tina_ipc::SLOW_IPC_THRESHOLD;

use crate::error::Result;
use crate::events::WorkerEvent;

use super::batch::{process_contacts, process_groups};
use super::buffer::DirtyBuffer;

/// Aplica todo o buffer numa transação coletiva por account_id,
/// emitindo um único `ChatsUpserted` por account no final.
pub(super) async fn flush(
    db: &TinaDb,
    event_tx: &mpsc::Sender<WorkerEvent>,
    open_chats: &Arc<RwLock<HashMap<String, HashSet<String>>>>,
    buffer: &mut DirtyBuffer,
) -> Result<()> {
    let started = Instant::now();
    let count_msgs: usize = buffer.messages.values().map(|v| v.len()).sum();
    let count_contacts: usize = buffer.contacts.values().map(|v| v.len()).sum();
    let count_groups: usize = buffer.groups.values().map(|v| v.len()).sum();

    let mut affected: HashMap<String, HashSet<String>> = HashMap::new();
    let mut msgs_per_account: HashMap<String, usize> = HashMap::new();
    let open_snapshot = open_chats.read().await.clone();

    flush_messages(db, event_tx, buffer, &mut affected, &open_snapshot, &mut msgs_per_account).await?;
    flush_contacts(db, buffer, &mut affected).await?;
    flush_groups(db, buffer, &mut affected).await?;
    emit_chats_upserted(db, event_tx, affected, msgs_per_account).await;

    log_flush_duration(started.elapsed(), count_msgs, count_contacts, count_groups);
    Ok(())
}

async fn flush_messages(
    db: &TinaDb,
    event_tx: &mpsc::Sender<WorkerEvent>,
    buffer: &mut DirtyBuffer,
    affected: &mut HashMap<String, HashSet<String>>,
    open_snapshot: &HashMap<String, HashSet<String>>,
    msgs_per_account: &mut HashMap<String, usize>,
) -> Result<()> {
    let messages = std::mem::take(&mut buffer.messages);
    for (account_id, msgs) in messages {
        let open_for_account = open_snapshot.get(&account_id);
        *msgs_per_account.entry(account_id.clone()).or_default() += msgs.len();

        // Pre-render the per-row JSON for mentioned JIDs so the
        // borrowed string lives long enough for the batch input.
        let mentions_storage: Vec<Option<String>> = msgs
            .iter()
            .map(|m| {
                if m.mentioned_jids.is_empty() {
                    None
                } else {
                    let raws: Vec<&str> = m.mentioned_jids.iter().map(|j| j.raw()).collect();
                    serde_json::to_string(&raws).ok()
                }
            })
            .collect();

        let inputs: Vec<tina_db::MessageBatchInput<'_>> = msgs
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let has_media = m.media_mimetype.is_some()
                    || m.media_filename.is_some()
                    || m.media_duration_secs.is_some()
                    || m.media_width.is_some()
                    || m.media_height.is_some()
                    || m.media_size_bytes.is_some()
                    || m.media_sha256.is_some()
                    || m.thumbnail.is_some();
                let media = has_media.then_some(tina_db::MediaMeta {
                    mimetype: m.media_mimetype.as_deref(),
                    filename: m.media_filename.as_deref(),
                    duration_secs: m.media_duration_secs,
                    width: m.media_width,
                    height: m.media_height,
                    size_bytes: m.media_size_bytes,
                    sha256: m.media_sha256.as_deref(),
                    thumbnail: m.thumbnail.as_deref(),
                });
                tina_db::MessageBatchInput {
                    message_id: &m.message_id,
                    chat_jid: m.chat_jid.raw(),
                    sender_jid: if m.sender_jid.is_empty_unknown() {
                        None
                    } else {
                        Some(m.sender_jid.raw())
                    },
                    content: m.content.as_deref(),
                    message_type: &m.message_type,
                    timestamp: m.timestamp,
                    is_from_me: m.is_from_me,
                    raw_json: m.raw_json.as_deref(),
                    media,
                    quoted_message_id: m.quoted_message_id.as_deref(),
                    quoted_sender_id: m.quoted_sender_id.as_ref().map(|x| x.raw()),
                    quoted_preview: m.quoted_preview.as_deref(),
                    mentions_json: mentions_storage[i].as_deref(),
                }
            })
            .collect();

        let res = db.run_message_batch(&account_id, None, &inputs).await?;

        affected
            .entry(account_id.clone())
            .or_default()
            .extend(res.affected_chat_ids);

        // Emit MessagesAppended SOMENTE para chats abertos como tab.
        // Durante history sync, dezenas de chats fechados recebem rows
        // novas — emitir para todos enchia o canal e fazia a UI travar
        // mesmo descartando do outro lado. O snapshot de chats já chega
        // via ChatsUpserted; chats fechados re-carregam via OpenChat
        // quando o usuário abrir a tab.
        let Some(open_set) = open_for_account else {
            let _ = res.active_chat_message_ids;
            continue;
        };
        for (chat_id, msg_ids) in res.new_message_ids_per_chat {
            if msg_ids.is_empty() || !open_set.contains(&chat_id) {
                continue;
            }
            let rows = db.get_message_rows_by_ids(&account_id, &msg_ids).await?;
            if !rows.is_empty() {
                let _ = event_tx
                    .send(WorkerEvent::MessagesAppended {
                        account_id: account_id.clone(),
                        chat_id,
                        messages: rows,
                    })
                    .await;
            }
        }
        let _ = res.active_chat_message_ids;
    }
    Ok(())
}

async fn flush_contacts(
    db: &TinaDb,
    buffer: &mut DirtyBuffer,
    affected: &mut HashMap<String, HashSet<String>>,
) -> Result<()> {
    let contacts = std::mem::take(&mut buffer.contacts);
    for (account_id, list) in contacts {
        let chat_ids = process_contacts(db, &account_id, list).await?;
        affected
            .entry(account_id.clone())
            .or_default()
            .extend(chat_ids);
    }
    Ok(())
}

async fn flush_groups(
    db: &TinaDb,
    buffer: &mut DirtyBuffer,
    affected: &mut HashMap<String, HashSet<String>>,
) -> Result<()> {
    let groups = std::mem::take(&mut buffer.groups);
    for (account_id, list) in groups {
        let chat_ids = process_groups(db, &account_id, list).await?;
        affected
            .entry(account_id.clone())
            .or_default()
            .extend(chat_ids);
    }
    Ok(())
}

async fn emit_chats_upserted(
    db: &TinaDb,
    event_tx: &mpsc::Sender<WorkerEvent>,
    affected: HashMap<String, HashSet<String>>,
    msgs_per_account: HashMap<String, usize>,
) {
    for (account_id, chat_ids) in affected {
        if chat_ids.is_empty() {
            continue;
        }
        let messages_written = msgs_per_account.get(&account_id).copied().unwrap_or(0);
        let ids: Vec<String> = chat_ids.into_iter().collect();
        match db.get_chat_rows(&account_id, &ids).await {
            Ok(rows) if !rows.is_empty() => {
                tracing::debug!(
                    "🔄 flush → {} chat(s) emitidos para {}",
                    rows.len(),
                    account_id
                );
                let _ = event_tx
                    .send(WorkerEvent::ChatsUpserted {
                        account_id,
                        rows,
                        messages_written,
                    })
                    .await;
            }
            Ok(_) => {}
            Err(e) => tracing::error!("get_chat_rows failed: {}", e),
        }
    }
}

fn log_flush_duration(
    elapsed: Duration,
    count_msgs: usize,
    count_contacts: usize,
    count_groups: usize,
) {
    // Flush é trabalho pesado; threshold mais frouxo que o resto do IPC.
    const SLOW_FLUSH_THRESHOLD: Duration = Duration::from_millis(200);
    if elapsed > SLOW_FLUSH_THRESHOLD {
        tracing::warn!(
            "🐌 flush levou {:?} (msgs={}, contacts={}, groups={})",
            elapsed,
            count_msgs,
            count_contacts,
            count_groups
        );
    } else if elapsed > SLOW_IPC_THRESHOLD {
        tracing::debug!(
            "flush {:?} (msgs={}, contacts={}, groups={})",
            elapsed,
            count_msgs,
            count_contacts,
            count_groups
        );
    }
}
