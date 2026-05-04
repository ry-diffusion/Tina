// Groups + newsletters: single upsert + the bulk batch.

use std::collections::HashSet;

use crate::error::Result;
use crate::models::ChatKind;

use super::db::TinaDb;
use super::util::{derive_pn_lid, now_ts, repeat_csv};

impl TinaDb {
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

        upsert_chats_for_groups(&mut tx, account_id, groups, now).await?;
        insert_chat_self_aliases(&mut tx, account_id, groups).await?;
        upsert_groups_table(&mut tx, account_id, groups).await?;
        upsert_participant_contacts(&mut tx, account_id, groups, now).await?;

        tx.commit().await?;
        Ok(groups.iter().map(|g| g.jid.to_string()).collect())
    }

    /// Bulk: dado um conjunto de aliases (PN/LID), retorna chat_ids dos
    /// DMs associados. Substitui N×SELECT por uma query só.
    pub async fn find_dm_chat_ids_for_aliases(
        &self,
        account_id: &str,
        aliases: &[&str],
    ) -> Result<Vec<String>> {
        if aliases.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = repeat_csv("?", aliases.len());
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
}

async fn upsert_chats_for_groups(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    account_id: &str,
    groups: &[crate::GroupBatchInput<'_>],
    now: i64,
) -> Result<()> {
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
        q.execute(&mut **tx).await?;
    }
    Ok(())
}

async fn insert_chat_self_aliases(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    account_id: &str,
    groups: &[crate::GroupBatchInput<'_>],
) -> Result<()> {
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
        q.execute(&mut **tx).await?;
    }
    Ok(())
}

async fn upsert_groups_table(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    account_id: &str,
    groups: &[crate::GroupBatchInput<'_>],
) -> Result<()> {
    const CHUNK: usize = 200;
    for chunk in groups.chunks(CHUNK) {
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
        q.execute(&mut **tx).await?;
    }
    Ok(())
}

async fn upsert_participant_contacts(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    account_id: &str,
    groups: &[crate::GroupBatchInput<'_>],
    now: i64,
) -> Result<()> {
    let mut contact_jids: Vec<&str> = Vec::new();
    for g in groups {
        if let Some(o) = g.owner {
            contact_jids.push(o);
        }
        contact_jids.extend(g.participant_jids.iter().copied());
    }
    let mut seen: HashSet<&str> = HashSet::with_capacity(contact_jids.len());
    contact_jids.retain(|j| seen.insert(*j));

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
        q.execute(&mut **tx).await?;
    }

    const CHAT_ALIAS_CHUNK: usize = 500;
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
        q.execute(&mut **tx).await?;
    }
    Ok(())
}
