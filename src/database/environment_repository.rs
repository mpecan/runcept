use crate::config::environment::{Environment, EnvironmentStatus};
use crate::config::project::ProjectConfig;
use crate::error::Result;
use chrono::{DateTime, Utc};
use sqlx::{Pool, Row, Sqlite};
use std::path::PathBuf;
use std::sync::Arc;

/// Repository for environment-related database operations
pub struct EnvironmentRepository {
    pool: Arc<Pool<Sqlite>>,
}

impl EnvironmentRepository {
    /// Create a new EnvironmentRepository
    pub fn new(pool: Arc<Pool<Sqlite>>) -> Self {
        Self { pool }
    }

    /// Insert a new environment into the database
    pub async fn insert_environment(&self, env: &Environment) -> Result<()> {
        let config_path = env.project_path.join(".runcept.toml");

        sqlx::query(
            r#"
            INSERT INTO environments (
                id, name, description, project_path, config_path, status, 
                created_at, updated_at, last_activity, inactivity_timeout, auto_shutdown
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&env.id)
        .bind(&env.name)
        .bind(env.project_config.environment.name.as_str()) // Use environment name as description
        .bind(env.project_path.to_string_lossy().as_ref())
        .bind(config_path.to_string_lossy().as_ref())
        .bind(env.status.to_string())
        .bind(env.created_at)
        .bind(env.updated_at)
        .bind(env.last_activity)
        .bind(
            env.project_config
                .environment
                .inactivity_timeout
                .as_ref()
                .and_then(|t| t.parse::<u32>().ok())
                .map(|t| t as i32),
        )
        .bind(
            env.project_config
                .environment
                .auto_shutdown
                .unwrap_or(false),
        )
        .execute(self.pool.as_ref())
        .await?;

        Ok(())
    }

    /// Update an existing environment in the database
    pub async fn update_environment(&self, env: &Environment) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE environments SET 
                name = ?, description = ?, status = ?, updated_at = ?, 
                last_activity = ?, inactivity_timeout = ?, auto_shutdown = ?
            WHERE id = ?
            "#,
        )
        .bind(&env.name)
        .bind(env.project_config.environment.name.as_str())
        .bind(env.status.to_string())
        .bind(env.updated_at)
        .bind(env.last_activity)
        .bind(
            env.project_config
                .environment
                .inactivity_timeout
                .as_ref()
                .and_then(|t| t.parse::<u32>().ok())
                .map(|t| t as i32),
        )
        .bind(
            env.project_config
                .environment
                .auto_shutdown
                .unwrap_or(false),
        )
        .bind(&env.id)
        .execute(self.pool.as_ref())
        .await?;

