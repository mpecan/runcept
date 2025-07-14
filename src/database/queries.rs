#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_query_manager_creation() {
        let db = Database::new("sqlite::memory:").await.unwrap();
        db.init().await.unwrap();
        
        let manager = QueryManager::new(db.get_pool());
        assert!(manager.pool.is_closed() == false);
    }

    #[tokio::test]
    async fn test_table_exists() {
        let db = Database::new("sqlite::memory:").await.unwrap();
        db.init().await.unwrap();
        
        let manager = QueryManager::new(db.get_pool());
        
        let exists = manager.table_exists("processes").await.unwrap();
        assert!(exists);
        
        let exists = manager.table_exists("nonexistent_table").await.unwrap();
        assert!(!exists);
    }

    #[tokio::test]
    async fn test_count_records() {
        let db = Database::new("sqlite::memory:").await.unwrap();
        db.init().await.unwrap();
        
        let manager = QueryManager::new(db.get_pool());
        
        let count = manager.count_records("processes").await.unwrap();
        assert_eq!(count, 0);
        
        let count = manager.count_records("environments").await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_get_database_info() {
        let db = Database::new("sqlite::memory:").await.unwrap();
        db.init().await.unwrap();
        
        let manager = QueryManager::new(db.get_pool());
        let info = manager.get_database_info().await.unwrap();
        
        assert!(info.contains_key("processes"));
        assert!(info.contains_key("environments"));
        assert!(info.contains_key("activity_logs"));
        assert_eq!(info["processes"], 0);
        assert_eq!(info["environments"], 0);
        assert_eq!(info["activity_logs"], 0);
    }

    #[tokio::test]
    async fn test_cleanup_old_activity_logs() {
        let db = Database::new("sqlite::memory:").await.unwrap();
        db.init().await.unwrap();
        
        let manager = QueryManager::new(db.get_pool());
        
        // Insert test environment and process first
        let env_id = Uuid::new_v4().to_string();
        let process_id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now();
        
        // Insert test environment
        sqlx::query(
            "INSERT INTO environments (id, name, project_path, config_path, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&env_id)
        .bind("test_env")
        .bind("/tmp/test")
        .bind("/tmp/test/.runit.toml")
        .bind("active")
        .bind(now)
        .bind(now)
        .execute(db.get_pool())
        .await
        .unwrap();
        
        // Insert test process
        sqlx::query(
            "INSERT INTO processes (id, name, command, working_dir, environment_id, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&process_id)
        .bind("test_process")
        .bind("echo test")
        .bind("/tmp")
        .bind(&env_id)
        .bind("running")
        .bind(now)
        .bind(now)
        .execute(db.get_pool())
        .await
        .unwrap();
        
        // Insert old log (more than 7 days ago)
        let old_timestamp = chrono::Utc::now() - chrono::Duration::days(8);
        sqlx::query(
            "INSERT INTO activity_logs (process_id, environment_id, activity_type, timestamp) VALUES (?, ?, ?, ?)"
        )
        .bind(&process_id)
        .bind(&env_id)
        .bind("test_activity")
        .bind(old_timestamp)
        .execute(db.get_pool())
        .await
        .unwrap();
        
        // Insert recent log
        let recent_timestamp = chrono::Utc::now() - chrono::Duration::days(1);
        sqlx::query(
            "INSERT INTO activity_logs (process_id, environment_id, activity_type, timestamp) VALUES (?, ?, ?, ?)"
        )
        .bind(&process_id)
        .bind(&env_id)
        .bind("test_activity")
        .bind(recent_timestamp)
        .execute(db.get_pool())
        .await
        .unwrap();
        
        // Verify we have 2 logs
        let count = manager.count_records("activity_logs").await.unwrap();
        assert_eq!(count, 2);
        
        // Clean up old logs
        let deleted = manager.cleanup_old_activity_logs(7).await.unwrap();
        assert_eq!(deleted, 1);
        
        // Verify only 1 log remains
        let count = manager.count_records("activity_logs").await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_vacuum_database() {
        let db = Database::new("sqlite::memory:").await.unwrap();
        db.init().await.unwrap();
        
        let manager = QueryManager::new(db.get_pool());
        let result = manager.vacuum_database().await;
        assert!(result.is_ok());
    }
}

use sqlx::{Pool, Sqlite, Row};
use std::collections::HashMap;
use crate::error::Result;

pub struct QueryManager<'a> {
    pool: &'a Pool<Sqlite>,
}

impl<'a> QueryManager<'a> {
    pub fn new(pool: &'a Pool<Sqlite>) -> Self {
        Self { pool }
    }

    pub async fn table_exists(&self, table_name: &str) -> Result<bool> {
        let result = sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name=?")
            .bind(table_name)
            .fetch_optional(self.pool)
            .await?;
        
        Ok(result.is_some())
    }

    pub async fn count_records(&self, table_name: &str) -> Result<i64> {
        let query = format!("SELECT COUNT(*) as count FROM {}", table_name);
        let row = sqlx::query(&query)
            .fetch_one(self.pool)
            .await?;
        
        Ok(row.get("count"))
    }

    pub async fn get_database_info(&self) -> Result<HashMap<String, i64>> {
        let mut info = HashMap::new();
        
        let tables = vec!["processes", "environments", "activity_logs"];
        
        for table in tables {
            let count = self.count_records(table).await?;
            info.insert(table.to_string(), count);
        }
        
        Ok(info)
    }

    pub async fn cleanup_old_activity_logs(&self, days_to_keep: i64) -> Result<u64> {
        let cutoff_date = chrono::Utc::now() - chrono::Duration::days(days_to_keep);
        
        let result = sqlx::query("DELETE FROM activity_logs WHERE timestamp < ?")
            .bind(cutoff_date)
            .execute(self.pool)
            .await?;
        
        Ok(result.rows_affected())
    }

    pub async fn vacuum_database(&self) -> Result<()> {
        sqlx::query("VACUUM").execute(self.pool).await?;
        Ok(())
    }

    pub async fn analyze_database(&self) -> Result<()> {
        sqlx::query("ANALYZE").execute(self.pool).await?;
        Ok(())
    }

    pub async fn get_database_size(&self) -> Result<i64> {
        let row = sqlx::query("SELECT page_count * page_size as size FROM pragma_page_count(), pragma_page_size()")
            .fetch_one(self.pool)
            .await?;
        
        Ok(row.get("size"))
    }

    pub async fn integrity_check(&self) -> Result<bool> {
        let result = sqlx::query("PRAGMA integrity_check")
            .fetch_one(self.pool)
            .await?;
        
        let check_result: String = result.get(0);
        Ok(check_result == "ok")
    }
}