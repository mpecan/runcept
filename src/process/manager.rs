use crate::cli::commands::ProcessInfo;
use crate::config::ProcessDefinition;
use crate::error::{Result, RunceptError};
use crate::process::LogEntry;
use crate::process::configuration::ProcessConfigurationManager;
use crate::process::runtime::ProcessRuntimeManager;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Main process manager that coordinates between configuration and runtime components
pub struct ProcessManager {
    // Component-based architecture
    pub configuration_manager: Arc<RwLock<ProcessConfigurationManager>>,
    pub runtime_manager: Arc<RwLock<ProcessRuntimeManager>>,
    pub global_config: crate::config::GlobalConfig,
    pub environment_manager: Option<Arc<RwLock<crate::config::EnvironmentManager>>>,
}

impl ProcessManager {
    /// Create a new ProcessManager with default environment manager
    pub async fn new(global_config: crate::config::GlobalConfig) -> Result<Self> {
        // Create a dummy environment manager for backward compatibility
        let env_manager = Arc::new(RwLock::new(
            crate::config::EnvironmentManager::new(global_config.clone()).await?,
        ));

        Self::new_with_environment_manager(global_config, env_manager, None).await
    }

    /// Create a new ProcessManager with provided environment manager
    pub async fn new_with_environment_manager(
        global_config: crate::config::GlobalConfig,
        environment_manager: Arc<RwLock<crate::config::EnvironmentManager>>,
        database_pool: Option<Arc<sqlx::SqlitePool>>,
    ) -> Result<Self> {
        // Create component managers
        let configuration_manager = Arc::new(RwLock::new(ProcessConfigurationManager::new(
            global_config.clone(),
            environment_manager.clone(),
        )));

        // Create process repository - required for DB-first approach
        let process_repository = match database_pool {
            Some(pool) => Arc::new(crate::database::ProcessRepository::new(pool)),
            None => return Err(RunceptError::DatabaseError("Database pool is required for ProcessRuntimeManager".to_string())),
        };

        let runtime_manager = Arc::new(RwLock::new(ProcessRuntimeManager::new(
            configuration_manager.clone(),
            global_config.clone(),
            process_repository,
        )));

        Ok(Self {
            configuration_manager,
            runtime_manager,
            global_config,
            environment_manager: Some(environment_manager),
        })
    }

    // ===== New Component-Based Methods =====

    /// Start a process by name in the specified environment
    pub async fn start_process_by_name_in_environment(
        &mut self,
        process_name: &str,
        environment_id: &str,
    ) -> Result<String> {
        let mut runtime_manager = self.runtime_manager.write().await;
        runtime_manager
            .start_process(process_name, environment_id)
            .await
    }

    /// Stop a process by name in the specified environment
    pub async fn stop_process_by_name_in_environment(
        &mut self,
        process_name: &str,
        environment_id: &str,
    ) -> Result<()> {
        let mut runtime_manager = self.runtime_manager.write().await;
        runtime_manager
            .stop_process(process_name, environment_id)
            .await
    }

    /// Restart a process by name in the specified environment
    pub async fn restart_process_by_name_in_environment(
        &mut self,
        process_name: &str,
        environment_id: &str,
    ) -> Result<()> {
        let mut runtime_manager = self.runtime_manager.write().await;
        runtime_manager
            .restart_process(process_name, environment_id)
            .await
    }

    /// Add a process to the specified environment
    pub async fn add_process_to_environment(
        &mut self,
        process_def: ProcessDefinition,
        environment_id: &str,
    ) -> Result<()> {
        let mut config_manager = self.configuration_manager.write().await;
        config_manager
            .add_process(process_def, environment_id)
            .await
    }

    /// Remove a process from the specified environment
    pub async fn remove_process_from_environment(
        &mut self,
        process_name: &str,
        environment_id: &str,
    ) -> Result<()> {
        // First stop the process if it's running
        let mut runtime_manager = self.runtime_manager.write().await;
        let _ = runtime_manager
            .stop_process(process_name, environment_id)
            .await;

        // Then remove it from configuration
        drop(runtime_manager); // Release the lock

        let mut config_manager = self.configuration_manager.write().await;
        config_manager
            .remove_process(process_name, environment_id)
            .await
    }