        Ok(())
    }

    /// Get an environment by ID from the database
    pub async fn get_environment_by_id(&self, id: &str) -> Result<Option<Environment>> {
        let row = sqlx::query(
            r#"
            SELECT id, name, description, project_path, config_path, status, 
                   created_at, updated_at, last_activity, inactivity_timeout, auto_shutdown
            FROM environments WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(self.pool.as_ref())
        .await?;

        if let Some(row) = row {
            let environment = self.row_to_environment(row).await?;
            Ok(Some(environment))
        } else {
            Ok(None)
        }
    }

    /// Get an environment by project path from the database
    pub async fn get_environment_by_path(&self, project_path: &str) -> Result<Option<Environment>> {
        let row = sqlx::query(
            r#"
            SELECT id, name, description, project_path, config_path, status, 
                   created_at, updated_at, last_activity, inactivity_timeout, auto_shutdown
            FROM environments WHERE project_path = ?
            "#,
        )
        .bind(project_path)
        .fetch_optional(self.pool.as_ref())
        .await?;

        if let Some(row) = row {
            let environment = self.row_to_environment(row).await?;
            Ok(Some(environment))
        } else {
            Ok(None)
        }
    }

    /// List all environments from the database
    pub async fn list_environments(&self) -> Result<Vec<Environment>> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, description, project_path, config_path, status, 
                   created_at, updated_at, last_activity, inactivity_timeout, auto_shutdown
            FROM environments ORDER BY updated_at DESC
            "#,
        )
        .fetch_all(self.pool.as_ref())
        .await?;

        let mut environments = Vec::new();
        for row in rows {
            let environment = self.row_to_environment(row).await?;
            environments.push(environment);
        }

        Ok(environments)
    }

    /// Delete an environment from the database
    pub async fn delete_environment(&self, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM environments WHERE id = ?")
            .bind(id)
            .execute(self.pool.as_ref())
            .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Update the last activity time for an environment
    pub async fn update_environment_activity(&self, id: &str) -> Result<()> {
        let now = Utc::now();
        sqlx::query("UPDATE environments SET last_activity = ?, updated_at = ? WHERE id = ?")
            .bind(now)
            .bind(now)
            .bind(id)
            .execute(self.pool.as_ref())
            .await?;

        Ok(())
    }

    /// Search environments by name or description
    pub async fn search_environments(&self, query: &str) -> Result<Vec<Environment>> {
        let search_pattern = format!("%{query}%");
        let rows = sqlx::query(
            r#"
            SELECT id, name, description, project_path, config_path, status, 
                   created_at, updated_at, last_activity, inactivity_timeout, auto_shutdown
            FROM environments 
            WHERE name LIKE ? OR description LIKE ? OR project_path LIKE ?
            ORDER BY updated_at DESC
            "#,
        )
        .bind(&search_pattern)
        .bind(&search_pattern)
        .bind(&search_pattern)
        .fetch_all(self.pool.as_ref())
        .await?;

        let mut environments = Vec::new();
        for row in rows {
            let environment = self.row_to_environment(row).await?;
            environments.push(environment);
        }

        Ok(environments)
    }

    /// Get environments by status
    pub async fn get_environments_by_status(
        &self,
        status: EnvironmentStatus,
    ) -> Result<Vec<Environment>> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, description, project_path, config_path, status, 
                   created_at, updated_at, last_activity, inactivity_timeout, auto_shutdown
            FROM environments WHERE status = ? ORDER BY updated_at DESC
            "#,
        )
        .bind(status.to_string())
        .fetch_all(self.pool.as_ref())
        .await?;

        let mut environments = Vec::new();
        for row in rows {
            let environment = self.row_to_environment(row).await?;
            environments.push(environment);
        }

        Ok(environments)
    }

    /// Get all environments in specific statuses (returns just ID and status)
    pub async fn get_environments_by_status_ids(
        &self,
        statuses: &[&str],
    ) -> Result<Vec<(String, String)>> {
        let status_placeholders = statuses.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let query = format!(
            r#"
            SELECT id, status 
            FROM environments 
            WHERE status IN ({})
            ORDER BY id
            "#,
            status_placeholders
        );

        let mut query_builder = sqlx::query(&query);
        for status in statuses {
            query_builder = query_builder.bind(*status);
        }

        let rows = query_builder.fetch_all(self.pool.as_ref()).await?;

        let mut environments = Vec::new();
        for row in rows {
            let env_id: String = row.get("id");
            let status: String = row.get("status");
            environments.push((env_id, status));
        }

        Ok(environments)
    }

    /// Update environment status in the database
    pub async fn update_environment_status(&self, env_id: &str, status: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE environments 
            SET status = ?, updated_at = CURRENT_TIMESTAMP 
            WHERE id = ?
            "#,
        )
        .bind(status)
        .bind(env_id)
        .execute(self.pool.as_ref())
        .await?;

        Ok(())
    }

    /// Update environment name
    pub async fn update_environment_name(&self, env_id: &str, name: &str) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            r#"
            UPDATE environments 
            SET name = ?, updated_at = ? 
            WHERE id = ?
            "#,
        )
        .bind(name)
        .bind(now)
        .bind(env_id)
        .execute(self.pool.as_ref())
        .await?;

        Ok(())
    }

    /// Update environment project path
    pub async fn update_environment_path(&self, env_id: &str, project_path: &str) -> Result<()> {
        let now = Utc::now();
        let config_path = PathBuf::from(project_path).join(".runcept.toml");
        
        sqlx::query(
            r#"
            UPDATE environments 
            SET project_path = ?, config_path = ?, updated_at = ? 
            WHERE id = ?
            "#,
        )
        .bind(project_path)
        .bind(config_path.to_string_lossy().as_ref())
        .bind(now)
        .bind(env_id)
        .execute(self.pool.as_ref())
        .await?;

        Ok(())
    }

    /// Count environments by status
    pub async fn count_environments_by_status(&self, status: &str) -> Result<i64> {
        let row = sqlx::query(
            r#"
            SELECT COUNT(*) as count 
            FROM environments 
            WHERE status = ?
            "#,
        )
        .bind(status)
        .fetch_one(self.pool.as_ref())
        .await?;

        Ok(row.get("count"))
    }

    /// Count total environments
    pub async fn count_environments(&self) -> Result<i64> {
        let row = sqlx::query("SELECT COUNT(*) as count FROM environments")
            .fetch_one(self.pool.as_ref())
            .await?;

        Ok(row.get("count"))
    }

    /// Get environments that have been inactive for longer than the specified timeout
    pub async fn get_inactive_environments(&self, timeout_minutes: i32) -> Result<Vec<Environment>> {
        let cutoff_time = Utc::now() - chrono::Duration::minutes(timeout_minutes as i64);
        
        let rows = sqlx::query(
            r#"
            SELECT id, name, description, project_path, config_path, status, 
                   created_at, updated_at, last_activity, inactivity_timeout, auto_shutdown
            FROM environments 
            WHERE status = 'active' 
            AND auto_shutdown = 1 
            AND (last_activity IS NULL OR last_activity < ?)
            ORDER BY last_activity ASC
            "#,
        )
        .bind(cutoff_time)
        .fetch_all(self.pool.as_ref())
        .await?;

        let mut environments = Vec::new();
        for row in rows {
            let environment = self.row_to_environment(row).await?;
            environments.push(environment);
        }

        Ok(environments)
    }

    /// Helper method to convert a database row to an Environment struct
    async fn row_to_environment(&self, row: sqlx::sqlite::SqliteRow) -> Result<Environment> {
        let project_path: String = row.get("project_path");
        let config_path: String = row.get("config_path");

        // Load the project config from file
        let project_config = if std::path::Path::new(&config_path).exists() {
            ProjectConfig::load_from_path(std::path::Path::new(&config_path))
                .await
                .unwrap_or_default()
        } else {
            ProjectConfig::default()
        };

        let status_str: String = row.get("status");
        let status = match status_str.as_str() {
            "inactive" => EnvironmentStatus::Inactive,
            "activating" => EnvironmentStatus::Activating,
            "active" => EnvironmentStatus::Active,
            "deactivating" => EnvironmentStatus::Deactivating,
            "failed" => EnvironmentStatus::Failed,
            _ => EnvironmentStatus::Inactive,
        };

        let created_at: DateTime<Utc> = row.get("created_at");
        let updated_at: DateTime<Utc> = row.get("updated_at");
        let last_activity: Option<DateTime<Utc>> = row.get("last_activity");

        Ok(Environment {
            id: row.get("id"),
            name: row.get("name"),
            project_path: PathBuf::from(project_path),
            status,
            created_at,
            updated_at,
            last_activity,
            processes: Vec::new(), // Will be populated separately if needed
            project_config: project_config.clone(),
            merged_config: project_config, // For now, same as project_config
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::project::ProjectEnvironmentConfig;
    use crate::database::Database;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_environment_repository_crud() {
        let db = Database::new("sqlite::memory:").await.unwrap();
        db.init().await.unwrap();

        let repo = EnvironmentRepository::new(Arc::new(db.get_pool().clone()));

        // Create a test environment
        let env_id = Uuid::new_v4().to_string();
        let project_path = PathBuf::from("/tmp/test_project");
        let now = Utc::now();

        let mut env = Environment {
            id: env_id.clone(),
            name: "test_env".to_string(),
            project_path: project_path.clone(),
            status: EnvironmentStatus::Active,
            created_at: now,
            updated_at: now,
            last_activity: Some(now),
            processes: Vec::new(),
            project_config: ProjectConfig {
                environment: ProjectEnvironmentConfig {
                    name: "Test Environment".to_string(),
                    inactivity_timeout: Some("30m".to_string()),
                    auto_shutdown: Some(true),
                    env_vars: Default::default(),
                },
                processes: Default::default(),
            },
            merged_config: ProjectConfig::default(),
        };

        // Insert environment
        repo.insert_environment(&env).await.unwrap();

        // Get environment by ID
        let retrieved = repo.get_environment_by_id(&env_id).await.unwrap().unwrap();
        assert_eq!(retrieved.name, "test_env");
        assert_eq!(retrieved.status, EnvironmentStatus::Active);

        // Update environment status
        repo.update_environment_status(&env_id, "inactive").await.unwrap();
        let retrieved = repo.get_environment_by_id(&env_id).await.unwrap().unwrap();
        assert_eq!(retrieved.status, EnvironmentStatus::Inactive);

        // Update environment name
        repo.update_environment_name(&env_id, "updated_name").await.unwrap();
        let retrieved = repo.get_environment_by_id(&env_id).await.unwrap().unwrap();
        assert_eq!(retrieved.name, "updated_name");

        // Delete environment
        let deleted = repo.delete_environment(&env_id).await.unwrap();
        assert!(deleted);

        // Verify deletion
        let retrieved = repo.get_environment_by_id(&env_id).await.unwrap();
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_get_environments_by_status() {
        let db = Database::new("sqlite::memory:").await.unwrap();
        db.init().await.unwrap();

        let repo = EnvironmentRepository::new(Arc::new(db.get_pool().clone()));

        let env_id = Uuid::new_v4().to_string();
        let project_path = PathBuf::from("/tmp/test_project");
        let now = Utc::now();

        let env = Environment {
            id: env_id.clone(),
            name: "test_env".to_string(),
            project_path,
            status: EnvironmentStatus::Active,
            created_at: now,
            updated_at: now,
            last_activity: Some(now),
            processes: Vec::new(),
            project_config: ProjectConfig::default(),
            merged_config: ProjectConfig::default(),
        };

        // Insert environment
        repo.insert_environment(&env).await.unwrap();

        // Get active environments
        let active_envs = repo.get_environments_by_status(EnvironmentStatus::Active).await.unwrap();
        assert_eq!(active_envs.len(), 1);
        assert_eq!(active_envs[0].id, env_id);

        // Get inactive environments (should be empty)
        let inactive_envs = repo.get_environments_by_status(EnvironmentStatus::Inactive).await.unwrap();
        assert_eq!(inactive_envs.len(), 0);
    }

    #[tokio::test]
    async fn test_search_environments() {
        let db = Database::new("sqlite::memory:").await.unwrap();
        db.init().await.unwrap();

        let repo = EnvironmentRepository::new(Arc::new(db.get_pool().clone()));

        let env_id = Uuid::new_v4().to_string();
        let project_path = PathBuf::from("/tmp/search_test");
        let now = Utc::now();

        let env = Environment {
            id: env_id.clone(),
            name: "search_test_env".to_string(),
            project_path,
            status: EnvironmentStatus::Active,
            created_at: now,
            updated_at: now,
            last_activity: Some(now),
            processes: Vec::new(),
            project_config: ProjectConfig::default(),
            merged_config: ProjectConfig::default(),
        };

        // Insert environment
        repo.insert_environment(&env).await.unwrap();

        // Search by name
        let results = repo.search_environments("search_test").await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, env_id);

        // Search by path
        let results = repo.search_environments("/tmp/search").await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, env_id);

        // Search with no matches
        let results = repo.search_environments("nonexistent").await.unwrap();
        assert_eq!(results.len(), 0);
    }
}