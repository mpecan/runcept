use crate::config::ProcessDefinition;
use crate::error::{Result, RunceptError};
use crate::process::configuration::ProcessConfigurationManager;
use crate::process::{
    ExecutionProcessHandle, HealthCheckTrait, Process, ProcessGroupManager, ProcessLogger,
    ProcessMonitoringService, ProcessRepositoryTrait, ProcessRuntimeTrait, ProcessStatus,
    health_check_from_url,
};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock, mpsc};
use tracing::{error, info, warn};

/// Service for managing process lifecycle operations (start/stop/restart)
pub struct ProcessLifecycleManager<R, RT, H>
where
    R: ProcessRepositoryTrait,
    RT: ProcessRuntimeTrait,
    H: HealthCheckTrait,
{
    /// Process handles mapped by environment_id -> process_name -> handle
    pub process_handles: HashMap<String, HashMap<String, ExecutionProcessHandle>>,
    /// Reference to configuration manager
    pub process_configuration_manager: Arc<RwLock<ProcessConfigurationManager>>,
    /// Global configuration
    pub global_config: crate::config::GlobalConfig,
    /// Trait implementations
    pub process_repository: Arc<R>,
    pub process_runtime: Arc<RT>,
    pub health_check_service: Arc<H>,
    /// Process group manager for termination
    pub group_manager: ProcessGroupManager,
    /// Process monitoring service for exit handling
    pub monitoring_service: Arc<ProcessMonitoringService<R>>,
}