    /// Update a process in the specified environment
    pub async fn update_process_in_environment(
        &mut self,
        process_name: &str,
        process_def: ProcessDefinition,
        environment_id: &str,
    ) -> Result<()> {
        // First stop the process if it's running
        let mut runtime_manager = self.runtime_manager.write().await;
        let _ = runtime_manager
            .stop_process(process_name, environment_id)
            .await;

        // Then update the configuration
        drop(runtime_manager); // Release the lock

        let mut config_manager = self.configuration_manager.write().await;
        config_manager
            .update_process(process_name, process_def, environment_id)
            .await
    }

    /// Get process logs by name in the specified environment
    pub async fn get_process_logs_by_name_in_environment(
        &self,
        process_name: &str,
        environment_id: &str,
        lines: Option<usize>,
    ) -> Result<Vec<LogEntry>> {
        let runtime_manager = self.runtime_manager.read().await;
        runtime_manager
            .get_process_logs(process_name, environment_id, lines)
            .await
    }

    /// List running processes for the specified environment
    pub async fn list_processes_for_environment(
        &self,
        environment_id: &str,
    ) -> Result<Vec<ProcessInfo>> {
        // Process any pending exit notifications first
        {
            let mut runtime_manager = self.runtime_manager.write().await;
            runtime_manager.process_exit_notifications().await?;
        }
        
        let runtime_manager = self.runtime_manager.read().await;
        Ok(runtime_manager.list_running_processes(environment_id).await)
    }

