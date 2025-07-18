use crate::error::Result;
use sqlx::{Pool, Row, Sqlite};
use std::sync::Arc;

/// Repository for process-related database operations
pub struct ProcessRepository {
    pool: Arc<Pool<Sqlite>>,
}

impl ProcessRepository {
    /// Create a new ProcessRepository
    pub fn new(pool: Arc<Pool<Sqlite>>) -> Self {
        Self { pool }
    }

    /// Get all processes marked as running in the database
    pub async fn get_running_processes(&self) -> Result<Vec<(String, String, Option<i64>)>> {
        let rows = sqlx::query(
            r#"
            SELECT id, environment_id, pid 
            FROM processes 
            WHERE status = 'running' OR status = 'starting'
            ORDER BY environment_id, id
            "#,
        )
        .fetch_all(self.pool.as_ref())
        .await?;

        let mut processes = Vec::new();
        for row in rows {
            let process_id: String = row.get("id");
            let environment_id: String = row.get("environment_id");
            let pid: Option<i64> = row.get("pid");
            processes.push((process_id, environment_id, pid));
        }

        Ok(processes)
    }

    /// Update process status in the database
    pub async fn update_process_status(&self, process_id: &str, status: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE processes 
            SET status = ?, updated_at = CURRENT_TIMESTAMP 
            WHERE id = ?
            "#,
        )
        .bind(status)
        .bind(process_id)
        .execute(self.pool.as_ref())
        .await?;

