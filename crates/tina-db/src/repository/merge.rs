// Alias collision resolution: when two aliases point at different
// chat/contact ids, merge the loser into the winner so all metadata
// (last message, avatars, etc) lands on a single record.

use sqlx::{Sqlite, Transaction};

use crate::error::Result;

use super::util::now_ts;

pub(super) async fn merge_chats_tx(
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

pub(super) async fn merge_contacts_tx(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    winner: &str,
    loser: &str,
) -> Result<()> {
    repoint_loser_references(tx, account_id, winner, loser).await?;
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

async fn repoint_loser_references(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    winner: &str,
    loser: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE contact_aliases SET contact_id = ? WHERE account_id = ? AND contact_id = ?",
    )
    .bind(winner)
    .bind(account_id)
    .bind(loser)
    .execute(&mut **tx)
    .await?;
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
    Ok(())
}
