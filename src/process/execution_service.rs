use crate::cli::commands::ProcessInfo;
use crate::config::ProcessDefinition;
use crate::error::{Result, RunceptError};
use crate::process::configuration::ProcessConfigurationManager;
use crate::process::{
    DefaultHealthCheckService, DefaultProcessRuntime, HealthCheckTrait, LogEntry, Process,
    ProcessLogger, ProcessRepositoryTrait, ProcessRuntimeTrait, ProcessStatus,
    health_check_from_url,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock, broadcast, mpsc};
use tracing::{debug, error, info, warn};

/// Notification sent when a process exits
#[derive(Debug, Clone)]
pub struct ProcessExitNotification {
    pub process_name: String,
    pub environment_id: String,
    pub exit_status: std::process::ExitStatus,
}

/// Handle for a running process (execution service specific)
pub struct ExecutionProcessHandle {
    pub child: Arc<Mutex<Option<tokio::process::Child>>>,
    pub shutdown_tx: mpsc::Sender<()>,
    pub logger: Arc<Mutex<ProcessLogger>>,
}

/// Trait-based process execution service that replaces ProcessRuntimeManager
pub struct ProcessExecutionService<R, RT, H>
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
    /// Channel for receiving process exit notifications
    pub exit_notification_tx: broadcast::Sender<ProcessExitNotification>,
    pub exit_notification_rx: broadcast::Receiver<ProcessExitNotification>,
    /// Trait implementations
    pub process_repository: Arc<R>,
    pub process_runtime: Arc<RT>,
    pub health_check_service: Arc<H>,
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
        let (exit_notification_tx, exit_notification_rx) = broadcast::channel(100);

        let mut service = Self {
            process_handles: HashMap::new(),
            process_configuration_manager,
            global_config,
            exit_notification_tx,
            exit_notification_rx,
            process_repository,
            process_runtime,
            health_check_service,
        };

        // Start the exit notification listener task
        service.start_exit_notification_listener();

        service
    }

    /// Start a background task to listen for process exit notifications
    /// and update database status accordingly
    fn start_exit_notification_listener(&mut self) {
        let mut rx = self.exit_notification_tx.subscribe();
        let repository = self.process_repository.clone();
        
        tokio::spawn(async move {
            while let Ok(notification) = rx.recv().await {
                debug!(
                    "Received exit notification for process '{}' in environment '{}' with status: {}",
                    notification.process_name, notification.environment_id, notification.exit_status
                );

                // Determine the new status based on exit code and current status
                let new_status = match repository
                    .get_process_by_name(&notification.environment_id, &notification.process_name)
                    .await
                {
                    Ok(Some(process)) if process.status == "stopping" => {
                        // Process was deliberately stopped via stop command, mark as stopped
                        "stopped"
                    }
                    _ => {
                        // Process exited naturally or crashed
                        if notification.exit_status.success() {
                            "stopped"
                        } else {
                            "crashed"
                        }
                    }
                };

                // Update process status in database
                if let Err(e) = repository
                    .update_process_status(&notification.environment_id, &notification.process_name, new_status)
                    .await
                {
                    error!(
                        "Failed to update process status for '{}:{}' to '{}': {}",
                        notification.environment_id, notification.process_name, new_status, e
                    );
                }

                // Clear the PID since process has exited
                if let Err(e) = repository
                    .clear_process_pid(&notification.environment_id, &notification.process_name)
                    .await
                {
                    error!(
                        "Failed to clear PID for process '{}:{}': {}",
                        notification.environment_id, notification.process_name, e
                    );
                }

                // Log the process exit event
                info!(
                    "Process '{}:{}' exited with code {:?}, new status: '{}'",
                    notification.environment_id, notification.process_name,
                    notification.exit_status.code(), new_status
                );

                info!(
                    "Updated process '{}:{}' status to '{}' after exit with code {:?}",
                    notification.environment_id, notification.process_name, new_status, notification.exit_status.code()
                );
            }
        });
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

        // Activity already logged through log_lifecycle_event above

        // Get process from database which has the correctly resolved working directory
        let process_record = self
            .process_repository
            .get_process_by_name(environment_id, name)
            .await?
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
                .await?
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

        // Activity already logged through log_lifecycle_event above

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
                        process_killed = self.kill_process_group(pid as i32, name, environment_id).await;
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
                    process_killed = self.kill_process_group(pid as i32, name, environment_id).await;
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
                error!(
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
    ) -> Option<ProcessStatus> {
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
                "running" => Some(ProcessStatus::Running),
                "stopped" => Some(ProcessStatus::Stopped),
                "starting" => Some(ProcessStatus::Starting),
                "stopping" => Some(ProcessStatus::Stopping),
                "failed" => Some(ProcessStatus::Failed),
                "crashed" => Some(ProcessStatus::Crashed),
                _ => Some(ProcessStatus::Stopped),
            },
            Ok(None) => None,
            Err(e) => {
                error!(
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
            // If process is running, try to read logs from logger
            if let Some(env_handles) = self.process_handles.get(environment_id) {
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
            Err(RunceptError::ProcessError(format!(
                "Process '{name}' not found in environment '{environment_id}'"
            )))
        }
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
                    "Failed to create logger for process '{}': {}",
                    process_name, e
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
        Self::spawn_process_monitoring_task(
            child_handle,
            process_name,
            shutdown_rx,
            environment_id.to_string(),
            self.exit_notification_tx.clone(),
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

    /// Kill an entire process group using system calls
    async fn kill_process_group(&self, pid: i32, process_name: &str, environment_id: &str) -> bool {
        use sysinfo::{Signal as SysinfoSignal, System};
        
        info!(
            "Attempting to kill process group for process '{}' (PID: {}) in environment '{}'",
            process_name, pid, environment_id
        );

        let mut system = System::new_all();
        
        // Find all processes in the same process group
        let mut processes_to_kill = Vec::new();
        
        // First, find the main process and its process group
        if let Some(_main_process) = system.process(sysinfo::Pid::from_u32(pid as u32)) {
            processes_to_kill.push(pid);
            
            // Find all child processes recursively
            self.find_all_child_processes(pid, &system, &mut processes_to_kill);
        }

        if processes_to_kill.is_empty() {
            warn!(
                "No processes found for process '{}' (PID: {}) in environment '{}'",
                process_name, pid, environment_id
            );
            return false;
        }

        info!(
            "Found {} processes to kill for '{}': {:?}",
            processes_to_kill.len(), process_name, processes_to_kill
        );

        // First pass: Send SIGTERM to all processes (graceful shutdown)
        for &target_pid in &processes_to_kill {
            if let Some(process) = system.process(sysinfo::Pid::from_u32(target_pid as u32)) {
                if process.kill_with(SysinfoSignal::Term).unwrap_or(false) {
                    debug!("Sent SIGTERM to process {}", target_pid);
                }
            }
        }

        // Wait a moment for graceful shutdown
        tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;

        // Second pass: Check which processes are still alive and force kill them
        system.refresh_all();
        for &target_pid in &processes_to_kill {
            if let Some(process) = system.process(sysinfo::Pid::from_u32(target_pid as u32)) {
                // Process is still alive, force kill it
                if process.kill_with(SysinfoSignal::Kill).unwrap_or(false) {
                    debug!("Sent SIGKILL to process {}", target_pid);
                } else {
                    warn!("Failed to send SIGKILL to process {}", target_pid);
                }
            }
        }

        // Wait a bit for processes to be killed
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Verify all processes are gone
        system.refresh_all();
        let mut remaining_processes = Vec::new();
        for &target_pid in &processes_to_kill {
            if system.process(sysinfo::Pid::from_u32(target_pid as u32)).is_some() {
                remaining_processes.push(target_pid);
            }
        }

        if remaining_processes.is_empty() {
            info!(
                "Successfully killed all {} processes for '{}' in environment '{}'",
                processes_to_kill.len(), process_name, environment_id
            );
            true
        } else {
            error!(
                "Failed to kill {} processes for '{}': {:?}",
                remaining_processes.len(), process_name, remaining_processes
            );
            false
        }
    }

    /// Recursively find all child processes of a given PID
    fn find_all_child_processes(&self, parent_pid: i32, system: &sysinfo::System, result: &mut Vec<i32>) {
        for (child_pid, process) in system.processes() {
            if let Some(ppid) = process.parent() {
                if ppid.as_u32() as i32 == parent_pid {
                    let child_pid_val = child_pid.as_u32() as i32;
                    result.push(child_pid_val);
                    
                    // Recursively find children of this child
                    self.find_all_child_processes(child_pid_val, system, result);
                }
            }
        }
    }

    /// Kill a process by PID using system calls

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


    /// Spawn a task to monitor process completion and handle shutdown signals
    fn spawn_process_monitoring_task(
        child_handle: Arc<Mutex<Option<tokio::process::Child>>>,
        process_name: String,
        mut shutdown_rx: mpsc::Receiver<()>,
        environment_id: String,
        exit_notification_tx: broadcast::Sender<ProcessExitNotification>,
        working_dir: PathBuf,
    ) {
        tokio::spawn(async move {
            let mut child_guard = child_handle.lock().await;
            if let Some(mut child) = child_guard.take() {
                drop(child_guard); // Release the lock while waiting

                // Wait for the process to complete or receive shutdown signal
                let mut shutdown_received = false;
                tokio::select! {
                    result = child.wait() => {
                        match result {
                            Ok(exit_status) => {
                                info!(
                                    "Process '{}' in environment '{}' exited with status: {}",
                                    process_name, environment_id, exit_status
                                );

                                // Send exit notification
                                let notification = ProcessExitNotification {
                                    process_name: process_name.clone(),
                                    environment_id: environment_id.clone(),
                                    exit_status,
                                };

                                if let Ok(mut logger) =
                                    ProcessLogger::new(process_name.clone(), working_dir.to_path_buf()).await
                                {
                                    let _ = logger
                                        .log_lifecycle(&format!(
                                            "Process '{process_name}' exited with status {exit_status}"
                                        ))
                                        .await;
                                }

                                if let Err(e) = exit_notification_tx.send(notification) {
                                    error!(
                                        "Failed to send exit notification for process '{}' in environment '{}': {}",
                                        process_name, environment_id, e
                                    );
                                }
                            }
                            Err(e) => {
                                error!(
                                    "Error waiting for process '{}' in environment '{}': {}",
                                    process_name, environment_id, e
                                );
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        debug!(
                            "Received shutdown signal for process '{}' in environment '{}', but still waiting for actual exit",
                            process_name, environment_id
                        );
                        shutdown_received = true;
                    }
                }

                // If we received a shutdown signal, we still need to wait for the actual process exit
                if shutdown_received {
                    debug!(
                        "Waiting for process '{}' in environment '{}' to actually exit after shutdown signal",
                        process_name, environment_id
                    );
                    
                    match child.wait().await {
                        Ok(exit_status) => {
                            info!(
                                "Process '{}' in environment '{}' exited after shutdown with status: {}",
                                process_name, environment_id, exit_status
                            );

                            // Send exit notification
                            let notification = ProcessExitNotification {
                                process_name: process_name.clone(),
                                environment_id: environment_id.clone(),
                                exit_status,
                            };

                            if let Ok(mut logger) =
                                ProcessLogger::new(process_name.clone(), working_dir.to_path_buf()).await
                            {
                                let _ = logger
                                    .log_lifecycle(&format!(
                                        "Process '{process_name}' exited after shutdown with status {exit_status}"
                                    ))
                                    .await;
                            }

                            if let Err(e) = exit_notification_tx.send(notification) {
                                error!(
                                    "Failed to send exit notification for process '{}' in environment '{}': {}",
                                    process_name, environment_id, e
                                );
                            }
                        }
                        Err(e) => {
                            error!(
                                "Error waiting for process '{}' in environment '{}' to exit after shutdown: {}",
                                process_name, environment_id, e
                            );
                        }
                    }
                }
            }
        });
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
