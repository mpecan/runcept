use crate::error::Result;

/// Parameters for updating process information
#[derive(Debug, Clone)]
pub struct ProcessUpdateParams {
    pub environment_id: String,
    pub name: String,
    pub status: Option<String>,
    pub pid: Option<i64>,
    pub command: Option<String>,
    pub working_dir: Option<String>,
}

impl ProcessUpdateParams {
    pub fn new(environment_id: &str, name: &str) -> Self {
        Self {
            environment_id: environment_id.to_string(),
            name: name.to_string(),
            status: None,
            pid: None,
            command: None,
            working_dir: None,
        }
    }

    pub fn with_status(mut self, status: &str) -> Self {
        self.status = Some(status.to_string());
        self
    }

    pub fn with_pid(mut self, pid: i64) -> Self {
        self.pid = Some(pid);
        self
    }

    pub fn with_command(mut self, command: &str) -> Self {
        self.command = Some(command.to_string());
        self
    }

    pub fn with_working_dir(mut self, working_dir: &str) -> Self {
        self.working_dir = Some(working_dir.to_string());
        self
    }
}

/// Parameters for database queries with common filters
#[derive(Debug, Clone)]
pub struct QueryParams {
    pub environment_id: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl QueryParams {
    pub fn new() -> Self {
        Self {
            environment_id: None,
            status: None,
            limit: None,
            offset: None,
        }
    }

    pub fn with_environment(mut self, environment_id: &str) -> Self {
        self.environment_id = Some(environment_id.to_string());
        self
    }

    pub fn with_status(mut self, status: &str) -> Self {
        self.status = Some(status.to_string());
        self
    }

    pub fn with_limit(mut self, limit: i64) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn with_offset(mut self, offset: i64) -> Self {
        self.offset = Some(offset);
        self
    }
}

impl Default for QueryParams {
    fn default() -> Self {
        Self::new()
    }
}

use std::future::Future;

/// Common database operations trait
pub trait DatabaseOperations {
    /// Update a record with automatic timestamp
    fn update_with_timestamp(
        &self,
        table: &str,
        set_clause: &str,
        where_clause: &str,
        params: Vec<String>,
    ) -> impl Future<Output = Result<u64>> + Send;

    /// Get a record by composite key (environment_id:name format)
    fn get_by_composite_key(
        &self,
        table: &str,
        key: &str,
    ) -> impl Future<Output = Result<Option<sqlx::sqlite::SqliteRow>>> + Send;
}