impl<R, RT, H> ProcessLifecycleManager<R, RT, H>
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
        monitoring_service: Arc<ProcessMonitoringService<R>>,
    ) -> Self {
        Self {
            process_handles: HashMap::new(),
            process_configuration_manager,
            global_config,
            process_repository,
            process_runtime,
            health_check_service,
            group_manager: ProcessGroupManager::new(),
            monitoring_service,
        }
    }

    /// Helper method to log lifecycle events to process logs
    async fn log_lifecycle_event(&self, process_name: &str, environment_id: &str, message: &str) {
        // Try to get the logger from active handles first
        if let Some(env_handles) = self.process_handles.get(environment_id) {
            if let Some(handle) = env_handles.get(process_name) {
                let mut logger = handle.logger.lock().await;
                let _ = logger.log_lifecycle(message).await;
                return;
            }
        }

        // If no active handle, create a temporary logger to write the lifecycle event
        let working_dir = match self
            .process_configuration_manager
            .read()
            .await
            .get_working_directory(environment_id)
            .await
        {
            Ok(dir) => dir,
            Err(_) => {
                error!("Failed to get working directory for lifecycle logging");
                return;
            }
        };

        match ProcessLogger::new(process_name.to_string(), working_dir).await {
            Ok(mut logger) => {
                let _ = logger.log_lifecycle(message).await;
            }
            Err(e) => {
                error!("Failed to create logger for lifecycle event: {}", e);
            }
        }
    }

    pub async fn start_process(&mut self, name: &str, environment_id: &str) -> Result<String> {
        info!(
            "Starting process '{}' in environment '{}'",
            name, environment_id
        );

        // Check if process is already running by querying via repository trait
        match self
            .process_repository
            .get_process_by_name(environment_id, name)
            .await
        {
            Ok(Some(existing_process)) => {
                if matches!(existing_process.status.as_str(), "running" | "starting") {
                    return Err(RunceptError::ProcessError(format!(
                        "Process '{name}' is already running in environment '{environment_id}'"
                    )));
                }
            }
            Ok(None) => {
                return Err(RunceptError::ProcessError(format!(
                    "Process '{name}' not found in environment '{environment_id}'"
                )));
            }
            Err(e) => {
                return Err(e);
            }
        }

        // Log activity event: starting
        self.log_lifecycle_event(name, environment_id, &format!("Starting process '{name}'"))
            .await;

        // Get process from database which has the correctly resolved working directory
        let process_record = self
            .process_repository
            .get_process_by_name(environment_id, name)
            .await
            .map_err(|e| {
                RunceptError::ProcessError(format!(
                    "Failed to retrieve process '{name}' from environment '{environment_id}': {e}"
                ))
            })?
            .ok_or_else(|| {
                RunceptError::ProcessError(format!(
                    "Process '{name}' not found in environment '{environment_id}'"
                ))
            })?;

        // Get process definition for complete configuration
        let process_def = {
            let config_manager = self.process_configuration_manager.read().await;
            config_manager
                .get_process_definition(name, environment_id)
                .await
                .map_err(|e| RunceptError::ProcessError(format!(
                    "Failed to get process definition for '{name}' in environment '{environment_id}': {e}"
                )))?
        };

        // Create Process from definition but use the resolved working_dir from database
        let mut process = Process::from_definition_with_context(
            process_def.clone(),
            environment_id.to_string(),
            Some(process_record.working_dir), // Use resolved working_dir from database
        );

        // Set the process ID to match database record
        process.id = process_record.id;
        process.created_at = process_record.created_at;
        process.updated_at = process_record.updated_at;

        // Start the process using the complete entity with correct working_dir and full configuration
        let process_id = self
            .start_process_with_existing_entity(process, &process_def, environment_id)
            .await?;

        // If process has a health check configured, wait for it to be healthy before returning
        if process_def.health_check_url.is_some() {
            self.log_lifecycle_event(
                name,
                environment_id,
                &format!("Waiting for health check to pass for process '{name}'"),
            )
            .await;

            match self
                .wait_for_health_check(&process_def, name, environment_id)
                .await
            {
                Ok(()) => {
                    self.log_lifecycle_event(
                        name,
                        environment_id,
                        &format!("Health check passed for process '{name}'"),
                    )
                    .await;
                }
                Err(e) => {
                    self.log_lifecycle_event(
                        name,
                        environment_id,
                        &format!("Health check failed for process '{name}': {e}"),
                    )
                    .await;

                    // Stop the process since health check failed
                    let _ = self.stop_process(name, environment_id).await;
                    return Err(e);
                }
            }
        }

        self.log_lifecycle_event(
            name,
            environment_id,
            &format!("Successfully started process '{name}' (ID: {process_id})"),
        )
        .await;

        info!(
            "Successfully started process '{}' in environment '{}' with ID '{}'",
            name, environment_id, process_id
        );
        Ok(process_id)
    }

    /// Stop a process in the specified environment
    pub async fn stop_process(&mut self, name: &str, environment_id: &str) -> Result<()> {
        info!(
            "Stopping process '{}' in environment '{}'",
            name, environment_id
        );

        // Log lifecycle event: stopping
        self.log_lifecycle_event(name, environment_id, &format!("Stopping process '{name}'"))
            .await;

        // Check if process exists and is running via repository trait
        let process_status = match self
            .process_repository
            .get_process_by_name(environment_id, name)
            .await
        {
            Ok(Some(process)) => process.status,
            Ok(None) => {
                return Err(RunceptError::ProcessError(format!(
                    "Process '{name}' not found in environment '{environment_id}'"
                )));
            }
            Err(e) => {
                return Err(e);
            }
        };

        // Check if process is already stopped
        if matches!(process_status.as_str(), "stopped" | "failed" | "crashed") {
            return Ok(()); // Already stopped
        }

        // Update process status to stopping in database via repository trait
        self.process_repository
            .update_process_status(environment_id, name, "stopping")
            .await?;

        // Get process handle and attempt graceful shutdown using runtime trait
        let mut process_killed = false;
        if let Some(env_handles) = self.process_handles.get(environment_id) {
            if let Some(handle) = env_handles.get(name) {
                // Send shutdown signal first
                let _ = handle.shutdown_tx.send(()).await;

                // Try to terminate the process group gracefully
                let child_guard = handle.child.lock().await;
                if let Some(child) = child_guard.as_ref() {
                    if let Some(pid) = child.id() {
                        // Try to kill the entire process group using sysinfo
                        process_killed = self
                            .group_manager
                            .kill_process_group(pid as i32, name, environment_id)
                            .await;
                    } else {
                        warn!(
                            "No PID available for process '{}' in environment '{}'",
                            name, environment_id
                        );
                    }

                    // Don't clear the child handle here! Let the monitoring task detect the exit
                    // and send the proper exit notification. The monitoring task will clean up.
                }
            }
        }

        // If we couldn't kill via handle, try to kill by PID using system calls
        if !process_killed {
            if let Ok(Some(process)) = self
                .process_repository
                .get_process_by_name(environment_id, name)
                .await
            {
                if let Some(pid) = process.pid {
                    // Try to kill the entire process group by PID
                    process_killed = self
                        .group_manager
                        .kill_process_group(pid as i32, name, environment_id)
                        .await;
                }
            }
        }

        // Note: We don't manually update the database status here anymore.
        // The exit notification system will handle the status transition from "stopping" to "stopped"
        // when the process actually exits and the monitoring task detects it.

        if !process_killed {
            warn!(
                "Failed to kill process '{}' in environment '{}' - it may still be running",
                name, environment_id
            );
        }

        // Log lifecycle event: stopped
        self.log_lifecycle_event(
            name,
            environment_id,
            &format!("Successfully stopped process '{name}'"),
        )
        .await;

        info!(
            "Successfully stopped process '{}' in environment '{}'",
            name, environment_id
        );
        Ok(())
    }

    /// Restart a process in the specified environment
    pub async fn restart_process(&mut self, name: &str, environment_id: &str) -> Result<()> {
        info!(
            "Restarting process '{}' in environment '{}'",
            name, environment_id
        );

        // Log lifecycle event: restarting
        self.log_lifecycle_event(
            name,
            environment_id,
            &format!("Restarting process '{name}'"),
        )
        .await;

        // Stop the process first
        self.stop_process(name, environment_id).await?;

        // Wait a bit for the process to stop
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Remove the old process from tracking
        self.remove_process_from_tracking(name, environment_id);

        // Start the process again
        self.start_process(name, environment_id).await?;

        // Log lifecycle event: restarted
        self.log_lifecycle_event(
            name,
            environment_id,
            &format!("Successfully restarted process '{name}'"),
        )
        .await;

        info!(
            "Successfully restarted process '{}' in environment '{}'",
            name, environment_id
        );
        Ok(())
    }

    async fn start_process_with_existing_entity(
        &mut self,
        mut process: crate::process::Process,
        process_def: &ProcessDefinition,
        environment_id: &str,
    ) -> Result<String> {
        // Process entity already has the correctly resolved working directory from database
        // Just apply any global config overrides

        // Override auto-restart with global config if not specified in definition
        if process_def.auto_restart.is_none() {
            process.auto_restart = self.global_config.process.auto_restart_on_crash;
        }

        // Set health check interval from global config if URL provided but interval not specified
        if process_def.health_check_url.is_some() && process_def.health_check_interval.is_none() {
            if let Some(url) = &process.health_check_url {
                process.set_health_check(
                    url.clone(),
                    self.global_config.process.health_check_interval,
                );
            }
        }

        // Transition to starting
        process.transition_to(ProcessStatus::Starting);

        // Store/update process in database with "starting" status via repository trait
        self.process_repository.insert_process(&process).await?;

        // Spawn the process using runtime trait
        let process_handle = self.process_runtime.spawn_process(&process).await?;

        // Get the PID from the child process
        let pid = process_handle.child.id().unwrap_or(0);

        // Update process with PID and running status
        self.process_repository
            .set_process_pid(environment_id, &process.name, pid as i64)
            .await?;

        self.process_repository
            .update_process_status(environment_id, &process.name, "running")
            .await?;

        // Store process handle using the correctly resolved working directory from the Process entity
        let process_id = process.id.clone();
        self.store_process_handle(
            process.name.clone(),
            environment_id,
            process_handle,
            std::path::Path::new(&process.working_dir), // Use the resolved working_dir from database
        )
        .await?;

        Ok(process_id)
    }

    /// Store a process handle for tracking
    async fn store_process_handle(
        &mut self,
        process_name: String,
        environment_id: &str,
        process_handle: crate::process::traits::ProcessHandle,
        working_dir: &Path,
    ) -> Result<()> {
        // Create process logger
        let logger = ProcessLogger::new(process_name.clone(), working_dir.to_path_buf())
            .await
            .map_err(|e| {
                RunceptError::ProcessError(format!(
                    "Failed to create logger for process '{process_name}': {e}"
                ))
            })?;

        // Set up shutdown channel
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>(1);

        // Convert process handle to our internal format
        let child_handle = Arc::new(Mutex::new(Some(process_handle.child)));
        let logger_handle = Arc::new(Mutex::new(logger));

        let handle = ExecutionProcessHandle {
            child: child_handle.clone(),
            shutdown_tx,
            logger: logger_handle.clone(),
        };

        // Initialize environment maps if they don't exist
        self.process_handles
            .entry(environment_id.to_string())
            .or_default();

        // Store the process handle
        self.process_handles
            .get_mut(environment_id)
            .unwrap()
            .insert(process_name.clone(), handle);

        // Spawn process monitoring task
        ProcessMonitoringService::<R>::spawn_process_monitoring_task(
            child_handle,
            process_name,
            shutdown_rx,
            environment_id.to_string(),
            self.monitoring_service.exit_notification_tx.clone(),
            working_dir.to_path_buf(),
        );

        Ok(())
    }

    /// Helper method to remove a process from tracking collections
    fn remove_process_from_tracking(&mut self, name: &str, environment_id: &str) {
        if let Some(env_handles) = self.process_handles.get_mut(environment_id) {
            env_handles.remove(name);
        }
    }

    /// Wait for a process health check to pass during startup
    async fn wait_for_health_check(
        &self,
        process_def: &ProcessDefinition,
        process_name: &str,
        environment_id: &str,
    ) -> Result<()> {
        let health_url = match &process_def.health_check_url {
            Some(url) => url,
            None => return Ok(()), // No health check configured
        };

        let timeout_secs = self.global_config.process.startup_health_check_timeout;
        let check_timeout =
            std::time::Duration::from_secs(self.global_config.process.health_check_timeout as u64);

        info!(
            "Starting health check for process '{}' with URL '{}', timeout: {}s",
            process_name, health_url, timeout_secs
        );

        let health_check_config = health_check_from_url(health_url, check_timeout.as_secs(), 3)
            .map_err(|e| {
                RunceptError::ProcessError(format!(
                    "Invalid health check URL for process '{process_name}': {e}"
                ))
            })?;

        let start_time = std::time::Instant::now();
        let timeout_duration = std::time::Duration::from_secs(timeout_secs as u64);

        // Poll health check until it passes or times out
        let mut check_count = 0;
        while start_time.elapsed() < timeout_duration {
            check_count += 1;

            match self
                .health_check_service
                .execute_health_check(&health_check_config)
                .await
            {
                Ok(result) if result.success => {
                    info!(
                        "Health check passed for process '{}' in environment '{}' after {:.2}s (attempt #{})",
                        process_name,
                        environment_id,
                        start_time.elapsed().as_secs_f64(),
                        check_count
                    );
                    return Ok(());
                }
                Ok(result) => {
                    warn!(
                        "Health check failed for process '{}' in environment '{}' (attempt #{}): {}",
                        process_name, environment_id, check_count, result.message
                    );
                }
                Err(e) => {
                    warn!(
                        "Health check error for process '{}' in environment '{}' (attempt #{}): {}",
                        process_name, environment_id, check_count, e
                    );
                }
            }

            // Wait before retrying
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }

        Err(RunceptError::ProcessError(format!(
            "Health check timeout for process '{process_name}' after {timeout_secs} seconds"
        )))
    }
}

