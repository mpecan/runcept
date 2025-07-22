use crate::database::process_repository::ProcessRecord;
use crate::error::Result;
use crate::process::Process;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Trait for process repository operations
///
/// This trait abstracts database operations for process management,
/// enabling testing with mock implementations and providing a clean
/// interface for process persistence.
#[async_trait]
pub trait ProcessRepositoryTrait: Send + Sync {
    // CRUD Operations

    /// Insert or update a process in the database (upsert operation)
    async fn insert_process(&self, process: &Process) -> Result<()>;

    /// Get a process by environment ID and name
    async fn get_process_by_name(
        &self,
        environment_id: &str,
        name: &str,
    ) -> Result<Option<ProcessRecord>>;

    /// Get a process by environment ID and name with environment validation
    async fn get_process_by_name_validated(
        &self,
        environment_id: &str,
        name: &str,
    ) -> Result<Option<ProcessRecord>>;

    /// Delete a process from the database
    async fn delete_process(&self, environment_id: &str, name: &str) -> Result<bool>;

    // Status Management

    /// Update process status in the database
    async fn update_process_status(
        &self,
        environment_id: &str,
        name: &str,
        status: &str,
    ) -> Result<()>;

    /// Get all processes marked as running or starting in the database
    async fn get_running_processes(&self) -> Result<Vec<(String, String, Option<i64>)>>;

    /// Get processes by status
    async fn get_processes_by_status(&self, status: &str) -> Result<Vec<ProcessRecord>>;

    // PID Management

    /// Set process PID in the database
    async fn set_process_pid(&self, environment_id: &str, name: &str, pid: i64) -> Result<()>;

    /// Clear process PID in the database
    async fn clear_process_pid(&self, environment_id: &str, name: &str) -> Result<()>;

    // Environment Operations

    /// Get all processes for a specific environment
    async fn get_processes_by_environment(
        &self,
        environment_id: &str,
    ) -> Result<Vec<ProcessRecord>>;

    /// Validate that an environment exists
    async fn validate_environment(&self, environment_id: &str) -> Result<bool>;

    /// Cleanup processes for a specific environment
    async fn cleanup_processes_by_environment(&self, environment_id: &str) -> Result<u64>;

    // Configuration Updates

    /// Update process command
    async fn update_process_command(
        &self,
        environment_id: &str,
        name: &str,
        command: &str,
    ) -> Result<()>;

    /// Update process working directory
    async fn update_process_working_dir(
        &self,
        environment_id: &str,
        name: &str,
        working_dir: &str,
    ) -> Result<()>;

    // Activity Management

    /// Update process last activity time
    async fn update_process_activity(&self, environment_id: &str, name: &str) -> Result<()>;

    // Analytics

    /// Count processes by environment
    async fn count_processes_by_environment(&self, environment_id: &str) -> Result<i64>;

    /// Count processes by status
    async fn count_processes_by_status(&self, status: &str) -> Result<i64>;
}

/// Trait for process runtime operations
///
/// This trait abstracts the actual process spawning operations,
/// separating business logic from system-level process handling.
/// Other process management operations are handled by the execution service.
#[async_trait]
pub trait ProcessRuntimeTrait: Send + Sync {
    /// Spawn a new process and return its handle
    async fn spawn_process(&self, process: &Process) -> Result<ProcessHandle>;
}

/// Trait for health check operations
///
/// This trait abstracts health check execution, enabling different
/// health check strategies and mock implementations for testing.
#[async_trait]
pub trait HealthCheckTrait: Send + Sync {
    /// Execute a health check for a process
    async fn execute_health_check(&self, check: &HealthCheckConfig) -> Result<HealthCheckResult>;

    /// Execute multiple health checks concurrently
    async fn execute_health_checks(
        &self,
        checks: &[HealthCheckConfig],
    ) -> Result<Vec<HealthCheckResult>>;
}

// Supporting types

/// Handle to a running process
///
/// Contains the actual system process handle and associated resources
/// that cannot be persisted to the database.
pub struct ProcessHandle {
    pub child: tokio::process::Child,
    pub logger: Option<crate::process::logging::ProcessLogger>,
    pub shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

/// Result of process exit
#[derive(Debug, Clone)]
pub struct ProcessExitResult {
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
    pub timestamp: DateTime<Utc>,
}

/// Configuration for a health check
#[derive(Debug, Clone)]
pub struct HealthCheckConfig {
    pub check_type: HealthCheckType,
    pub timeout_secs: u64,
    pub retries: u32,
}

/// Type of health check to perform
#[derive(Debug, Clone)]
pub enum HealthCheckType {
    Http {
        url: String,
        expected_status: u16,
    },
    Tcp {
        host: String,
        port: u16,
    },
    Command {
        command: String,
        expected_exit_code: i32,
    },
}

/// Result of a health check
#[derive(Debug, Clone)]
pub struct HealthCheckResult {
    pub check_type: HealthCheckType,
    pub success: bool,
    pub message: String,
    pub duration_ms: u64,
    pub timestamp: DateTime<Utc>,
}
