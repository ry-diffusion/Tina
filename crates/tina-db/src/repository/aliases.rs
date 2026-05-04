// Resolver internals: shared between the chats and contacts modules.
// Walks the alias tables, registers new ones, and merges entries when
// two aliases collide on the same primary id.

use sqlx::{Row, Sqlite, Transaction};

use crate::error::Result;
use crate::models::ChatKind;

pub(super) async fn lookup_alias<'e, E>(
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

/// Resolve um JID para um chat_id, criando o chat e o alias se necessário.
pub(super) async fn register_chat_alias_tx(
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

pub(super) async fn register_contact_alias_tx(
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
    backfill_contact_identity(tx, account_id, alias_jid).await?;
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

async fn backfill_contact_identity(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    contact_id: &str,
) -> Result<()> {
    let server = contact_id.rsplit_once('@').map(|(_, s)| s).unwrap_or("");
    match server {
        "lid" => {
            sqlx::query(
                "UPDATE contacts SET lid_jid = COALESCE(lid_jid, ?) WHERE account_id = ? AND contact_id = ?",
            )
            .bind(contact_id)
            .bind(account_id)
            .bind(contact_id)
            .execute(&mut **tx)
            .await?;
        }
        "s.whatsapp.net" | "c.us" | "hosted" => {
            let phone = contact_id.split('@').next().unwrap_or("");
            sqlx::query(
                "UPDATE contacts SET pn_jid = COALESCE(pn_jid, ?), phone_number = COALESCE(phone_number, ?) WHERE account_id = ? AND contact_id = ?",
            )
            .bind(contact_id)
            .bind(phone)
            .bind(account_id)
            .bind(contact_id)
            .execute(&mut **tx)
            .await?;
        }
        _ => {}
    }
    Ok(())
}

/// Liga `alias_jid` ao `winner_id`, mesclando o chat/contact existente
/// caso já estivesse apontando pra outro id.
pub(super) async fn link_alias_tx(
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
            if is_chat {
                super::merge::merge_chats_tx(tx, account_id, winner_id, &loser_id).await
            } else {
                super::merge::merge_contacts_tx(tx, account_id, winner_id, &loser_id).await
            }
        }
        None => attach_new_alias(tx, account_id, alias_jid, winner_id, kind, is_chat).await,
    }
}

async fn attach_new_alias(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    alias_jid: &str,
    winner_id: &str,
    kind: ChatKind,
    is_chat: bool,
) -> Result<()> {
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
        backfill_winner_identity(tx, account_id, alias_jid, winner_id).await?;
    }
    Ok(())
}

async fn backfill_winner_identity(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    alias_jid: &str,
    winner_id: &str,
) -> Result<()> {
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
    Ok(())
}
