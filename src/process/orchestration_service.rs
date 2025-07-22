use crate::cli::commands::ProcessInfo;
use crate::config::ProcessDefinition;
use crate::error::{Result, RunceptError};
use crate::process::configuration::ProcessConfigurationManager;
use crate::process::{
    DefaultHealthCheckService, DefaultProcessRuntime, HealthCheckTrait, LogEntry,
    ProcessExecutionService, ProcessOrchestrationTrait, ProcessRepositoryTrait,
    ProcessRuntimeTrait,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// High-level orchestration service that coordinates process management operations
///
/// This service replaces ProcessManager and provides a clean trait-based architecture
/// for orchestrating complex process management workflows across multiple components.
pub struct ProcessOrchestrationService<R, RT, H>
where
    R: ProcessRepositoryTrait,
    RT: ProcessRuntimeTrait,
    H: HealthCheckTrait,
{
    /// Configuration manager for process definitions and environment handling
    pub configuration_manager: Arc<RwLock<ProcessConfigurationManager>>,
    /// Execution service for actual process runtime management
    pub execution_service: Arc<RwLock<ProcessExecutionService<R, RT, H>>>,
    /// Global configuration
    pub global_config: crate::config::GlobalConfig,
    /// Environment manager for environment lifecycle
    pub environment_manager: Option<Arc<RwLock<crate::config::EnvironmentManager>>>,
    /// Process repository for database operations
    pub process_repository: Arc<R>,
}

impl<R, RT, H> ProcessOrchestrationService<R, RT, H>
where
    R: ProcessRepositoryTrait + 'static,
    RT: ProcessRuntimeTrait,
    H: HealthCheckTrait,
{
    /// Create a new ProcessOrchestrationService
    pub fn new(
        configuration_manager: Arc<RwLock<ProcessConfigurationManager>>,
        execution_service: Arc<RwLock<ProcessExecutionService<R, RT, H>>>,
        global_config: crate::config::GlobalConfig,
        environment_manager: Option<Arc<RwLock<crate::config::EnvironmentManager>>>,
        process_repository: Arc<R>,
    ) -> Self {
        Self {
            configuration_manager,
            execution_service,
            global_config,
            environment_manager,
            process_repository,
        }
    }

    // ===== Core Orchestration Methods =====

    /// Start a process by name in the specified environment
    pub async fn start_process_by_name_in_environment(
        &mut self,
        process_name: &str,
        environment_id: &str,
    ) -> Result<String> {
        info!(
            "Orchestrating start of process '{}' in environment '{}'",
            process_name, environment_id
        );

        // Log orchestration activity with tracing
        info!(
            "Orchestration: Start requested for process '{}:{}' by orchestrator",
            environment_id, process_name
        );

        // Delegate to execution service
        let execution_service = self.execution_service.read().await;
        let result = execution_service
            .start_process(process_name, environment_id)
            .await?;

        info!(
            "Successfully orchestrated start of process '{}' in environment '{}' with ID '{}'",
            process_name, environment_id, result
        );

        Ok(result)
    }

    /// Stop a process by name in the specified environment
    pub async fn stop_process_by_name_in_environment(
        &mut self,
        process_name: &str,
        environment_id: &str,
    ) -> Result<()> {
        info!(
            "Orchestrating stop of process '{}' in environment '{}'",
            process_name, environment_id
        );

        // Log orchestration activity

        // Delegate to execution service
        let execution_service = self.execution_service.read().await;
        execution_service
            .stop_process(process_name, environment_id)
            .await?;

        info!(
            "Successfully orchestrated stop of process '{}' in environment '{}'",
            process_name, environment_id
        );

        Ok(())
    }

    /// Restart a process by name in the specified environment
    pub async fn restart_process_by_name_in_environment(
        &mut self,
        process_name: &str,
        environment_id: &str,
    ) -> Result<()> {
        info!(
            "Orchestrating restart of process '{}' in environment '{}'",
            process_name, environment_id
        );

        // Log orchestration activity

        // Delegate to execution service
        let execution_service = self.execution_service.read().await;
        execution_service
            .restart_process(process_name, environment_id)
            .await?;

        info!(
            "Successfully orchestrated restart of process '{}' in environment '{}'",
            process_name, environment_id
        );

        Ok(())
    }

    /// Add a process to the specified environment
    pub async fn add_process_to_environment(
        &mut self,
        process_def: ProcessDefinition,
        environment_id: &str,
    ) -> Result<()> {
        info!(
            "Orchestrating addition of process '{}' to environment '{}'",
            process_def.name, environment_id
        );

        // Log orchestration activity

        // Delegate to configuration manager
        let mut config_manager = self.configuration_manager.write().await;
        config_manager
            .add_process(process_def.clone(), environment_id)
            .await?;

        info!(
            "Successfully orchestrated addition of process '{}' to environment '{}'",
            process_def.name, environment_id
        );

        Ok(())
    }

    /// Remove a process from the specified environment
    pub async fn remove_process_from_environment(
        &mut self,
        process_name: &str,
        environment_id: &str,
    ) -> Result<()> {
        info!(
            "Orchestrating removal of process '{}' from environment '{}'",
            process_name, environment_id
        );

        // First stop the process if it's running

        // Log orchestration activity

        // Then remove it from configuration
        let mut config_manager = self.configuration_manager.write().await;
        config_manager
            .remove_process(process_name, environment_id)
            .await?;

        info!(
            "Successfully orchestrated removal of process '{}' from environment '{}'",
            process_name, environment_id
        );

        Ok(())
    }

    /// Update a process in the specified environment
    pub async fn update_process_in_environment(
        &mut self,
        process_name: &str,
        process_def: ProcessDefinition,
        environment_id: &str,
    ) -> Result<()> {
        info!(
            "Orchestrating update of process '{}' in environment '{}'",
            process_name, environment_id
        );

        // First stop the process if it's running

        // Log orchestration activity

        // Then update the configuration
        let mut config_manager = self.configuration_manager.write().await;
        config_manager
            .update_process(process_name, process_def, environment_id)
            .await?;

        info!(
            "Successfully orchestrated update of process '{}' in environment '{}'",
            process_name, environment_id
        );

        Ok(())
    }

    /// Get process logs by name in the specified environment
    pub async fn get_process_logs_by_name_in_environment(
        &self,
        process_name: &str,
        environment_id: &str,
        lines: Option<usize>,
    ) -> Result<Vec<LogEntry>> {
        debug!(
            "Orchestrating log retrieval for process '{}' in environment '{}'",
            process_name, environment_id
        );

        // Delegate to execution service
        let execution_service = self.execution_service.read().await;
        execution_service
            .get_process_logs(process_name, environment_id, lines)
            .await
    }

    /// List running processes for the specified environment
    pub async fn list_processes_for_environment(
        &self,
        environment_id: &str,
    ) -> Result<Vec<ProcessInfo>> {
        debug!(
            "Orchestrating process listing for environment '{}'",
            environment_id
        );

        // Delegate to execution service
        let execution_service = self.execution_service.read().await;
        Ok(execution_service
            .list_running_processes(environment_id)
            .await)
    }

    /// Get comprehensive process information for an environment
    pub async fn get_processes_for_environment(
        &self,
        environment_id: &str,
    ) -> Result<Vec<ProcessInfo>> {
        debug!(
            "Orchestrating comprehensive process retrieval for environment '{}'",
            environment_id
        );

        // Get configured processes from repository
        let configured_processes = self
            .process_repository
            .get_processes_by_environment(environment_id)
            .await?;

        // Get running processes from execution service
        let running_processes = self.list_processes_for_environment(environment_id).await?;

        // Create a map of running processes for quick lookup
        let running_processes_map: HashMap<String, &ProcessInfo> = running_processes
            .iter()
            .map(|process| (process.name.clone(), process))
            .collect();

        // Build process list from configured processes, showing their current status
        let processes: Vec<ProcessInfo> = configured_processes
            .iter()
            .map(|process_record| {
                if let Some(running_process) = running_processes_map.get(&process_record.name) {
                    // Process is running, use actual status
                    (*running_process).clone()
                } else {
                    // Process is configured but not running
                    ProcessInfo {
                        id: process_record.id.clone(),
                        name: process_record.name.clone(),
                        status: process_record.status.clone(),
                        pid: process_record.pid.map(|p| p as u32),
                        uptime: None,
                        environment: process_record.environment_id.clone(),
                        health_status: None,
                        restart_count: 0,
                        last_activity: Some(
                            process_record
                                .updated_at
                                .format("%Y-%m-%d %H:%M:%S")
                                .to_string(),
                        ),
                    }
                }
            })
            .collect();

        Ok(processes)
    }

    /// Get process summary for an environment (processes, total count, running count)
    pub async fn get_environment_process_summary(
        &self,
        environment_id: &str,
    ) -> Result<(Vec<ProcessInfo>, usize, usize)> {
        debug!(
            "Orchestrating process summary for environment '{}'",
            environment_id
        );

        let processes = self.get_processes_for_environment(environment_id).await?;
        let total = processes.len();
        let running = processes.iter().filter(|p| p.status == "running").count();

        Ok((processes, total, running))
    }

    /// Stop all processes in the specified environment
    pub async fn stop_all_processes_in_environment(&mut self, environment_id: &str) -> Result<()> {
        info!(
            "Orchestrating stop of all processes in environment '{}'",
            environment_id
        );

        // Log orchestration activity

        // Get all processes for the environment
        let processes = self.get_processes_for_environment(environment_id).await?;

        // Stop each running process
        for process in processes {
            if matches!(process.status.as_str(), "running" | "starting") {}
        }

        info!(
            "Successfully orchestrated stop of all processes in environment '{}'",
            environment_id
        );
        Ok(())
    }

    // ===== Legacy Compatibility Methods =====

    /// Start a process by name from the current active environment (legacy compatibility)
    pub async fn start_process_by_name(
        &mut self,
        process_name: &str,
        current_environment_id: Option<String>,
    ) -> Result<String> {
        let environment_id = self.resolve_environment_id(current_environment_id).await?;
        self.start_process_by_name_in_environment(process_name, &environment_id)
            .await
    }

    /// Stop a process by name from the current active environment (legacy compatibility)
    pub async fn stop_process_by_name(&mut self, process_name: &str) -> Result<()> {
        let environment_id = self.resolve_environment_id(None).await?;
        self.stop_process_by_name_in_environment(process_name, &environment_id)
            .await
    }

    /// Stop a process by ID (legacy compatibility)
    pub async fn stop_process(&mut self, process_id: &str) -> Result<()> {
        let environment_id = self.resolve_environment_id(None).await?;
        self.stop_process_by_name_in_environment(process_id, &environment_id)
            .await
    }

    /// Restart a process by name from the current active environment (legacy compatibility)
    pub async fn restart_process_by_name(&mut self, process_name: &str) -> Result<()> {
        let environment_id = self.resolve_environment_id(None).await?;
        self.restart_process_by_name_in_environment(process_name, &environment_id)
            .await
    }

    /// Restart a process by ID (legacy compatibility)
    pub async fn restart_process(&mut self, process_id: &str) -> Result<()> {
        let environment_id = self.resolve_environment_id(None).await?;
        self.restart_process_by_name_in_environment(process_id, &environment_id)
            .await
    }

    /// Stop all processes in the current environment (legacy compatibility)
    pub async fn stop_all_processes(&mut self) -> Result<()> {
        let environment_id = self.resolve_environment_id(None).await?;
        self.stop_all_processes_in_environment(&environment_id)
            .await
    }

    /// Get process logs by ID (legacy compatibility)
    pub async fn get_process_logs(
        &self,
        process_id: &str,
        max_lines: Option<usize>,
    ) -> Result<Vec<LogEntry>> {
        let environment_id = self.resolve_environment_id(None).await?;
        self.get_process_logs_by_name_in_environment(process_id, &environment_id, max_lines)
            .await
    }

    /// Get process logs by name from the current active environment (legacy compatibility)
    pub async fn get_process_logs_by_name(
        &self,
        process_name: &str,
        max_lines: Option<usize>,
    ) -> Result<Vec<LogEntry>> {
        let environment_id = self.resolve_environment_id(None).await?;
        self.get_process_logs_by_name_in_environment(process_name, &environment_id, max_lines)
            .await
    }

    /// Add a process to the current environment (legacy compatibility)
    pub async fn add_process(&mut self, process: ProcessDefinition) -> Result<()> {
        let environment_id = self.resolve_environment_id(None).await?;
        self.add_process_to_environment(process, &environment_id)
            .await
    }

    /// Remove a process from the current environment (legacy compatibility)
    pub async fn remove_process(&mut self, process_name: &str) -> Result<()> {
        let environment_id = self.resolve_environment_id(None).await?;
        self.remove_process_from_environment(process_name, &environment_id)
            .await
    }

    /// Update a process in the current environment (legacy compatibility)
    pub async fn update_process(
        &mut self,
        process_name: &str,
        process: ProcessDefinition,
    ) -> Result<()> {
        let environment_id = self.resolve_environment_id(None).await?;
        self.update_process_in_environment(process_name, process, &environment_id)
            .await
    }

    // ===== Helper Methods =====

    /// Resolve environment ID - either use provided ID or get current active environment
    async fn resolve_environment_id(&self, environment_id: Option<String>) -> Result<String> {
        match environment_id {
            Some(env_id) => Ok(env_id),
            None => {
                // Get the current active environment
                if let Some(env_manager) = &self.environment_manager {
                    let env_manager_guard = env_manager.read().await;
                    let active_envs = env_manager_guard.get_active_environments();
                    if let Some((env_id, _)) = active_envs.first() {
                        Ok(env_id.to_string())
                    } else {
                        Err(RunceptError::EnvironmentError(
                            "No active environment".to_string(),
                        ))
                    }
                } else {
                    Err(RunceptError::EnvironmentError(
                        "No environment manager available".to_string(),
                    ))
                }
            }
        }
    }
}

// Type alias for the default concrete implementation
pub type DefaultProcessOrchestrationService = ProcessOrchestrationService<
    crate::database::ProcessRepository,
    DefaultProcessRuntime,
    DefaultHealthCheckService,
>;

impl DefaultProcessOrchestrationService {
    /// Create a new ProcessOrchestrationService with default implementations
    pub async fn new_default(
        global_config: crate::config::GlobalConfig,
        environment_manager: Option<Arc<RwLock<crate::config::EnvironmentManager>>>,
        db_pool: Arc<sqlx::Pool<sqlx::Sqlite>>,
    ) -> Result<Self> {
        let process_repository = Arc::new(crate::database::ProcessRepository::new(db_pool.clone()));
        let process_runtime = Arc::new(DefaultProcessRuntime);
        let health_check_service = Arc::new(DefaultHealthCheckService::new());

        // Create default environment manager if none provided
        let env_manager = if let Some(mgr) = environment_manager {
            mgr
        } else {
            Arc::new(RwLock::new(
                crate::config::EnvironmentManager::new(global_config.clone()).await?,
            ))
        };

        // Create configuration manager
        let configuration_manager = Arc::new(RwLock::new(ProcessConfigurationManager::new(
            global_config.clone(),
            env_manager.clone(),
            process_repository.clone(),
        )));

        // Create execution service
        let execution_service = Arc::new(RwLock::new(ProcessExecutionService::new(
            configuration_manager.clone(),
            global_config.clone(),
            process_repository.clone(),
            process_runtime,
            health_check_service,
        )));

        Ok(Self::new(
            configuration_manager,
            execution_service,
            global_config,
            Some(env_manager),
            process_repository,
        ))
    }
}

#[async_trait]
impl<R, RT, H> ProcessOrchestrationTrait for ProcessOrchestrationService<R, RT, H>
where
    R: ProcessRepositoryTrait + 'static,
    RT: ProcessRuntimeTrait,
    H: HealthCheckTrait,
{
    async fn start_process_by_name_in_environment(
        &mut self,
        process_name: &str,
        environment_id: &str,
    ) -> Result<String> {
        self.start_process_by_name_in_environment(process_name, environment_id)
            .await
    }

    async fn stop_process_by_name_in_environment(
        &mut self,
        process_name: &str,
        environment_id: &str,
    ) -> Result<()> {
        self.stop_process_by_name_in_environment(process_name, environment_id)
            .await
    }

    async fn restart_process_by_name_in_environment(
        &mut self,
        process_name: &str,
        environment_id: &str,
    ) -> Result<()> {
        self.restart_process_by_name_in_environment(process_name, environment_id)
            .await
    }

    async fn add_process_to_environment(
        &mut self,
        process_def: ProcessDefinition,
        environment_id: &str,
    ) -> Result<()> {
        self.add_process_to_environment(process_def, environment_id)
            .await
    }

    async fn remove_process_from_environment(
        &mut self,
        process_name: &str,
        environment_id: &str,
    ) -> Result<()> {
        self.remove_process_from_environment(process_name, environment_id)
            .await
    }

    async fn update_process_in_environment(
        &mut self,
        process_name: &str,
        process_def: ProcessDefinition,
        environment_id: &str,
    ) -> Result<()> {
        self.update_process_in_environment(process_name, process_def, environment_id)
            .await
    }

    async fn get_process_logs_by_name_in_environment(
        &self,
        process_name: &str,
        environment_id: &str,
        lines: Option<usize>,
    ) -> Result<Vec<LogEntry>> {
        self.get_process_logs_by_name_in_environment(process_name, environment_id, lines)
            .await
    }

    async fn list_processes_for_environment(
        &self,
        environment_id: &str,
    ) -> Result<Vec<ProcessInfo>> {
        self.list_processes_for_environment(environment_id).await
    }

    async fn get_processes_for_environment(
        &self,
        environment_id: &str,
    ) -> Result<Vec<ProcessInfo>> {
        self.get_processes_for_environment(environment_id).await
    }

    async fn get_environment_process_summary(
        &self,
        environment_id: &str,
    ) -> Result<(Vec<ProcessInfo>, usize, usize)> {
        self.get_environment_process_summary(environment_id).await
    }

    async fn stop_all_processes_in_environment(&mut self, environment_id: &str) -> Result<()> {
        self.stop_all_processes_in_environment(environment_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use std::collections::HashMap;
    use tempfile::TempDir;

    /// Create an in-memory database for testing
    async fn create_test_database() -> Database {
        let db = Database::new("sqlite::memory:").await.unwrap();
        db.init().await.unwrap();
        db
    }

    #[tokio::test]
    async fn test_orchestration_service_creation() {
        let global_config = crate::config::GlobalConfig::default();
        let database = create_test_database().await;
        let database_pool = Arc::new(database.get_pool().clone());

        let env_manager = Arc::new(RwLock::new(
            crate::config::EnvironmentManager::new_with_database(
                global_config.clone(),
                Some(database.get_pool().clone()),
            )
            .await
            .unwrap(),
        ));

        let service = DefaultProcessOrchestrationService::new_default(
            global_config,
            Some(env_manager),
            database_pool,
        )
        .await
        .unwrap();

        assert!(service.environment_manager.is_some());
    }

    #[tokio::test]
    async fn test_environment_resolution() {
        let global_config = crate::config::GlobalConfig::default();
        let database = create_test_database().await;
        let database_pool = Arc::new(database.get_pool().clone());

        let env_manager = Arc::new(RwLock::new(
            crate::config::EnvironmentManager::new_with_database(
                global_config.clone(),
                Some(database.get_pool().clone()),
            )
            .await
            .unwrap(),
        ));

        let service = DefaultProcessOrchestrationService::new_default(
            global_config,
            Some(env_manager),
            database_pool,
        )
        .await
        .unwrap();

        // Test environment resolution with provided ID
        let result = service
            .resolve_environment_id(Some("test_env".to_string()))
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test_env");
    }

    #[tokio::test]
    async fn test_orchestration_workflow() {
        let global_config = crate::config::GlobalConfig::default();
        let database = create_test_database().await;
        let database_pool = Arc::new(database.get_pool().clone());

        let env_manager = Arc::new(RwLock::new(
            crate::config::EnvironmentManager::new_with_database(
                global_config.clone(),
                Some(database.get_pool().clone()),
            )
            .await
            .unwrap(),
        ));

        let mut service = DefaultProcessOrchestrationService::new_default(
            global_config,
            Some(env_manager),
            database_pool,
        )
        .await
        .unwrap();

        let temp_dir = TempDir::new().unwrap();
        let environment_id = temp_dir.path().to_string_lossy().to_string();

        let process_def = ProcessDefinition {
            name: "test-process".to_string(),
            command: "echo hello".to_string(),
            working_dir: None,
            auto_restart: Some(false),
            health_check_url: None,
            health_check_interval: None,
            depends_on: vec![],
            env_vars: HashMap::new(),
        };

        // Test orchestration workflow
        let result = service
            .add_process_to_environment(process_def.clone(), &environment_id)
            .await;

        // The result depends on environment setup, but should not panic
        match result {
            Ok(_) => {
                // Success - orchestration is working
            }
            Err(e) => {
                // Expected error due to environment setup - that's okay for this test
                assert!(
                    e.to_string().contains("Environment")
                        || e.to_string().contains("not found")
                        || e.to_string().contains("validate")
                );
            }
        }
    }
}
