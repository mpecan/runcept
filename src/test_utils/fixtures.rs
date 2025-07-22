use crate::database::Database;
use crate::error::Result;
use std::sync::Arc;
use tempfile::TempDir;

/// Test fixture for creating in-memory databases
pub struct DatabaseFixture {
    pub database: Database,
    pub pool: Arc<sqlx::Pool<sqlx::Sqlite>>,
}

impl DatabaseFixture {
    pub async fn new() -> Result<Self> {
        let database = Database::new("sqlite::memory:").await?;
        database.init().await?;
        let pool = Arc::new(database.get_pool().clone());

        Ok(Self { database, pool })
    }

    /// Create a test environment in the database
    pub async fn create_test_environment(&self, environment_id: &str) -> Result<()> {
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
        .execute(self.database.get_pool())
        .await?;

        Ok(())
    }
}

/// Test fixture for creating temporary directories
pub struct TempDirFixture {
    pub temp_dir: TempDir,
}

impl TempDirFixture {
    pub fn new() -> Result<Self> {
        let temp_dir = TempDir::new().map_err(crate::error::RunceptError::IoError)?;

        Ok(Self { temp_dir })
    }

    pub fn path(&self) -> &std::path::Path {
        self.temp_dir.path()
    }

    pub fn path_string(&self) -> String {
        self.temp_dir.path().to_string_lossy().to_string()
    }
}