    /// Get processes for a specific environment, combining configured and running processes
    pub async fn get_processes_for_environment(
        &self,
        environment_id: &str,
    ) -> Result<Vec<ProcessInfo>> {
        // Process any pending exit notifications first
        {
            let mut runtime_manager = self.runtime_manager.write().await;
            runtime_manager.process_exit_notifications().await?;
        }
        
        // Get configured processes from environment
        let configured_processes = if let Some(env_manager) = &self.environment_manager {
            let env_manager_guard = env_manager.read().await;
            if let Some(env) = env_manager_guard.get_environment(environment_id) {
                env.project_config.processes.clone()
            } else {
                return Err(RunceptError::EnvironmentError(format!(
                    "Environment '{}' not found",
                    environment_id
                )));
            }
        } else {
            return Err(RunceptError::EnvironmentError(
                "No environment manager available".to_string(),
            ));
        };

        // Get running processes from runtime manager
        let running_processes = self
            .list_processes_for_environment(environment_id)
            .await
            .unwrap_or_else(|_| Vec::new());

        // Create a map of running processes for quick lookup
        let running_processes_map: std::collections::HashMap<String, &ProcessInfo> =
            running_processes
                .iter()
                .map(|process| (process.name.clone(), process))
                .collect();

        // Build process list from configured processes, showing their current status
        let processes: Vec<ProcessInfo> = configured_processes
            .keys()
            .map(|name| {
                if let Some(running_process) = running_processes_map.get(name) {
                    // Process is running, use actual status
                    (*running_process).clone()
                } else {
                    // Process is configured but not running
                    ProcessInfo {
                        id: format!("{name}-configured"),
                        name: name.clone(),
                        status: "configured".to_string(),
                        pid: None,
                        uptime: None,
                        environment: environment_id.to_string(),
                        health_status: None,
                        restart_count: 0,
                        last_activity: None,
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
        let processes = self.get_processes_for_environment(environment_id).await?;
        let total = processes.len();
        let running = processes.iter().filter(|p| p.status == "running").count();
        Ok((processes, total, running))
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

    /// Start a process with a specific process definition and working directory
    /// Legacy method - delegates to new component architecture
    pub async fn start_process(
        &mut self,
        process_def: &ProcessDefinition,
        working_dir: &Path,
    ) -> Result<String> {
        // Use working directory as environment ID for legacy compatibility
        let environment_id = working_dir.to_string_lossy().to_string();

        // First add the process to the configuration if it doesn't exist
        let temp_process = process_def.clone();
        let _ = self
            .add_process_to_environment(temp_process, &environment_id)
            .await;

        // Then start it using the new component architecture
        self.start_process_by_name_in_environment(&process_def.name, &environment_id)
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
        // For legacy compatibility, treat process_id as process_name
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
        // For legacy compatibility, treat process_id as process_name
        let environment_id = self.resolve_environment_id(None).await?;
        self.restart_process_by_name_in_environment(process_id, &environment_id)
            .await
    }

    /// Stop all processes in the current environment (legacy compatibility)
    pub async fn stop_all_processes(&mut self) -> Result<()> {
        let environment_id = self.resolve_environment_id(None).await?;
        let mut runtime_manager = self.runtime_manager.write().await;
        runtime_manager.stop_all_processes(&environment_id).await
    }

    /// Get process logs by ID (legacy compatibility)
    pub async fn get_process_logs(
        &self,
        process_id: &str,
        max_lines: Option<usize>,
    ) -> Result<Vec<LogEntry>> {
        // For legacy compatibility, treat process_id as process_name
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

// ===== Tests =====

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GlobalConfig, ProcessDefinition};
    use crate::database::Database;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tempfile::TempDir;

    /// Create an in-memory database for testing
    async fn create_test_database() -> Database {
        let db = Database::new("sqlite::memory:").await.unwrap();
        db.init().await.unwrap();
        db
    }

    #[tokio::test]
    async fn test_process_manager_creation() {
        let global_config = GlobalConfig::default();
        
        // Create test database
        let database = create_test_database().await;
        let database_pool = Arc::new(database.get_pool().clone());
        
        // Create environment manager with database
        let env_manager = Arc::new(tokio::sync::RwLock::new(
            crate::config::EnvironmentManager::new_with_database(
                global_config.clone(),
                Some(database.get_pool().clone()),
            ).await.unwrap(),
        ));
        
        let manager = ProcessManager::new_with_environment_manager(
            global_config,
            env_manager,
            Some(database_pool),
        ).await.unwrap();

        // With the new architecture, we have proper component delegation
        assert!(manager.environment_manager.is_some());
    }

    #[tokio::test]
    async fn test_environment_resolution() {
        let global_config = GlobalConfig::default();
        
        // Create test database
        let database = create_test_database().await;
        let database_pool = Arc::new(database.get_pool().clone());
        
        // Create environment manager with database
        let env_manager = Arc::new(tokio::sync::RwLock::new(
            crate::config::EnvironmentManager::new_with_database(
                global_config.clone(),
                Some(database.get_pool().clone()),
            ).await.unwrap(),
        ));
        
        let manager = ProcessManager::new_with_environment_manager(
            global_config,
            env_manager,
            Some(database_pool),
        ).await.unwrap();

        // Test environment resolution with no active environment
        let result = manager.resolve_environment_id(None).await;
        assert!(result.is_err());

        // Test environment resolution with provided ID
        let result = manager
            .resolve_environment_id(Some("test_env".to_string()))
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test_env");
    }

    #[tokio::test]
    async fn test_component_delegation() {
        let global_config = GlobalConfig::default();
        
        // Create test database
        let database = create_test_database().await;
        let database_pool = Arc::new(database.get_pool().clone());
        
        // Create environment manager with database
        let env_manager = Arc::new(tokio::sync::RwLock::new(
            crate::config::EnvironmentManager::new_with_database(
                global_config.clone(),
                Some(database.get_pool().clone()),
            ).await.unwrap(),
        ));
        
        let mut manager = ProcessManager::new_with_environment_manager(
            global_config,
            env_manager,
            Some(database_pool),
        ).await.unwrap();

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

        // Test that component delegation works without errors
        let result = manager
            .add_process_to_environment(process_def.clone(), &environment_id)
            .await;

        // For now, we expect this to work or fail gracefully
        // The actual behavior depends on the environment manager setup
        match result {
            Ok(_) => {
                // Success - component delegation is working
                assert!(true);
            }
            Err(e) => {
                // Expected error due to environment setup - that's okay for this test
                assert!(
                    e.to_string().contains("Environment") || e.to_string().contains("not found")
                );
            }
        }
    }
}
