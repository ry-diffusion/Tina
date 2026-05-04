// Contacts: alias registration, individual upsert, batch upsert,
// directory listing.

use std::collections::HashMap;

use crate::error::Result;
use crate::models::{ChatKind, Contact};

use super::aliases::{link_alias_tx, register_contact_alias_tx};
use super::db::TinaDb;
use super::util::{derive_pn_lid, now_ts, repeat_csv};

impl TinaDb {
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

    /// Aplica todos os contatos em UMA transação **com multi-row INSERT**.
    /// Antes: 4 statements/contato (lookup + maybe insert + update). Aqui:
    /// 1 SELECT bulk pra mapear aliases existentes + 1 INSERT…UPSERT chunked
    /// pros contatos + 1 INSERT…DO NOTHING chunked pros aliases.
    pub async fn run_contacts_batch(
        &self,
        account_id: &str,
        contacts: &[crate::ContactBatchInput<'_>],
    ) -> Result<Vec<String>> {
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

        // 2. Pré-fetch dos aliases já mapeados.
        let existing = lookup_existing_contact_aliases(&mut tx, account_id, &all_aliases).await?;

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

        // 4. Bulk UPSERT em `contacts`.
        upsert_contacts_chunked(&mut tx, account_id, contacts, &resolved_ids).await?;

        // 5. Bulk INSERT em `contact_aliases`.
        insert_contact_aliases(&mut tx, account_id, contacts, &resolved_ids).await?;

        tx.commit().await?;
        Ok(all_aliases)
    }
}

async fn lookup_existing_contact_aliases(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    account_id: &str,
    all_aliases: &[String],
) -> Result<HashMap<String, String>> {
    use sqlx::Row;

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
        for row in q.fetch_all(&mut **tx).await? {
            let alias: String = row.get(0);
            let cid: String = row.get(1);
            existing.insert(alias, cid);
        }
    }
    Ok(existing)
}

async fn upsert_contacts_chunked(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    account_id: &str,
    contacts: &[crate::ContactBatchInput<'_>],
    resolved_ids: &[String],
) -> Result<()> {
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
        q.execute(&mut **tx).await?;
    }
    Ok(())
}

async fn insert_contact_aliases(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    account_id: &str,
    contacts: &[crate::ContactBatchInput<'_>],
    resolved_ids: &[String],
) -> Result<()> {
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
        q.execute(&mut **tx).await?;
    }
    Ok(())
}
