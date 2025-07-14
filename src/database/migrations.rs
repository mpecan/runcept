#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    #[tokio::test]
    async fn test_migration_manager_creation() {
        let db = Database::new("sqlite::memory:").await.unwrap();
        let manager = MigrationManager::new(db.get_pool());
        assert!(manager.pool.is_closed() == false);
    }

    #[tokio::test]
    async fn test_get_current_version() {
        let db = Database::new("sqlite::memory:").await.unwrap();
        db.init().await.unwrap();

        let manager = MigrationManager::new(db.get_pool());
        let version = manager.get_current_version().await;
        assert!(version.is_ok());
    }

    #[tokio::test]
    async fn test_migration_info() {
        let db = Database::new("sqlite::memory:").await.unwrap();
        db.init().await.unwrap();

        let manager = MigrationManager::new(db.get_pool());
        let info = manager.get_migration_info().await;
        assert!(info.is_ok());

        let migrations = info.unwrap();
        assert!(!migrations.is_empty());
    }

    #[tokio::test]
    async fn test_validate_schema() {
        let db = Database::new("sqlite::memory:").await.unwrap();
        db.init().await.unwrap();

        let manager = MigrationManager::new(db.get_pool());
        let is_valid = manager.validate_schema().await;
        assert!(is_valid.is_ok());
        assert!(is_valid.unwrap());
    }
}

use crate::error::Result;
use sqlx::{Pool, Row, Sqlite};

pub struct MigrationManager<'a> {
    pool: &'a Pool<Sqlite>,
}

#[derive(Debug, Clone)]
pub struct MigrationInfo {
    pub version: i64,
    pub description: String,
    pub installed_on: Option<chrono::DateTime<chrono::Utc>>,
    pub checksum: Vec<u8>,
    pub execution_time: Option<i64>,
    pub success: bool,
}

impl<'a> MigrationManager<'a> {
    pub fn new(pool: &'a Pool<Sqlite>) -> Self {
        Self { pool }
    }

    pub async fn get_current_version(&self) -> Result<i64> {
        let result =
            sqlx::query("SELECT version FROM _sqlx_migrations ORDER BY version DESC LIMIT 1")
                .fetch_optional(self.pool)
                .await?;

        match result {
            Some(row) => Ok(row.get("version")),
            None => Ok(0),
        }
    }

    pub async fn get_migration_info(&self) -> Result<Vec<MigrationInfo>> {
        let rows = sqlx::query("SELECT version, description, installed_on, checksum, execution_time, success FROM _sqlx_migrations ORDER BY version")
            .fetch_all(self.pool)
            .await?;

        let mut migrations = Vec::new();
        for row in rows {
            migrations.push(MigrationInfo {
                version: row.get("version"),
                description: row.get("description"),
                installed_on: row.get("installed_on"),
                checksum: row.get("checksum"),
                execution_time: row.get("execution_time"),
                success: row.get("success"),
            });
        }

        Ok(migrations)
    }

    pub async fn validate_schema(&self) -> Result<bool> {
        // Check if all required tables exist
        let required_tables = vec!["environments", "processes", "activity_logs"];

        for table in required_tables {
            let result =
                sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name=?")
                    .bind(table)
                    .fetch_optional(self.pool)
                    .await?;

            if result.is_none() {
                return Ok(false);
            }
        }

        // Check if required indexes exist
        let required_indexes = vec![
            "idx_processes_environment",
            "idx_processes_status",
            "idx_environments_status",
            "idx_activity_logs_timestamp",
        ];

        for index in required_indexes {
            let result =
                sqlx::query("SELECT name FROM sqlite_master WHERE type='index' AND name=?")
                    .bind(index)
                    .fetch_optional(self.pool)
                    .await?;

            if result.is_none() {
                return Ok(false);
            }
        }

        Ok(true)
    }

    pub async fn reset_database(&self) -> Result<()> {
        // Drop all tables in reverse order to handle foreign key constraints
        sqlx::query("DROP TABLE IF EXISTS activity_logs")
            .execute(self.pool)
            .await?;
        sqlx::query("DROP TABLE IF EXISTS processes")
            .execute(self.pool)
            .await?;
        sqlx::query("DROP TABLE IF EXISTS environments")
            .execute(self.pool)
            .await?;
        sqlx::query("DROP TABLE IF EXISTS _sqlx_migrations")
            .execute(self.pool)
            .await?;

        Ok(())
    }
}
