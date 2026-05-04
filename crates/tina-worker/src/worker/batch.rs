// Pure DB-batch helpers: take buffered events, run them as a single
// SQLite transaction per account, return the affected chat IDs.

use std::collections::HashSet;

use tina_core::{ContactData, GroupData};
use tina_db::TinaDb;

use crate::error::Result;

/// Pura: aplica contatos em UMA transação e devolve os DM chats afetados.
pub(super) async fn process_contacts(
    db: &TinaDb,
    account_id: &str,
    contacts: Vec<ContactData>,
) -> Result<HashSet<String>> {
    if contacts.is_empty() {
        return Ok(HashSet::new());
    }

    let inputs: Vec<tina_db::ContactBatchInput<'_>> = contacts
        .iter()
        .map(|c| tina_db::ContactBatchInput {
            jid: c.jid.raw(),
            lid: c.lid.as_ref().map(|x| x.raw()),
            phone_number: c.phone_number.as_deref(),
            push_name: c.notify.as_deref(),
            contact_name: c.name.as_deref(),
            verified_name: c.verified_name.as_deref(),
            avatar_url: c.img_url.as_deref(),
            status: c.status.as_deref(),
        })
        .collect();

    let aliases = db.run_contacts_batch(account_id, &inputs).await?;

    // Lookup bulk de DM chats afetados (read-only, fora da transação).
    const CHUNK: usize = 500;
    let mut affected: HashSet<String> = HashSet::new();
    let alias_refs: Vec<&str> = aliases.iter().map(|s| s.as_str()).collect();
    for chunk in alias_refs.chunks(CHUNK) {
        let ids = db.find_dm_chat_ids_for_aliases(account_id, chunk).await?;
        affected.extend(ids);
    }
    Ok(affected)
}

/// Pura: aplica grupos/newsletters em UMA transação e devolve chats
/// afetados.
pub(super) async fn process_groups(
    db: &TinaDb,
    account_id: &str,
    groups: Vec<GroupData>,
) -> Result<HashSet<String>> {
    if groups.is_empty() {
        return Ok(HashSet::new());
    }
    // participants_json + participant_jids precisam viver pelo escopo
    // da chamada.
    let mut participants_json: Vec<Option<String>> = Vec::with_capacity(groups.len());
    let mut participant_id_storage: Vec<Vec<String>> = Vec::with_capacity(groups.len());
    for g in &groups {
        participants_json.push(serde_json::to_string(&g.participants).ok());
        participant_id_storage.push(
            g.participants
                .iter()
                .map(|p| p.id.raw().to_string())
                .collect(),
        );
    }
    // Refs depois que os Strings já estão armazenados.
    let participant_refs: Vec<Vec<&str>> = participant_id_storage
        .iter()
        .map(|v| v.iter().map(|s| s.as_str()).collect())
        .collect();

    let inputs: Vec<tina_db::GroupBatchInput<'_>> = groups
        .iter()
        .enumerate()
        .map(|(i, g)| tina_db::GroupBatchInput {
            jid: g.jid.raw(),
            subject: g.subject.as_deref(),
            owner: g.owner.as_ref().map(|x| x.raw()),
            description: g.description.as_deref(),
            participants_json: participants_json[i].as_deref(),
            participant_jids: participant_refs[i].as_slice(),
        })
        .collect();

    let affected = db.run_groups_batch(account_id, &inputs).await?;
    Ok(affected.into_iter().collect())
}
