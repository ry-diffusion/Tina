// Key/value settings store. Backed by the `settings` table created in
// the base schema; values are opaque strings — callers serialize as
// they like (booleans as "0"/"1", JSON for structured values, etc.).

use crate::error::Result;

use super::db::TinaDb;

impl TinaDb {
    pub async fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let row: Option<(Option<String>,)> =
            sqlx::query_as("SELECT value FROM settings WHERE key = ?")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.and_then(|(v,)| v))
    }

    pub async fn put_setting(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
