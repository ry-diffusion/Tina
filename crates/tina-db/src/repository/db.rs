// `TinaDb` itself — pool ownership + open-or-migrate logic.

use directories::ProjectDirs;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Pool, Sqlite};
use std::path::PathBuf;

use crate::error::{DbError, Result};
use crate::schema::{
    MIGRATION_V2_TO_V3, MIGRATION_V3_TO_V4, MIGRATION_V4_TO_V5, MIGRATION_V5_TO_V6, SCHEMA,
    SCHEMA_DROP, SCHEMA_VERSION,
};

pub struct TinaDb {
    pub(super) pool: Pool<Sqlite>,
}

impl TinaDb {
    pub async fn new() -> Result<Self> {
        let db_path = Self::get_db_path()?;
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let url = format!("sqlite:{}?mode=rwc", db_path.display());
        Self::open(&url).await
    }

    pub async fn new_with_path(path: &str) -> Result<Self> {
        let url = format!("sqlite:{}?mode=rwc", path);
        Self::open(&url).await
    }

    /// Abre (ou cria) um pool, garantindo o schema na versão atual.
    /// Quando `user_version` não bate, dropamos tudo e recriamos.
    pub async fn open(url: &str) -> Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(url)
            .await?;

        configure_pragmas(&pool).await?;
        migrate(&pool).await?;
        sqlx::query(&format!("PRAGMA user_version = {}", SCHEMA_VERSION))
            .execute(&pool)
            .await?;

        tracing::info!("Database ready at: {}", url);
        Ok(Self { pool })
    }

    /// Construtor para testes — banco em memória sem checagem de versão.
    pub async fn in_memory() -> Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await?;
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await?;
        sqlx::raw_sql(SCHEMA).execute(&pool).await?;
        Ok(Self { pool })
    }

    fn get_db_path() -> Result<PathBuf> {
        let dirs = ProjectDirs::from("com.br", "zesmoi", "tina")
            .ok_or_else(|| DbError::AccountNotFound("Could not find project dirs".into()))?;
        Ok(dirs.data_dir().join("tina.db"))
    }

    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }
}

async fn configure_pragmas(pool: &Pool<Sqlite>) -> Result<()> {
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(pool)
        .await?;
    // WAL: leitores não bloqueiam escritas e vice-versa. NORMAL: fsync
    // só em checkpoint (perda máxima ≈ último commit em queda de força,
    // aceitável p/ chat). Ganho ~5-10× em throughput de write.
    sqlx::query("PRAGMA journal_mode = WAL")
        .execute(pool)
        .await
        .ok();
    sqlx::query("PRAGMA synchronous = NORMAL")
        .execute(pool)
        .await
        .ok();
    // Cache maior reduz IOPS em sync inicial.
    sqlx::query("PRAGMA cache_size = -65536") // 64MB
        .execute(pool)
        .await
        .ok();
    sqlx::query("PRAGMA temp_store = MEMORY")
        .execute(pool)
        .await
        .ok();
    Ok(())
}

async fn migrate(pool: &Pool<Sqlite>) -> Result<()> {
    let current: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(pool)
        .await?;

    // Pra cada par (from, to) suportado, aplica ALTER TABLE in-place.
    // Versões mais antigas (sem migração escrita) caem no fallback de
    // drop+recreate.
    match current {
        0 => {
            sqlx::raw_sql(SCHEMA).execute(pool).await?;
        }
        v if v == SCHEMA_VERSION => {
            sqlx::raw_sql(SCHEMA).execute(pool).await?;
        }
        2 => {
            tracing::info!("Migrating tina.db from v2 → v6");
            sqlx::raw_sql(MIGRATION_V2_TO_V3).execute(pool).await?;
            sqlx::raw_sql(MIGRATION_V3_TO_V4).execute(pool).await?;
            sqlx::raw_sql(MIGRATION_V4_TO_V5).execute(pool).await?;
            sqlx::raw_sql(MIGRATION_V5_TO_V6).execute(pool).await?;
            sqlx::raw_sql(SCHEMA).execute(pool).await?;
        }
        3 => {
            tracing::info!("Migrating tina.db from v3 → v6");
            sqlx::raw_sql(MIGRATION_V3_TO_V4).execute(pool).await?;
            sqlx::raw_sql(MIGRATION_V4_TO_V5).execute(pool).await?;
            sqlx::raw_sql(MIGRATION_V5_TO_V6).execute(pool).await?;
            sqlx::raw_sql(SCHEMA).execute(pool).await?;
        }
        4 => {
            tracing::info!("Migrating tina.db from v4 → v6");
            sqlx::raw_sql(MIGRATION_V4_TO_V5).execute(pool).await?;
            sqlx::raw_sql(MIGRATION_V5_TO_V6).execute(pool).await?;
            sqlx::raw_sql(SCHEMA).execute(pool).await?;
        }
        5 => {
            tracing::info!("Migrating tina.db from v5 → v6 (media_thumbnail)");
            sqlx::raw_sql(MIGRATION_V5_TO_V6).execute(pool).await?;
            sqlx::raw_sql(SCHEMA).execute(pool).await?;
        }
        other => {
            tracing::warn!(
                "Unsupported schema version (db={}, expected={}). Recreating from scratch.",
                other,
                SCHEMA_VERSION
            );
            sqlx::raw_sql(SCHEMA_DROP).execute(pool).await?;
            sqlx::raw_sql(SCHEMA).execute(pool).await?;
        }
    }
    Ok(())
}
