use crate::database::process_repository::ProcessRecord;
use crate::error::Result;
use crate::process::{
    HealthCheckConfig, HealthCheckResult, HealthCheckTrait, HealthCheckType, Process,
    ProcessHandle, ProcessRepositoryTrait, ProcessRuntimeTrait,
};
use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Mutex as StdMutex;

/// Shared mock repository implementation for testing
#[derive(Debug)]
pub struct MockProcessRepository {
    pub processes: StdMutex<HashMap<String, ProcessRecord>>,
    pub status_updates: StdMutex<Vec<(String, String, String)>>, // env_id, name, status
    pub pid_clears: StdMutex<Vec<(String, String)>>,             // env_id, name
    pub pid_sets: StdMutex<Vec<(String, String, i64)>>,          // env_id, name, pid
}

impl MockProcessRepository {
    pub fn new() -> Self {
        Self {
            processes: StdMutex::new(HashMap::new()),
            status_updates: StdMutex::new(Vec::new()),
            pid_clears: StdMutex::new(Vec::new()),
            pid_sets: StdMutex::new(Vec::new()),
        }
    }

    pub fn add_process(&self, process: ProcessRecord) {
        let key = format!("{}:{}", process.environment_id, process.name);
        self.processes.lock().unwrap().insert(key, process);
    }

    pub fn get_status_updates(&self) -> Vec<(String, String, String)> {
        self.status_updates.lock().unwrap().clone()
    }

    pub fn get_pid_clears(&self) -> Vec<(String, String)> {
        self.pid_clears.lock().unwrap().clone()
    }

    pub fn get_pid_sets(&self) -> Vec<(String, String, i64)> {
        self.pid_sets.lock().unwrap().clone()
    }

    pub fn clear_tracking(&self) {
        self.status_updates.lock().unwrap().clear();
        self.pid_clears.lock().unwrap().clear();
        self.pid_sets.lock().unwrap().clear();
    }
}

impl Default for MockProcessRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProcessRepositoryTrait for MockProcessRepository {
    async fn insert_process(&self, _process: &Process) -> Result<()> {
        Ok(())
    }

    async fn get_process_by_name(
        &self,
        environment_id: &str,
        name: &str,
    ) -> Result<Option<ProcessRecord>> {
        let key = format!("{environment_id}:{name}");
        Ok(self.processes.lock().unwrap().get(&key).cloned())
    }

    async fn get_process_by_name_validated(
        &self,
        environment_id: &str,
        name: &str,
    ) -> Result<Option<ProcessRecord>> {
        self.get_process_by_name(environment_id, name).await
    }

    async fn delete_process(&self, _environment_id: &str, _name: &str) -> Result<bool> {
        Ok(true)
    }

    async fn update_process_status(
        &self,
        environment_id: &str,
        name: &str,
        status: &str,
    ) -> Result<()> {
        self.status_updates.lock().unwrap().push((
            environment_id.to_string(),
            name.to_string(),
            status.to_string(),
        ));
        Ok(())
    }

    async fn get_running_processes(&self) -> Result<Vec<(String, String, Option<i64>)>> {
        Ok(Vec::new())
    }

    async fn get_processes_by_status(&self, _status: &str) -> Result<Vec<ProcessRecord>> {
        Ok(Vec::new())
    }

    async fn set_process_pid(&self, environment_id: &str, name: &str, pid: i64) -> Result<()> {
        self.pid_sets
            .lock()
            .unwrap()
            .push((environment_id.to_string(), name.to_string(), pid));
        Ok(())
    }

    async fn clear_process_pid(&self, environment_id: &str, name: &str) -> Result<()> {
        self.pid_clears
            .lock()
            .unwrap()
            .push((environment_id.to_string(), name.to_string()));
        Ok(())
    }

    async fn get_processes_by_environment(
        &self,
        environment_id: &str,
    ) -> Result<Vec<ProcessRecord>> {
        Ok(self
            .processes
            .lock()
            .unwrap()
            .values()
            .filter(|p| p.environment_id == environment_id)
            .cloned()
            .collect())
    }

    async fn validate_environment(&self, _environment_id: &str) -> Result<bool> {
        Ok(true)
    }

    async fn cleanup_processes_by_environment(&self, _environment_id: &str) -> Result<u64> {
        Ok(0)
    }

    async fn update_process_command(
        &self,
        _environment_id: &str,
        _name: &str,
        _command: &str,
    ) -> Result<()> {
        Ok(())
    }

    async fn update_process_working_dir(
        &self,
        _environment_id: &str,
        _name: &str,
        _working_dir: &str,
    ) -> Result<()> {
        Ok(())
    }

    async fn update_process_activity(&self, _environment_id: &str, _name: &str) -> Result<()> {
        Ok(())
    }

    async fn count_processes_by_environment(&self, _environment_id: &str) -> Result<i64> {
        Ok(0)
    }

    async fn count_processes_by_status(&self, _status: &str) -> Result<i64> {
        Ok(0)
    }
}

/// Mock runtime implementation for testing
pub struct MockProcessRuntime;

#[async_trait]
impl ProcessRuntimeTrait for MockProcessRuntime {
    async fn spawn_process(&self, _process: &Process) -> Result<ProcessHandle> {
        // Create a mock child process that exits quickly
        let child = tokio::process::Command::new("echo")
            .arg("test")
            .spawn()
            .map_err(|e| {
                crate::error::RunceptError::ProcessError(format!(
                    "Failed to spawn mock process: {e}"
                ))
            })?;

        Ok(ProcessHandle {
            child,
            logger: None,
            shutdown_tx: None,
        })
    }
}

/// Mock health check implementation for testing
pub struct MockHealthCheckService {
    pub should_succeed: bool,
}

impl MockHealthCheckService {
    pub fn new() -> Self {
        Self {
            should_succeed: true,
        }
    }

    pub fn with_success(mut self, should_succeed: bool) -> Self {
        self.should_succeed = should_succeed;
        self
    }
}

impl Default for MockHealthCheckService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl HealthCheckTrait for MockHealthCheckService {
    async fn execute_health_check(&self, _check: &HealthCheckConfig) -> Result<HealthCheckResult> {
        Ok(HealthCheckResult {
            check_type: HealthCheckType::Http {
                url: "http://localhost:8080".to_string(),
                expected_status: 200,
            },
            success: self.should_succeed,
            message: if self.should_succeed {
                "Mock health check passed".to_string()
            } else {
                "Mock health check failed".to_string()
            },
            duration_ms: 10,
            timestamp: Utc::now(),
        })
    }

    async fn execute_health_checks(
        &self,
        checks: &[HealthCheckConfig],
    ) -> Result<Vec<HealthCheckResult>> {
        let mut results = Vec::new();
        for check in checks {
            results.push(self.execute_health_check(check).await?);
        }
        Ok(results)
    }
}
