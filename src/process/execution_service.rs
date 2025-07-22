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