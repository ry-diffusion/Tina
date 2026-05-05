// Mention-picker candidate query.
//
// Reads `groups.participants_json` (the snapshot the GroupsUpsert path
// stores) and joins each participant against `contact_aliases` →
// `contacts` so the popover can show resolved names. DMs return an
// empty list — WhatsApp's mention model is groups-only.

use std::collections::HashMap;

use crate::error::Result;
use crate::models::MentionCandidate;

use super::db::TinaDb;
use super::util::repeat_csv;

/// Subset of `ParticipantData` we read out of `participants_json`.
/// We deliberately don't depend on tina-core here so the DB crate
/// stays standalone — the JSON fields match because `WaIdentity`
/// serializes as a plain string.
#[derive(serde::Deserialize)]
struct ParsedParticipant {
    id: String,
    #[serde(default)]
    phone_number: Option<String>,
}

/// Row shape pulled by the bulk contacts JOIN. Matches the column
/// order in the SELECT.
type ContactRow = (
    String,         // alias_jid
    Option<String>, // contact_name
    Option<String>, // push_name
    Option<String>, // business_name
    Option<String>, // verified_name
    Option<String>, // phone_number
    Option<String>, // avatar_path
);

impl TinaDb {
    /// List candidates for the `@`-mention popover in `chat_id`.
    ///
    /// Returns an empty Vec for non-group chats and for groups whose
    /// `participants_json` hasn't been populated yet (still
    /// reconciling). `exclude_jid` is filtered out — the user
    /// can't mention themselves.
    pub async fn list_mention_candidates(
        &self,
        account_id: &str,
        chat_id: &str,
        exclude_jid: Option<&str>,
    ) -> Result<Vec<MentionCandidate>> {
        // Primary source: the group's `participants_json` snapshot.
        // Some groups arrive with this field NULL (the GroupsUpsert
        // event hadn't fired yet for them) — we fall back to
        // distinct senders observed in `messages` so the popover
        // still has data to filter on those chats.
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT participants_json FROM groups WHERE account_id = ? AND chat_id = ?",
        )
        .bind(account_id)
        .bind(chat_id)
        .fetch_optional(&self.pool)
        .await?;

        let mut parts: Vec<ParsedParticipant> = match &row {
            Some((Some(json),)) => serde_json::from_str(json).unwrap_or_default(),
            _ => Vec::new(),
        };

        // Fallback for groups with no snapshot yet: pull every
        // distinct sender of a message in this chat.
        if parts.is_empty() {
            let raw: Vec<(String,)> = sqlx::query_as(
                r#"SELECT DISTINCT m.sender_contact_id
                   FROM messages m
                   WHERE m.account_id = ? AND m.chat_id = ?
                     AND m.sender_contact_id IS NOT NULL
                     AND m.is_from_me = 0"#,
            )
            .bind(account_id)
            .bind(chat_id)
            .fetch_all(&self.pool)
            .await?;
            parts = raw
                .into_iter()
                .map(|(id,)| ParsedParticipant {
                    id,
                    phone_number: None,
                })
                .collect();
        }

        if parts.is_empty() {
            return Ok(Vec::new());
        }

        let alias_jids: Vec<&str> = parts
            .iter()
            .filter(|p| Some(p.id.as_str()) != exclude_jid)
            .map(|p| p.id.as_str())
            .collect();
        if alias_jids.is_empty() {
            return Ok(Vec::new());
        }

        // Bulk JOIN: alias_jid → contact fields. Done in one query so
        // a 200-member group still resolves in a single round-trip.
        let placeholders = repeat_csv("?", alias_jids.len());
        let sql = format!(
            r#"SELECT ca.alias_jid,
                      c.contact_name,
                      c.push_name,
                      c.business_name,
                      c.verified_name,
                      c.phone_number,
                      c.avatar_path
               FROM contact_aliases ca
               JOIN contacts c
                 ON c.account_id = ca.account_id AND c.contact_id = ca.contact_id
               WHERE ca.account_id = ? AND ca.alias_jid IN ({placeholders})"#
        );
        let mut q = sqlx::query_as::<_, ContactRow>(&sql).bind(account_id);
        for j in &alias_jids {
            q = q.bind(*j);
        }
        let resolved = q.fetch_all(&self.pool).await?;
        let by_alias: HashMap<String, ContactRow> = resolved
            .into_iter()
            .map(|r| (r.0.clone(), r))
            .collect();

        let mut out = Vec::with_capacity(alias_jids.len());
        for p in &parts {
            if Some(p.id.as_str()) == exclude_jid {
                continue;
            }
            let user_part = p.id.split('@').next().unwrap_or(&p.id).to_string();
            let (display_name, phone, avatar_path) = match by_alias.get(&p.id) {
                Some((_, contact_name, push, biz, verified, phone, avatar)) => {
                    let display = contact_name
                        .clone()
                        .or_else(|| push.clone())
                        .or_else(|| biz.clone())
                        .or_else(|| verified.clone())
                        .or_else(|| phone.clone())
                        .unwrap_or_else(|| user_part.clone());
                    let phone = phone
                        .clone()
                        .or_else(|| p.phone_number.clone())
                        .unwrap_or_else(|| user_part.clone());
                    (display, phone, avatar.clone())
                }
                None => {
                    let phone = p.phone_number.clone().unwrap_or_else(|| user_part.clone());
                    (phone.clone(), phone, None)
                }
            };
            out.push(MentionCandidate {
                jid: p.id.clone(),
                display_name,
                phone,
                avatar_path,
            });
        }
        // Stable order by display name so the popover doesn't shuffle
        // between two near-simultaneous opens.
        out.sort_by(|a, b| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()));
        Ok(out)
    }
}