// Type alias for the default concrete implementation
pub type DefaultProcessLifecycleManager = ProcessLifecycleManager<
    crate::database::ProcessRepository,
    crate::process::DefaultProcessRuntime,
    crate::process::DefaultHealthCheckService,
>;

impl DefaultProcessLifecycleManager {
    /// Create a new ProcessLifecycleManager with default implementations
    pub fn new_default(
        process_configuration_manager: Arc<RwLock<ProcessConfigurationManager>>,
        global_config: crate::config::GlobalConfig,
        db_pool: Arc<sqlx::Pool<sqlx::Sqlite>>,
    ) -> Self {
        let process_repository = Arc::new(crate::database::ProcessRepository::new(db_pool.clone()));
        let process_runtime = Arc::new(crate::process::DefaultProcessRuntime);
        let health_check_service = Arc::new(crate::process::DefaultHealthCheckService::new());
        let monitoring_service =
            Arc::new(ProcessMonitoringService::new(process_repository.clone()));

        Self::new(
            process_configuration_manager,
            global_config,
            process_repository,
            process_runtime,
            health_check_service,
            monitoring_service,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{
        DatabaseFixture, MockHealthCheckService, MockProcessRepository, MockProcessRuntime,
    };

    #[tokio::test]
    async fn test_lifecycle_manager_creation() {
        let global_config = crate::config::GlobalConfig::default();
        let mock_repo = Arc::new(MockProcessRepository::new());
        let mock_runtime = Arc::new(MockProcessRuntime);
        let mock_health = Arc::new(MockHealthCheckService::new());
        let monitoring_service = Arc::new(ProcessMonitoringService::new(mock_repo.clone()));

        // Create a real ProcessRepository for the configuration manager
        let db_fixture = DatabaseFixture::new().await.unwrap();
        let real_repo = Arc::new(crate::database::ProcessRepository::new(
            db_fixture.pool.clone(),
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

        let lifecycle_manager = ProcessLifecycleManager::new(
            config_manager,
            global_config,
            mock_repo,
            mock_runtime,
            mock_health,
            monitoring_service,
        );

        assert!(lifecycle_manager.process_handles.is_empty());
    }

    #[tokio::test]
    async fn test_start_nonexistent_process() {
        let global_config = crate::config::GlobalConfig::default();
        let mock_repo = Arc::new(MockProcessRepository::new());
        let mock_runtime = Arc::new(MockProcessRuntime);
        let mock_health = Arc::new(MockHealthCheckService::new());
        let monitoring_service = Arc::new(ProcessMonitoringService::new(mock_repo.clone()));

        // Create a real ProcessRepository with in-memory database for configuration manager
        let db_fixture = DatabaseFixture::new().await.unwrap();
        let real_repo = Arc::new(crate::database::ProcessRepository::new(
            db_fixture.pool.clone(),
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

        let mut lifecycle_manager = ProcessLifecycleManager::new(
            config_manager,
            global_config,
            mock_repo,
            mock_runtime,
            mock_health,
            monitoring_service,
        );

        // Try to start a process that doesn't exist
        let result = lifecycle_manager
            .start_process("nonexistent", "test-env")
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_stop_nonexistent_process() {
        let global_config = crate::config::GlobalConfig::default();
        let mock_repo = Arc::new(MockProcessRepository::new());
        let mock_runtime = Arc::new(MockProcessRuntime);
        let mock_health = Arc::new(MockHealthCheckService::new());
        let monitoring_service = Arc::new(ProcessMonitoringService::new(mock_repo.clone()));

        // Create a real ProcessRepository with in-memory database for configuration manager
        let db_fixture = DatabaseFixture::new().await.unwrap();
        let real_repo = Arc::new(crate::database::ProcessRepository::new(
            db_fixture.pool.clone(),
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

        let mut lifecycle_manager = ProcessLifecycleManager::new(
            config_manager,
            global_config,
            mock_repo,
            mock_runtime,
            mock_health,
            monitoring_service,
        );

        // Try to stop a process that doesn't exist
        let result = lifecycle_manager
            .stop_process("nonexistent", "test-env")
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_remove_process_from_tracking() {
        let global_config = crate::config::GlobalConfig::default();
        let mock_repo = Arc::new(MockProcessRepository::new());
        let mock_runtime = Arc::new(MockProcessRuntime);
        let mock_health = Arc::new(MockHealthCheckService::new());
        let monitoring_service = Arc::new(ProcessMonitoringService::new(mock_repo.clone()));

        // Create a real ProcessRepository with in-memory database for configuration manager
        let db_fixture = DatabaseFixture::new().await.unwrap();
        let real_repo = Arc::new(crate::database::ProcessRepository::new(
            db_fixture.pool.clone(),
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

        let mut lifecycle_manager = ProcessLifecycleManager::new(
            config_manager,
            global_config,
            mock_repo,
            mock_runtime,
            mock_health,
            monitoring_service,
        );

        // Add a fake process to tracking
        lifecycle_manager
            .process_handles
            .insert("test-env".to_string(), HashMap::new());

        // Remove it
        lifecycle_manager.remove_process_from_tracking("test-process", "test-env");

        // Should not panic
        assert!(true);
    }
}
