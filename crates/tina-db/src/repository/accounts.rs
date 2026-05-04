// Accounts CRUD + identity persistence.

use crate::error::{DbError, Result};
use crate::models::Account;

use super::db::TinaDb;
use super::util::now_ts;

impl TinaDb {
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
}
