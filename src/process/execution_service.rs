use crate::cli::commands::ProcessInfo;
use crate::error::Result;
use crate::process::configuration::ProcessConfigurationManager;
use crate::process::{
    DefaultHealthCheckService, DefaultProcessRuntime, HealthCheckTrait, LogEntry, 
    ProcessLifecycleManager, ProcessMonitoringService, ProcessRepositoryTrait, ProcessRuntimeTrait,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

/// Simplified execution service that coordinates between smaller focused services
pub struct ProcessExecutionService<R, RT, H>
where
    R: ProcessRepositoryTrait,
    RT: ProcessRuntimeTrait,
    H: HealthCheckTrait,
{
    /// Lifecycle manager for start/stop/restart operations
    pub lifecycle_manager: Arc<RwLock<ProcessLifecycleManager<R, RT, H>>>,
    /// Monitoring service for exit detection and status updates
    pub monitoring_service: Arc<ProcessMonitoringService<R>>,
    /// Reference to configuration manager
    pub process_configuration_manager: Arc<RwLock<ProcessConfigurationManager>>,
    /// Global configuration
    pub global_config: crate::config::GlobalConfig,
    /// Process repository for database operations
    pub process_repository: Arc<R>,
}

impl<R, RT, H> ProcessExecutionService<R, RT, H>
where
    R: ProcessRepositoryTrait + 'static,
    RT: ProcessRuntimeTrait,
    H: HealthCheckTrait,
{
    pub fn new(
        process_configuration_manager: Arc<RwLock<ProcessConfigurationManager>>,
        global_config: crate::config::GlobalConfig,
        process_repository: Arc<R>,
        process_runtime: Arc<RT>,
        health_check_service: Arc<H>,
    ) -> Self {
        // Create monitoring service
        let monitoring_service = Arc::new(ProcessMonitoringService::new(process_repository.clone()));

        // Create lifecycle manager
        let lifecycle_manager = Arc::new(RwLock::new(ProcessLifecycleManager::new(
            process_configuration_manager.clone(),
            global_config.clone(),
            process_repository.clone(),
            process_runtime,
            health_check_service,
            monitoring_service.clone(),
        )));

        Self {
            lifecycle_manager,
            monitoring_service,
            process_configuration_manager,
            global_config,
            process_repository,
        }
    }

    /// Start a process in the specified environment
    pub async fn start_process(&self, name: &str, environment_id: &str) -> Result<String> {
        let mut lifecycle_manager = self.lifecycle_manager.write().await;
        lifecycle_manager.start_process(name, environment_id).await
    }

    /// Stop a process in the specified environment
    pub async fn stop_process(&self, name: &str, environment_id: &str) -> Result<()> {
        let mut lifecycle_manager = self.lifecycle_manager.write().await;
        lifecycle_manager.stop_process(name, environment_id).await
    }

    /// Restart a process in the specified environment
    pub async fn restart_process(&self, name: &str, environment_id: &str) -> Result<()> {
        let mut lifecycle_manager = self.lifecycle_manager.write().await;
        lifecycle_manager.restart_process(name, environment_id).await
    }

    /// List running processes in the specified environment
    pub async fn list_running_processes(&self, environment_id: &str) -> Vec<ProcessInfo> {
        debug!(
            "Listing running processes in environment '{}'",
            environment_id
        );

        // Query processes from database via repository trait
        match self
            .process_repository
            .get_processes_by_environment(environment_id)
            .await
        {
            Ok(processes) => processes
                .into_iter()
                .map(|process| {
                    ProcessInfo {
                        id: process.id.clone(),
                        name: process.name.clone(),
                        status: process.status.clone(),
                        pid: process.pid.map(|p| p as u32),
                        uptime: Some(process.created_at.format("%Y-%m-%d %H:%M:%S").to_string()),
                        environment: environment_id.to_string(),
                        health_status: None, // TODO: Implement health status tracking
                        restart_count: 0,    // TODO: Implement restart count tracking
                        last_activity: None, // TODO: Implement last activity tracking
                    }
                })
                .collect(),
            Err(e) => {
                tracing::error!(
                    "Failed to query processes for environment '{}': {}",
                    environment_id, e
                );
                Vec::new()
            }
        }
    }

    /// Get the status of a specific process in the specified environment
    pub async fn get_process_status(
        &self,
        name: &str,
        environment_id: &str,
    ) -> Option<crate::process::ProcessStatus> {
        debug!(
            "Getting status for process '{}' in environment '{}'",
            name, environment_id
        );

        match self
            .process_repository
            .get_process_by_name(environment_id, name)
            .await
        {
            Ok(Some(process)) => match process.status.as_str() {
                "running" => Some(crate::process::ProcessStatus::Running),
                "stopped" => Some(crate::process::ProcessStatus::Stopped),
                "starting" => Some(crate::process::ProcessStatus::Starting),
                "stopping" => Some(crate::process::ProcessStatus::Stopping),
                "failed" => Some(crate::process::ProcessStatus::Failed),
                "crashed" => Some(crate::process::ProcessStatus::Crashed),
                _ => Some(crate::process::ProcessStatus::Stopped),
            },
            Ok(None) => None,
            Err(e) => {
                tracing::error!(
                    "Failed to query process status for '{}:{}': {}",
                    environment_id, name, e
                );
                None
            }
        }
    }

    /// Get logs for a specific process in the specified environment
    pub async fn get_process_logs(
        &self,
        name: &str,
        environment_id: &str,
        lines: Option<usize>,
    ) -> Result<Vec<LogEntry>> {
        debug!(
            "Getting logs for process '{}' in environment '{}'",
            name, environment_id
        );

        if self
            .process_repository
            .get_process_by_name(environment_id, name)
            .await?
            .is_some()
        {
            // Check if process is running by looking at active handles in lifecycle manager
            let lifecycle_manager = self.lifecycle_manager.read().await;
            if let Some(env_handles) = lifecycle_manager.process_handles.get(environment_id) {
                if let Some(handle) = env_handles.get(name) {
                    let logger = handle.logger.lock().await;
                    return logger.read_logs(lines).await;
                }
            }

            // Process is not running, try to read logs from filesystem
            let working_dir = {
                let config_manager = self.process_configuration_manager.read().await;
                config_manager.get_working_directory(environment_id).await?
            };

            crate::process::read_process_logs(name, working_dir.as_path(), lines).await
        } else {
            Err(crate::error::RunceptError::ProcessError(format!(
                "Process '{name}' not found in environment '{environment_id}'"
            )))
        }
    }
}

// Type alias for the default concrete implementation
pub type DefaultProcessExecutionService = ProcessExecutionService<
    crate::database::ProcessRepository,
    DefaultProcessRuntime,
    DefaultHealthCheckService,
>;

impl DefaultProcessExecutionService {
    /// Create a new ProcessExecutionService with default implementations
    pub fn new_default(
        process_configuration_manager: Arc<RwLock<ProcessConfigurationManager>>,
        global_config: crate::config::GlobalConfig,
        db_pool: Arc<sqlx::Pool<sqlx::Sqlite>>,
    ) -> Self {
        let process_repository = Arc::new(crate::database::ProcessRepository::new(db_pool));
        let process_runtime = Arc::new(DefaultProcessRuntime);
        let health_check_service = Arc::new(DefaultHealthCheckService::new());

        Self::new(
            process_configuration_manager,
            global_config,
            process_repository,
            process_runtime,
            health_check_service,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::process_repository::ProcessRecord;
    use crate::error::Result;
    use crate::process::{HealthCheckConfig, HealthCheckResult, HealthCheckType, ProcessHandle};
    use async_trait::async_trait;
    use chrono::Utc;
    use std::collections::HashMap;
    use std::sync::Mutex as StdMutex;
    use tempfile::TempDir;

    // Mock repository implementation
    #[derive(Debug)]
    struct MockProcessRepository {
        processes: StdMutex<HashMap<String, ProcessRecord>>,
    }

    impl MockProcessRepository {
        fn new() -> Self {
            Self {
                processes: StdMutex::new(HashMap::new()),
            }
        }

        fn add_process(&self, process: ProcessRecord) {
            let key = format!("{}:{}", process.environment_id, process.name);
            self.processes.lock().unwrap().insert(key, process);
        }
    }

    #[async_trait]
    impl ProcessRepositoryTrait for MockProcessRepository {
        async fn insert_process(&self, _process: &crate::process::Process) -> Result<()> {
            Ok(())
        }

        async fn get_process_by_name(&self, environment_id: &str, name: &str) -> Result<Option<ProcessRecord>> {
            let key = format!("{environment_id}:{name}");
            Ok(self.processes.lock().unwrap().get(&key).cloned())
        }

        async fn get_process_by_name_validated(&self, environment_id: &str, name: &str) -> Result<Option<ProcessRecord>> {
            self.get_process_by_name(environment_id, name).await
        }

        async fn delete_process(&self, _environment_id: &str, _name: &str) -> Result<bool> {
            Ok(true)
        }

        async fn update_process_status(&self, _environment_id: &str, _name: &str, _status: &str) -> Result<()> {
            Ok(())
        }

        async fn get_running_processes(&self) -> Result<Vec<(String, String, Option<i64>)>> {
            Ok(Vec::new())
        }

        async fn get_processes_by_status(&self, _status: &str) -> Result<Vec<ProcessRecord>> {
            Ok(Vec::new())
        }

        async fn set_process_pid(&self, _environment_id: &str, _name: &str, _pid: i64) -> Result<()> {
            Ok(())
        }

        async fn clear_process_pid(&self, _environment_id: &str, _name: &str) -> Result<()> {
            Ok(())
        }

        async fn get_processes_by_environment(&self, environment_id: &str) -> Result<Vec<ProcessRecord>> {
            Ok(self.processes.lock().unwrap().values()
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

        async fn update_process_command(&self, _environment_id: &str, _name: &str, _command: &str) -> Result<()> {
            Ok(())
        }

        async fn update_process_working_dir(&self, _environment_id: &str, _name: &str, _working_dir: &str) -> Result<()> {
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

    // Mock runtime implementation
    struct MockProcessRuntime;

    #[async_trait]
    impl ProcessRuntimeTrait for MockProcessRuntime {
        async fn spawn_process(&self, _process: &crate::process::Process) -> Result<ProcessHandle> {
            let child = tokio::process::Command::new("echo")
                .arg("test")
                .spawn()
                .map_err(|e| crate::error::RunceptError::ProcessError(format!("Failed to spawn mock process: {}", e)))?;

            Ok(ProcessHandle {
                child,
                logger: None,
                shutdown_tx: None,
            })
        }
    }

    // Mock health check implementation
    struct MockHealthCheckService;

    #[async_trait]
    impl HealthCheckTrait for MockHealthCheckService {
        async fn execute_health_check(&self, _check: &HealthCheckConfig) -> Result<HealthCheckResult> {
            Ok(HealthCheckResult {
                check_type: HealthCheckType::Http {
                    url: "http://localhost:8080".to_string(),
                    expected_status: 200,
                },
                success: true,
                message: "Mock health check passed".to_string(),
                duration_ms: 10,
                timestamp: Utc::now(),
            })
        }

        async fn execute_health_checks(&self, checks: &[HealthCheckConfig]) -> Result<Vec<HealthCheckResult>> {
            let mut results = Vec::new();
            for check in checks {
                results.push(self.execute_health_check(check).await?);
            }
            Ok(results)
        }
    }

    #[tokio::test]
    async fn test_execution_service_creation() {
        let temp_dir = TempDir::new().unwrap();
        let global_config = crate::config::GlobalConfig::default();
        let mock_repo = Arc::new(MockProcessRepository::new());
        let mock_runtime = Arc::new(MockProcessRuntime);
        let mock_health = Arc::new(MockHealthCheckService);

        // Create a real ProcessRepository with in-memory database for configuration manager
        let temp_db = crate::database::Database::new("sqlite::memory:")
            .await
            .unwrap();
        temp_db.init().await.unwrap();
        let real_repo = Arc::new(crate::database::ProcessRepository::new(
            Arc::new(temp_db.get_pool().clone())
        ));

        let config_manager = Arc::new(RwLock::new(ProcessConfigurationManager::new(
            global_config.clone(),
            Arc::new(RwLock::new(
                crate::config::EnvironmentManager::new(global_config.clone())
                    .await
                    .unwrap(),
            )),
            real_repo,
        )));

        let service = ProcessExecutionService::new(
            config_manager,
            global_config,
            mock_repo,
            mock_runtime,
            mock_health,
        );

        // Should have created the lifecycle and monitoring services
        assert!(service.lifecycle_manager.read().await.process_handles.is_empty());
    }

    #[tokio::test]
    async fn test_list_running_processes_empty() {
        let temp_dir = TempDir::new().unwrap();
        let global_config = crate::config::GlobalConfig::default();
        let mock_repo = Arc::new(MockProcessRepository::new());
        let mock_runtime = Arc::new(MockProcessRuntime);
        let mock_health = Arc::new(MockHealthCheckService);

        // Create a real ProcessRepository with in-memory database for configuration manager
        let temp_db = crate::database::Database::new("sqlite::memory:")
            .await
            .unwrap();
        temp_db.init().await.unwrap();
        let real_repo = Arc::new(crate::database::ProcessRepository::new(
            Arc::new(temp_db.get_pool().clone())
        ));

        let config_manager = Arc::new(RwLock::new(ProcessConfigurationManager::new(
            global_config.clone(),
            Arc::new(RwLock::new(
                crate::config::EnvironmentManager::new(global_config.clone())
                    .await
                    .unwrap(),
            )),
            real_repo,
        )));

        let service = ProcessExecutionService::new(
            config_manager,
            global_config,
            mock_repo,
            mock_runtime,
            mock_health,
        );

        let processes = service.list_running_processes("test-env").await;
        assert!(processes.is_empty());
    }

    #[tokio::test]
    async fn test_list_running_processes_with_data() {
        let temp_dir = TempDir::new().unwrap();
        let global_config = crate::config::GlobalConfig::default();
        let mock_repo = Arc::new(MockProcessRepository::new());
        let mock_runtime = Arc::new(MockProcessRuntime);
        let mock_health = Arc::new(MockHealthCheckService);

        // Add a test process
        mock_repo.add_process(ProcessRecord {
            id: "test-env:test-process".to_string(),
            name: "test-process".to_string(),
            command: "echo hello".to_string(),
            working_dir: "/tmp".to_string(),
            environment_id: "test-env".to_string(),
            status: "running".to_string(),
            pid: Some(1234),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        });

        // Create a real ProcessRepository with in-memory database for configuration manager
        let temp_db = crate::database::Database::new("sqlite::memory:")
            .await
            .unwrap();
        temp_db.init().await.unwrap();
        let real_repo = Arc::new(crate::database::ProcessRepository::new(
            Arc::new(temp_db.get_pool().clone())
        ));

        let config_manager = Arc::new(RwLock::new(ProcessConfigurationManager::new(
            global_config.clone(),
            Arc::new(RwLock::new(
                crate::config::EnvironmentManager::new(global_config.clone())
                    .await
                    .unwrap(),
            )),
            real_repo,
        )));

        let service = ProcessExecutionService::new(
            config_manager,
            global_config,
            mock_repo,
            mock_runtime,
            mock_health,
        );

        let processes = service.list_running_processes("test-env").await;
        assert_eq!(processes.len(), 1);
        assert_eq!(processes[0].name, "test-process");
        assert_eq!(processes[0].status, "running");
    }

    #[tokio::test]
    async fn test_get_process_status() {
        let temp_dir = TempDir::new().unwrap();
        let global_config = crate::config::GlobalConfig::default();
        let mock_repo = Arc::new(MockProcessRepository::new());
        let mock_runtime = Arc::new(MockProcessRuntime);
        let mock_health = Arc::new(MockHealthCheckService);

        // Add a test process
        mock_repo.add_process(ProcessRecord {
            id: "test-env:test-process".to_string(),
            name: "test-process".to_string(),
            command: "echo hello".to_string(),
            working_dir: "/tmp".to_string(),
            environment_id: "test-env".to_string(),
            status: "running".to_string(),
            pid: Some(1234),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        });

        // Create a real ProcessRepository with in-memory database for configuration manager
        let temp_db = crate::database::Database::new("sqlite::memory:")
            .await
            .unwrap();
        temp_db.init().await.unwrap();
        let real_repo = Arc::new(crate::database::ProcessRepository::new(
            Arc::new(temp_db.get_pool().clone())
        ));

        let config_manager = Arc::new(RwLock::new(ProcessConfigurationManager::new(
            global_config.clone(),
            Arc::new(RwLock::new(
                crate::config::EnvironmentManager::new(global_config.clone())
                    .await
                    .unwrap(),
            )),
            real_repo,
        )));

        let service = ProcessExecutionService::new(
            config_manager,
            global_config,
            mock_repo,
            mock_runtime,
            mock_health,
        );

        let status = service.get_process_status("test-process", "test-env").await;
        assert!(status.is_some());
        assert!(matches!(status.unwrap(), crate::process::ProcessStatus::Running));

        // Test nonexistent process
        let status = service.get_process_status("nonexistent", "test-env").await;
        assert!(status.is_none());
    }
}