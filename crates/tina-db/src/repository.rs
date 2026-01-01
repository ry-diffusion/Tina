use directories::ProjectDirs;
use sqlx::{Pool, Sqlite, SqlitePool};
use std::path::PathBuf;

use crate::error::{DbError, Result};
use crate::models::{Account, Contact, Group, Message};
use crate::schema::SCHEMA;

pub struct TinaDb {
    pool: Pool<Sqlite>,
}

impl TinaDb {
    pub async fn new() -> Result<Self> {
        let db_path = Self::get_db_path()?;

        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let db_url = format!("sqlite:{}?mode=rwc", db_path.display());
        let pool = SqlitePool::connect(&db_url).await?;

        sqlx::raw_sql(SCHEMA).execute(&pool).await?;

        tracing::info!("Database initialized at: {}", db_path.display());

        Ok(Self { pool })
    }

    pub async fn new_with_path(path: &str) -> Result<Self> {
        let db_url = format!("sqlite:{}?mode=rwc", path);
        let pool = SqlitePool::connect(&db_url).await?;
        sqlx::raw_sql(SCHEMA).execute(&pool).await?;
        Ok(Self { pool })
    }

    fn get_db_path() -> Result<PathBuf> {
        let dirs = ProjectDirs::from("com.br", "zesmoi", "tina")
            .ok_or_else(|| DbError::AccountNotFound("Could not find project dirs".into()))?;
        Ok(dirs.data_dir().join("tina.db"))
    }

    pub async fn create_account(&self, id: &str, name: Option<&str>) -> Result<Account> {
        let now = chrono_timestamp();

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
        Ok(sqlx::query_as::<_, Account>("SELECT * FROM accounts ORDER BY created_at")
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn save_auth_state(&self, account_id: &str, auth_state: &str) -> Result<()> {
        let now = chrono_timestamp();

        sqlx::query("UPDATE accounts SET auth_state = ?, updated_at = ? WHERE id = ?")
            .bind(auth_state)
            .bind(now)
            .bind(account_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn get_auth_state(&self, account_id: &str) -> Result<Option<String>> {
        let account = self.get_account(account_id).await?;
        Ok(account.auth_state)
    }

    pub async fn upsert_contact(
        &self,
        account_id: &str,
        jid: &str,
        lid: Option<&str>,
        phone_number: Option<&str>,
        name: Option<&str>,
        notify_name: Option<&str>,
        verified_name: Option<&str>,
        img_url: Option<&str>,
        status: Option<&str>,
        is_local: bool,
    ) -> Result<()> {
        let now = chrono_timestamp();

        sqlx::query(
            r#"INSERT INTO contacts (account_id, jid, lid, phone_number, name, notify_name, verified_name, img_url, status, is_local, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(account_id, jid) DO UPDATE SET
                 lid = COALESCE(excluded.lid, lid),
                 phone_number = COALESCE(excluded.phone_number, phone_number),
                 name = COALESCE(excluded.name, name),
                 notify_name = COALESCE(excluded.notify_name, notify_name),
                 verified_name = COALESCE(excluded.verified_name, verified_name),
                 img_url = COALESCE(excluded.img_url, img_url),
                 status = COALESCE(excluded.status, status),
                 is_local = excluded.is_local,
                 updated_at = excluded.updated_at"#,
        )
        .bind(account_id)
        .bind(jid)
        .bind(lid)
        .bind(phone_number)
        .bind(name)
        .bind(notify_name)
        .bind(verified_name)
        .bind(img_url)
        .bind(status)
        .bind(is_local)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_contacts(&self, account_id: &str) -> Result<Vec<Contact>> {
        Ok(
            sqlx::query_as::<_, Contact>("SELECT * FROM contacts WHERE account_id = ? ORDER BY name")
                .bind(account_id)
                .fetch_all(&self.pool)
                .await?,
        )
    }

    pub async fn get_contact_by_jid(&self, account_id: &str, jid: &str) -> Result<Option<Contact>> {
        Ok(sqlx::query_as::<_, Contact>(
            "SELECT * FROM contacts WHERE account_id = ? AND jid = ?",
        )
        .bind(account_id)
        .bind(jid)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn upsert_group(
        &self,
        account_id: &str,
        jid: &str,
        subject: Option<&str>,
        owner: Option<&str>,
        description: Option<&str>,
        participants_json: Option<&str>,
    ) -> Result<()> {
        let now = chrono_timestamp();

        sqlx::query(
            r#"INSERT INTO groups (account_id, jid, subject, owner, description, participants_json, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(account_id, jid) DO UPDATE SET
                 subject = COALESCE(excluded.subject, subject),
                 owner = COALESCE(excluded.owner, owner),
                 description = COALESCE(excluded.description, description),
                 participants_json = COALESCE(excluded.participants_json, participants_json),
                 updated_at = excluded.updated_at"#,
        )
        .bind(account_id)
        .bind(jid)
        .bind(subject)
        .bind(owner)
        .bind(description)
        .bind(participants_json)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_groups(&self, account_id: &str) -> Result<Vec<Group>> {
        Ok(
            sqlx::query_as::<_, Group>("SELECT * FROM groups WHERE account_id = ? ORDER BY subject")
                .bind(account_id)
                .fetch_all(&self.pool)
                .await?,
        )
    }

    pub async fn get_group_by_jid(&self, account_id: &str, jid: &str) -> Result<Option<Group>> {
        Ok(sqlx::query_as::<_, Group>(
            "SELECT * FROM groups WHERE account_id = ? AND jid = ?",
        )
        .bind(account_id)
        .bind(jid)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn insert_message(
        &self,
        account_id: &str,
        message_id: &str,
        chat_jid: &str,
        sender_jid: &str,
        content: Option<&str>,
        message_type: &str,
        timestamp: i64,
        is_from_me: bool,
        raw_json: Option<&str>,
    ) -> Result<()> {
        let now = chrono_timestamp();

        sqlx::query(
            r#"INSERT OR IGNORE INTO messages 
               (account_id, message_id, chat_jid, sender_jid, content, message_type, timestamp, is_from_me, raw_json, created_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(account_id)
        .bind(message_id)
        .bind(chat_jid)
        .bind(sender_jid)
        .bind(content)
        .bind(message_type)
        .bind(timestamp)
        .bind(is_from_me)
        .bind(raw_json)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_messages(
        &self,
        account_id: &str,
        chat_jid: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Message>> {
        if let Some(chat) = chat_jid {
            Ok(sqlx::query_as::<_, Message>(
                "SELECT * FROM messages WHERE account_id = ? AND chat_jid = ? ORDER BY timestamp DESC LIMIT ? OFFSET ?",
            )
            .bind(account_id)
            .bind(chat)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?)
        } else {
            Ok(sqlx::query_as::<_, Message>(
                "SELECT * FROM messages WHERE account_id = ? ORDER BY timestamp DESC LIMIT ? OFFSET ?",
            )
            .bind(account_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?)
        }
    }

    pub async fn get_chats(&self, account_id: &str) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT chat_jid FROM messages WHERE account_id = ? GROUP BY chat_jid ORDER BY MAX(timestamp) DESC",
        )
        .bind(account_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|(jid,)| jid).collect())
    }

    pub async fn delete_account(&self, account_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM accounts WHERE id = ?")
            .bind(account_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

fn chrono_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
