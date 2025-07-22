#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_database_connection() {
        let temp_file = NamedTempFile::new().unwrap();
        let db_path = temp_file.path().to_str().unwrap();
        let db_url = format!("sqlite://{db_path}");

        let db = Database::new(&db_url).await;
        assert!(db.is_ok());
    }

    #[tokio::test]
    async fn test_database_connection_in_memory() {
        let db = Database::new("sqlite::memory:").await;
        assert!(db.is_ok());
    }

    #[tokio::test]
    async fn test_database_close() {
        let db = Database::new("sqlite::memory:").await.unwrap();
        let result = db.close().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_database_health_check() {
        let db = Database::new("sqlite::memory:").await.unwrap();
        let is_healthy = db.health_check().await;
        assert!(is_healthy.is_ok());
        assert!(is_healthy.unwrap());
    }

    #[tokio::test]
    async fn test_database_migrations() {
        let db = Database::new("sqlite::memory:").await.unwrap();
        let result = db.run_migrations().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_database_tables_exist_after_init() {
        let db = Database::new("sqlite::memory:").await.unwrap();
        db.init().await.unwrap();

        // Check that tables exist by querying them
        let result = sqlx::query("SELECT COUNT(*) FROM processes")
            .fetch_one(&db.pool)
            .await;
        assert!(result.is_ok());

        let result = sqlx::query("SELECT COUNT(*) FROM environments")
            .fetch_one(&db.pool)
            .await;
        assert!(result.is_ok());

    }

    #[tokio::test]
    async fn test_database_indexes_exist() {
        let db = Database::new("sqlite::memory:").await.unwrap();
        db.init().await.unwrap();

        // Check that indexes exist by querying sqlite_master
        let result = sqlx::query("SELECT name FROM sqlite_master WHERE type='index' AND name='idx_processes_environment'")
            .fetch_one(&db.pool)
            .await;
        assert!(result.is_ok());

        let result = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type='index' AND name='idx_processes_status'",
        )
        .fetch_one(&db.pool)
        .await;
        assert!(result.is_ok());
    }
}

use crate::error::Result;
use sqlx::{Pool, Sqlite, SqlitePool, migrate::MigrateDatabase};

pub struct Database {
    pub pool: Pool<Sqlite>,
}

impl Database {
    pub async fn new(database_url: &str) -> Result<Self> {
        // Create database file if it doesn't exist (for file-based databases)
        if !database_url.contains("memory") && !Sqlite::database_exists(database_url).await? {
            Sqlite::create_database(database_url).await?;
        }

        let pool = SqlitePool::connect(database_url).await?;
        Ok(Database { pool })
    }

    pub async fn init(&self) -> Result<()> {
        self.run_migrations().await?;
        Ok(())
    }

    pub async fn close(self) -> Result<()> {
        self.pool.close().await;
        Ok(())
    }

    pub async fn health_check(&self) -> Result<bool> {
        let result = sqlx::query("SELECT 1").fetch_one(&self.pool).await;

        match result {
            Ok(_) => Ok(true),
            Err(e) => {
                tracing::error!("Database health check failed: {}", e);
                Ok(false)
            }
        }
    }

    pub async fn run_migrations(&self) -> Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }

    pub fn get_pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }
}