        Ok(())
    }

    /// Clear process PID in the database
    pub async fn clear_process_pid(&self, process_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE processes 
            SET pid = NULL, updated_at = CURRENT_TIMESTAMP 
            WHERE id = ?
            "#,
        )
        .bind(process_id)
        .execute(self.pool.as_ref())
        .await?;

        Ok(())
    }

    /// Set process PID in the database
    pub async fn set_process_pid(&self, process_id: &str, pid: i64) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE processes 
            SET pid = ?, updated_at = CURRENT_TIMESTAMP 
            WHERE id = ?
            "#,
        )
        .bind(pid)
        .bind(process_id)
        .execute(self.pool.as_ref())
        .await?;

        Ok(())
    }

    /// Insert a new process into the database
    pub async fn insert_process(
        &self,
        id: &str,
        name: &str,
        command: &str,
        working_dir: &str,
        environment_id: &str,
        status: &str,
        pid: Option<i64>,
    ) -> Result<()> {
        let now = chrono::Utc::now();
        
        sqlx::query(
            r#"
            INSERT INTO processes (
                id, name, command, working_dir, environment_id, status, pid,
                created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(id)
        .bind(name)
        .bind(command)
        .bind(working_dir)
        .bind(environment_id)
        .bind(status)
        .bind(pid)
        .bind(now)
        .bind(now)
        .execute(self.pool.as_ref())
        .await?;

        Ok(())
    }

    /// Delete a process from the database
    pub async fn delete_process(&self, process_id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM processes WHERE id = ?")
            .bind(process_id)
            .execute(self.pool.as_ref())
            .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Update process last activity time
    pub async fn update_process_activity(&self, process_id: &str) -> Result<()> {
        let now = chrono::Utc::now();
        sqlx::query(
            r#"
            UPDATE processes 
            SET updated_at = ? 
            WHERE id = ?
            "#,
        )
        .bind(now)
        .bind(process_id)
        .execute(self.pool.as_ref())
        .await?;

        Ok(())
    }

    /// Get process by ID
    pub async fn get_process_by_id(&self, process_id: &str) -> Result<Option<ProcessRecord>> {
        let row = sqlx::query(
            r#"
            SELECT id, name, command, working_dir, environment_id, status, pid,
                   created_at, updated_at
            FROM processes WHERE id = ?
            "#,
        )
        .bind(process_id)
        .fetch_optional(self.pool.as_ref())
        .await?;

        if let Some(row) = row {
            let process = ProcessRecord {
                id: row.get("id"),
                name: row.get("name"),
                command: row.get("command"),
                working_dir: row.get("working_dir"),
                environment_id: row.get("environment_id"),
                status: row.get("status"),
                pid: row.get("pid"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            };
            Ok(Some(process))
        } else {
            Ok(None)
        }
    }

    /// Get all processes for a specific environment
    pub async fn get_processes_by_environment(
        &self,
        environment_id: &str,
    ) -> Result<Vec<ProcessRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, command, working_dir, environment_id, status, pid,
                   created_at, updated_at
            FROM processes 
            WHERE environment_id = ?
            ORDER BY name
            "#,
        )
        .bind(environment_id)
        .fetch_all(self.pool.as_ref())
        .await?;

        let mut processes = Vec::new();
        for row in rows {
            let process = ProcessRecord {
                id: row.get("id"),
                name: row.get("name"),
                command: row.get("command"),
                working_dir: row.get("working_dir"),
                environment_id: row.get("environment_id"),
                status: row.get("status"),
                pid: row.get("pid"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            };
            processes.push(process);
        }

        Ok(processes)
    }

    /// Get processes by status
    pub async fn get_processes_by_status(&self, status: &str) -> Result<Vec<ProcessRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, command, working_dir, environment_id, status, pid,
                   created_at, updated_at
            FROM processes 
            WHERE status = ?
            ORDER BY environment_id, name
            "#,
        )
        .bind(status)
        .fetch_all(self.pool.as_ref())
        .await?;

        let mut processes = Vec::new();
        for row in rows {
            let process = ProcessRecord {
                id: row.get("id"),
                name: row.get("name"),
                command: row.get("command"),
                working_dir: row.get("working_dir"),
                environment_id: row.get("environment_id"),
                status: row.get("status"),
                pid: row.get("pid"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            };
            processes.push(process);
        }

        Ok(processes)
    }

    /// Update process command
    pub async fn update_process_command(&self, process_id: &str, command: &str) -> Result<()> {
        let now = chrono::Utc::now();
        sqlx::query(
            r#"
            UPDATE processes 
            SET command = ?, updated_at = ? 
            WHERE id = ?
            "#,
        )
        .bind(command)
        .bind(now)
        .bind(process_id)
        .execute(self.pool.as_ref())
        .await?;

        Ok(())
    }

    /// Update process working directory
    pub async fn update_process_working_dir(&self, process_id: &str, working_dir: &str) -> Result<()> {
        let now = chrono::Utc::now();
        sqlx::query(
            r#"
            UPDATE processes 
            SET working_dir = ?, updated_at = ? 
            WHERE id = ?
            "#,
        )
        .bind(working_dir)
        .bind(now)
        .bind(process_id)
        .execute(self.pool.as_ref())
        .await?;

        Ok(())
    }

    /// Count processes by environment
    pub async fn count_processes_by_environment(&self, environment_id: &str) -> Result<i64> {
        let row = sqlx::query(
            r#"
            SELECT COUNT(*) as count 
            FROM processes 
            WHERE environment_id = ?
            "#,
        )
        .bind(environment_id)
        .fetch_one(self.pool.as_ref())
        .await?;

        Ok(row.get("count"))
    }

    /// Count processes by status
    pub async fn count_processes_by_status(&self, status: &str) -> Result<i64> {
        let row = sqlx::query(
            r#"
            SELECT COUNT(*) as count 
            FROM processes 
            WHERE status = ?
            "#,
        )
        .bind(status)
        .fetch_one(self.pool.as_ref())
        .await?;

        Ok(row.get("count"))
    }

    /// Cleanup processes for a specific environment
    pub async fn cleanup_processes_by_environment(&self, environment_id: &str) -> Result<u64> {
        let result = sqlx::query("DELETE FROM processes WHERE environment_id = ?")
            .bind(environment_id)
            .execute(self.pool.as_ref())
            .await?;

        Ok(result.rows_affected())
    }
}

/// Database record for a process
#[derive(Debug, Clone)]
pub struct ProcessRecord {
    pub id: String,
    pub name: String,
    pub command: String,
    pub working_dir: String,
    pub environment_id: String,
    pub status: String,
    pub pid: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use uuid::Uuid;

    /// Helper function to create a test environment in the database
    async fn create_test_environment(db: &Database, environment_id: &str) {
        let now = chrono::Utc::now();
        sqlx::query(
            r#"
            INSERT INTO environments (id, name, description, project_path, config_path, status, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(environment_id)
        .bind("test_env")
        .bind("Test Environment")
        .bind("/tmp/test")
        .bind("/tmp/test/.runcept.toml")
        .bind("active")
        .bind(now)
        .bind(now)
        .execute(db.get_pool())
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_process_repository_crud() {
        let db = Database::new("sqlite::memory:").await.unwrap();
        db.init().await.unwrap();

        let repo = ProcessRepository::new(Arc::new(db.get_pool().clone()));

        // Create a test process
        let process_id = Uuid::new_v4().to_string();
        let environment_id = Uuid::new_v4().to_string();

        // Create test environment first
        create_test_environment(&db, &environment_id).await;

        // Insert process
        repo.insert_process(
            &process_id,
            "test_process",
            "echo hello",
            "/tmp",
            &environment_id,
            "running",
            Some(1234),
        )
        .await
        .unwrap();

        // Get process by ID
        let process = repo.get_process_by_id(&process_id).await.unwrap().unwrap();
        assert_eq!(process.name, "test_process");
        assert_eq!(process.status, "running");
        assert_eq!(process.pid, Some(1234));

        // Update process status
        repo.update_process_status(&process_id, "stopped").await.unwrap();
        let process = repo.get_process_by_id(&process_id).await.unwrap().unwrap();
        assert_eq!(process.status, "stopped");

        // Clear PID
        repo.clear_process_pid(&process_id).await.unwrap();
        let process = repo.get_process_by_id(&process_id).await.unwrap().unwrap();
        assert_eq!(process.pid, None);

        // Delete process
        let deleted = repo.delete_process(&process_id).await.unwrap();
        assert!(deleted);

        // Verify deletion
        let process = repo.get_process_by_id(&process_id).await.unwrap();
        assert!(process.is_none());
    }

    #[tokio::test]
    async fn test_get_running_processes() {
        let db = Database::new("sqlite::memory:").await.unwrap();
        db.init().await.unwrap();

        let repo = ProcessRepository::new(Arc::new(db.get_pool().clone()));

        let process_id = Uuid::new_v4().to_string();
        let environment_id = Uuid::new_v4().to_string();

        // Create test environment first
        create_test_environment(&db, &environment_id).await;

        // Insert running process
        repo.insert_process(
            &process_id,
            "test_process",
            "echo hello",
            "/tmp",
            &environment_id,
            "running",
            Some(1234),
        )
        .await
        .unwrap();

        // Get running processes
        let running = repo.get_running_processes().await.unwrap();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].0, process_id);
        assert_eq!(running[0].1, environment_id);
        assert_eq!(running[0].2, Some(1234));
    }
}