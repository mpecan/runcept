use crate::cli::commands::ProcessInfo;
use crate::config::ProcessDefinition;
use crate::database::ProcessRepository;
use crate::error::{Result, RunceptError};
use crate::process::configuration::ProcessConfigurationManager;
use crate::process::monitor::HealthCheck;
use crate::process::{LogEntry, Process, ProcessLogger, ProcessStatus};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, RwLock, broadcast, mpsc};
use tracing::{debug, error, info, warn};

/// Notification sent when a process exits
#[derive(Debug, Clone)]
pub struct ProcessExitNotification {
    pub process_name: String,
    pub environment_id: String,
    pub exit_status: std::process::ExitStatus,
}

/// Handle for a running process
pub struct ProcessHandle {
    pub child: Arc<Mutex<Option<Child>>>,
    pub shutdown_tx: mpsc::Sender<()>,
    pub logger: Arc<Mutex<ProcessLogger>>,
}

/// Manages the runtime execution of processes
pub struct ProcessRuntimeManager {
    /// Process handles mapped by environment_id -> process_name -> handle
    /// This is the only in-memory state we need - actual process data is stored in DB
    pub process_handles: HashMap<String, HashMap<String, ProcessHandle>>,
    /// Reference to configuration manager
    pub process_configuration_manager: Arc<RwLock<ProcessConfigurationManager>>,
    /// Global configuration
    pub global_config: crate::config::GlobalConfig,
    /// Channel for receiving process exit notifications
    pub exit_notification_tx: broadcast::Sender<ProcessExitNotification>,
    pub exit_notification_rx: broadcast::Receiver<ProcessExitNotification>,
    /// Process repository for database operations (required for DB-first approach)
    pub process_repository: Arc<ProcessRepository>,
}

impl ProcessRuntimeManager {
    /// Create a new ProcessRuntimeManager
    pub fn new(
        process_configuration_manager: Arc<RwLock<ProcessConfigurationManager>>,
        global_config: crate::config::GlobalConfig,
        process_repository: Arc<ProcessRepository>,
    ) -> Self {
        let (exit_notification_tx, exit_notification_rx) = broadcast::channel(100);

        Self {
            process_handles: HashMap::new(),
            process_configuration_manager,
            global_config,
            exit_notification_tx,
            exit_notification_rx,
            process_repository,
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

    /// Start a process in the specified environment
    pub async fn start_process(&mut self, name: &str, environment_id: &str) -> Result<String> {
        info!(
            "Starting process '{}' in environment '{}'",
            name, environment_id
        );

        // Check if process is already running by querying the database
        match self
            .process_repository
            .get_process_by_name_validated(environment_id, name)
            .await
        {
            Ok(Some(existing_process)) => {
                if existing_process.status == "running" || existing_process.status == "starting" {
                    return Err(RunceptError::ProcessError(format!(
                        "Process '{name}' is already running in environment '{environment_id}'"
                    )));
                }
            }
            Ok(None) => {
                // Process doesn't exist in DB, need to return an error
                return Err(RunceptError::ProcessError(format!(
                    "Process '{name}' not found in environment '{environment_id}'"
                )));
            }
            Err(e) => {
                // Environment validation failed or database error
                return Err(e);
            }
        }

        // Log lifecycle event: starting
        self.log_lifecycle_event(name, environment_id, &format!("Starting process '{name}'"))
            .await;

        // Get process definition and working directory from configuration manager
        let (process_def, working_dir) = {
            let config_manager = self.process_configuration_manager.read().await;
            let process_def = config_manager
                .get_process_definition(name, environment_id)
                .await?;
            let working_dir = config_manager.get_working_directory(environment_id).await?;
            (process_def, working_dir)
        };

        // Create and start the process
        let process_id = self
            .start_process_with_definition(&process_def, working_dir.as_path(), environment_id)
            .await?;

        // If process has a health check configured, wait for it to be healthy before returning
        info!(
            "Checking if process '{}' has health check configured: {:?}",
            name, process_def.health_check_url
        );
        if process_def.health_check_url.is_some() {
            info!(
                "Process '{}' has health check configured: {:?}",
                name, process_def.health_check_url
            );
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
        } else {
            info!(
                "Process '{}' has no health check configured, skipping health check wait",
                name
            );
        }

        self.log_lifecycle_event(
            name,
            environment_id,
            &format!("Successfully started process '{name}' (PID: {process_id})"),
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

        // Check if process exists and is running in the database
        let process_id = format!("{environment_id}:{name}");
        let process_status = match self
            .process_repository
            .get_process_by_name_validated(environment_id, name)
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
        if process_status == "stopped" || process_status == "failed" || process_status == "crashed"
        {
            return Ok(()); // Already stopped
        }

        // Update process status to stopping in database
        self.process_repository
            .update_process_status(&process_id, "stopping")
            .await
            .map_err(|e| {
                RunceptError::ProcessError(format!(
                    "Failed to update process status to stopping: {e}"
                ))
            })?;

        // Get process handle and attempt graceful shutdown
        let mut process_killed = false;
        if let Some(env_handles) = self.process_handles.get(environment_id) {
            if let Some(handle) = env_handles.get(name) {
                // Send shutdown signal first
                let _ = handle.shutdown_tx.send(()).await;

                // Try to terminate the process gracefully
                let mut child_guard = handle.child.lock().await;
                if let Some(child) = child_guard.as_mut() {
                    // First try SIGTERM (graceful shutdown)
                    if let Err(e) = child.kill().await {
                        warn!(
                            "Failed to send SIGTERM to process '{}' in environment '{}': {}",
                            name, environment_id, e
                        );
                    }

                    // Wait a bit for graceful shutdown
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

                    // Check if process is still running
                    match child.try_wait() {
                        Ok(Some(_exit_status)) => {
                            // Process has exited gracefully
                            debug!(
                                "Process '{}' in environment '{}' exited gracefully",
                                name, environment_id
                            );
                            process_killed = true;
                        }
                        Ok(None) => {
                            // Process is still running, force kill
                            warn!(
                                "Process '{}' in environment '{}' did not exit gracefully, force killing",
                                name, environment_id
                            );

                            // Force kill if it's still running
                            if let Err(e) = child.kill().await {
                                error!(
                                    "Failed to force kill process '{}' in environment '{}': {}",
                                    name, environment_id, e
                                );
                            } else {
                                process_killed = true;
                            }
                        }
                        Err(e) => {
                            error!(
                                "Error checking process '{}' in environment '{}' status: {}",
                                name, environment_id, e
                            );
                        }
                    }

                    // Clear the child handle
                    child_guard.take();
                }
            }
        }

        // If we don't have a handle or couldn't kill via handle, try to kill by PID
        if !process_killed {
            // Get the process from database to find its PID
            if let Ok(Some(process)) = self.process_repository.get_process_by_id(&process_id).await
            {
                if let Some(pid) = process.pid {
                    warn!(
                        "Process '{}' in environment '{}' has no handle, attempting to kill by PID {}",
                        name, environment_id, pid
                    );

                    // Try to kill the process by PID using system call
                    if let Err(e) = self.kill_process_by_pid(pid as u32).await {
                        error!(
                            "Failed to kill process '{}' in environment '{}' by PID {}: {}",
                            name, environment_id, pid, e
                        );
                    } else {
                        info!(
                            "Successfully killed process '{}' in environment '{}' by PID {}",
                            name, environment_id, pid
                        );
                        process_killed = true;
                    }
                }
            }
        }

        // Only update database status if we successfully killed the process or it's not running
        if process_killed {
            // Update process status to stopped in database
            self.process_repository
                .update_process_status(&process_id, "stopped")
                .await
                .map_err(|e| {
                    RunceptError::ProcessError(format!(
                        "Failed to update process status to stopped: {e}"
                    ))
                })?;

            // Clear the PID in database
            self.process_repository
                .clear_process_pid(&process_id)
                .await
                .map_err(|e| {
                    RunceptError::ProcessError(format!("Failed to clear process PID: {e}"))
                })?;
        } else {
            // If we couldn't kill the process, don't mark it as stopped
            // Check if the process is actually still alive
            if let Ok(Some(process)) = self.process_repository.get_process_by_id(&process_id).await
            {
                if let Some(pid) = process.pid {
                    if Self::is_process_alive(pid as u32) {
                        return Err(RunceptError::ProcessError(format!(
                            "Failed to stop process '{name}' in environment '{environment_id}': process is still running with PID {pid}"
                        )));
                    } else {
                        // Process is not alive anymore, safe to mark as stopped
                        self.process_repository
                            .update_process_status(&process_id, "stopped")
                            .await
                            .map_err(|e| {
                                RunceptError::ProcessError(format!(
                                    "Failed to update process status to stopped: {e}"
                                ))
                            })?;

                        self.process_repository
                            .clear_process_pid(&process_id)
                            .await
                            .map_err(|e| {
                                RunceptError::ProcessError(format!(
                                    "Failed to clear process PID: {e}"
                                ))
                            })?;
                    }
                } else {
                    // No PID, safe to mark as stopped
                    self.process_repository
                        .update_process_status(&process_id, "stopped")
                        .await
                        .map_err(|e| {
                            RunceptError::ProcessError(format!(
                                "Failed to update process status to stopped: {e}"
                            ))
                        })?;
                }
            }
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

        // Query processes from database
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
            .get_process_by_name_validated(environment_id, name)
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
            .get_process_by_name_validated(environment_id, name)
            .await?
            .is_some()
        {
            // If process is running, try to read logs from logger
            // First check if the process is currently running
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

    /// Clean up finished processes in the specified environment
    pub async fn cleanup_finished_processes(
        &mut self,
        environment_id: &str,
    ) -> Result<Vec<String>> {
        debug!(
            "Cleaning up finished processes in environment '{}'",
            environment_id
        );

        let mut finished_processes = Vec::new();

        // Query finished processes from database
        match self
            .process_repository
            .get_processes_by_environment(environment_id)
            .await
        {
            Ok(processes) => {
                for process in processes {
                    if matches!(process.status.as_str(), "stopped" | "failed" | "crashed") {
                        // Remove from tracking (handles only, process data is in DB)
                        self.remove_process_from_tracking(&process.name, environment_id);
                        finished_processes.push(process.name);
                    }
                }
            }
            Err(e) => {
                error!(
                    "Failed to query processes for environment '{}': {}",
                    environment_id, e
                );
            }
        }

        Ok(finished_processes)
    }

    /// Stop all processes in the specified environment
    pub async fn stop_all_processes(&mut self, environment_id: &str) -> Result<()> {
        info!("Stopping all processes in environment '{}'", environment_id);

        // Get all processes for this environment from database
        match self
            .process_repository
            .get_processes_by_environment(environment_id)
            .await
        {
            Ok(processes) => {
                for process in processes {
                    if let Err(e) = self.stop_process(&process.name, environment_id).await {
                        warn!(
                            "Failed to stop process '{}' in environment '{}': {}",
                            process.name, environment_id, e
                        );
                    }
                }
            }
            Err(e) => {
                error!(
                    "Failed to query processes for environment '{}': {}",
                    environment_id, e
                );
            }
        }

        Ok(())
    }

    /// Start a process with a specific definition and working directory
    async fn start_process_with_definition(
        &mut self,
        process_def: &ProcessDefinition,
        working_dir: &Path,
        environment_id: &str,
    ) -> Result<String> {
        let mut process = Process::from_definition(
            process_def,
            working_dir.to_string_lossy().to_string(),
            environment_id.to_string(),
        );

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

        // Parse command and arguments using proper shell parsing
        let parts = shlex::split(&process_def.command).ok_or_else(|| {
            RunceptError::ProcessError(format!("Failed to parse command: {}", process_def.command))
        })?;

        if parts.is_empty() {
            return Err(RunceptError::ProcessError(
                "Command cannot be empty".to_string(),
            ));
        }

        let program = &parts[0];
        let args = &parts[1..];

        // Set up working directory
        let work_dir = if let Some(wd) = &process_def.working_dir {
            let wd_path = Path::new(wd);
            if wd_path.is_absolute() {
                wd_path.to_path_buf()
            } else if wd == "." {
                working_dir.to_path_buf()
            } else {
                working_dir.join(wd_path)
            }
        } else {
            working_dir.to_path_buf()
        };

        // Create command
        let mut cmd = Command::new(program);
        cmd.args(args)
            .current_dir(&work_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());

        // Create a new process group so we can kill the entire process tree
        #[cfg(unix)]
        {
            #[allow(unused_imports)]
            use std::os::unix::process::CommandExt;
            cmd.process_group(0); // Create new process group
        }

        // Add environment variables
        for (key, value) in &process.env_vars {
            cmd.env(key, value);
        }

        // Spawn the process
        let mut child = cmd.spawn().map_err(|e| {
            RunceptError::ProcessError(format!("Failed to start process '{}': {}", process.name, e))
        })?;

        // Get the PID and set it on our process struct
        let pid = child.id().map(|p| p as i64);
        process.pid = pid.map(|p| p as u32);

        // Process ID is already set by from_definition() method
        let process_id = process.id.clone();

        // Store process in database with "starting" status
        // Check if process record already exists and update it, otherwise insert new one
        if let Ok(Some(_existing_process)) =
            self.process_repository.get_process_by_id(&process_id).await
        {
            // Update existing process record
            self.process_repository
                .update_process_command(&process_id, &process_def.command)
                .await
                .map_err(|e| {
                    RunceptError::ProcessError(format!(
                        "Failed to update process '{}' command in database: {}",
                        process_def.name, e
                    ))
                })?;

            self.process_repository
                .update_process_working_dir(&process_id, &working_dir.to_string_lossy())
                .await
                .map_err(|e| {
                    RunceptError::ProcessError(format!(
                        "Failed to update process '{}' working directory in database: {}",
                        process_def.name, e
                    ))
                })?;

            self.process_repository
                .update_process_status(&process_id, "starting")
                .await
                .map_err(|e| {
                    RunceptError::ProcessError(format!(
                        "Failed to update process '{}' status in database: {}",
                        process_def.name, e
                    ))
                })?;

            if let Some(pid) = pid {
                self.process_repository
                    .set_process_pid(&process_id, pid)
                    .await
                    .map_err(|e| {
                        RunceptError::ProcessError(format!(
                            "Failed to set process '{}' PID in database: {}",
                            process_def.name, e
                        ))
                    })?;
            }
        } else {
            // Insert new process record using the Process struct
            self.process_repository
                .insert_process(&process)
                .await
                .map_err(|e| {
                    RunceptError::ProcessError(format!(
                        "Failed to store process '{}' in database: {}",
                        process_def.name, e
                    ))
                })?;
        }

        // Create process logger
        let logger = ProcessLogger::new(process_def.name.clone(), working_dir.to_path_buf())
            .await
            .map_err(|e| {
                RunceptError::ProcessError(format!(
                    "Failed to create logger for process '{}': {}",
                    process_def.name, e
                ))
            })?;

        // Capture stdout and stderr
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| RunceptError::ProcessError("Failed to capture stdout".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| RunceptError::ProcessError("Failed to capture stderr".to_string()))?;

        // Update process status to running in database
        self.process_repository
            .update_process_status(&process_id, "running")
            .await
            .map_err(|e| {
                RunceptError::ProcessError(format!(
                    "Failed to update process '{}' status to running: {}",
                    process_def.name, e
                ))
            })?;

        // Log lifecycle event: process spawned successfully
        if let Ok(mut logger) =
            ProcessLogger::new(process_def.name.clone(), working_dir.to_path_buf()).await
        {
            let _ = logger
                .log_lifecycle(&format!(
                    "Process '{}' spawned successfully with PID {}",
                    process_def.name,
                    pid.unwrap_or(0)
                ))
                .await;
        }

        let process_name = process_def.name.clone();

        // Set up shutdown channel
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>(1);

        // Store process handle with Arc<Mutex<>> for shared access
        let child_handle = Arc::new(Mutex::new(Some(child)));
        let logger_handle = Arc::new(Mutex::new(logger));
        let handle = ProcessHandle {
            child: Arc::clone(&child_handle),
            shutdown_tx,
            logger: Arc::clone(&logger_handle),
        };

        // Initialize environment maps if they don't exist
        self.process_handles
            .entry(environment_id.to_string())
            .or_default();

        // Store only the process handle (process data is in database)
        self.process_handles
            .get_mut(environment_id)
            .unwrap()
            .insert(process_name.clone(), handle);

        // Spawn logging and monitoring tasks
        Self::spawn_output_logging_task(stdout, Arc::clone(&logger_handle), true);
        Self::spawn_output_logging_task(stderr, Arc::clone(&logger_handle), false);

        // Spawn process monitoring task with proper shutdown handling
        Self::spawn_process_monitoring_task(
            Arc::clone(&child_handle),
            process_name.clone(),
            shutdown_rx,
            environment_id.to_string(),
            self.exit_notification_tx.clone(),
            working_dir.to_path_buf(),
        );

        Ok(process_id)
    }

    /// Helper method to remove a process from tracking collections
    /// (Only removes handles now, process data is in database)
    fn remove_process_from_tracking(&mut self, name: &str, environment_id: &str) {
        if let Some(env_handles) = self.process_handles.get_mut(environment_id) {
            env_handles.remove(name);
        }
    }

    /// Check if a process is still alive by PID
    pub fn is_process_alive(pid: u32) -> bool {
        #[cfg(unix)]
        {
            use nix::sys::signal::kill;
            use nix::unistd::Pid;

            // Handle potential invalid PIDs gracefully
            if pid == 0 {
                return false;
            }

            let nix_pid = match Pid::from_raw(pid as i32) {
                pid if pid.as_raw() > 0 => pid,
                _ => return false,
            };

            // Use kill with signal 0 to check if process exists
            kill(nix_pid, None).is_ok()
        }

        #[cfg(not(unix))]
        {
            // On non-Unix systems, we'll implement a different approach
            // For now, just assume the process is still running
            true
        }
    }

    /// Kill a process and all its children by killing the entire process group
    async fn kill_process_by_pid(&self, pid: u32) -> Result<()> {
        #[cfg(unix)]
        {
            use nix::sys::signal::{Signal, kill};
            use nix::unistd::Pid;

            // Handle potential invalid PIDs gracefully
            if pid == 0 {
                return Err(RunceptError::ProcessError("Invalid PID: 0".to_string()));
            }

            let nix_pid = match Pid::from_raw(pid as i32) {
                pid if pid.as_raw() > 0 => pid,
                _ => return Err(RunceptError::ProcessError(format!("Invalid PID: {pid}"))),
            };

            // Kill the entire process group to ensure child processes are also terminated
            // Use negative PID to kill the process group
            let process_group_pid = Pid::from_raw(-(pid as i32));

            // First try SIGTERM (graceful shutdown) to the entire process group
            match kill(process_group_pid, Signal::SIGTERM) {
                Ok(_) => {
                    info!("Sent SIGTERM to process group {}", pid);

                    // Wait a bit for graceful shutdown
                    tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;

                    // Check if the main process is still alive
                    if Self::is_process_alive(pid) {
                        warn!(
                            "Process group {} did not exit gracefully, sending SIGKILL",
                            pid
                        );

                        // Force kill the entire process group with SIGKILL
                        match kill(process_group_pid, Signal::SIGKILL) {
                            Ok(_) => {
                                info!("Sent SIGKILL to process group {}", pid);
                                Ok(())
                            }
                            Err(e) => Err(RunceptError::ProcessError(format!(
                                "Failed to send SIGKILL to process group {pid}: {e}"
                            ))),
                        }
                    } else {
                        info!("Process group {} exited gracefully after SIGTERM", pid);
                        Ok(())
                    }
                }
                Err(e) => {
                    // If process group kill fails, fallback to killing just the parent process
                    warn!(
                        "Failed to kill process group {}, falling back to single process kill: {}",
                        pid, e
                    );

                    // Fallback: kill just the parent process
                    match kill(nix_pid, Signal::SIGTERM) {
                        Ok(_) => {
                            info!("Sent SIGTERM to process {} (fallback)", pid);

                            // Wait a bit for graceful shutdown
                            tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;

                            // Check if process is still alive
                            if Self::is_process_alive(pid) {
                                warn!("Process {} did not exit gracefully, sending SIGKILL", pid);

                                // Force kill with SIGKILL
                                match kill(nix_pid, Signal::SIGKILL) {
                                    Ok(_) => {
                                        info!("Sent SIGKILL to process {}", pid);
                                        Ok(())
                                    }
                                    Err(e) => Err(RunceptError::ProcessError(format!(
                                        "Failed to send SIGKILL to process {pid}: {e}"
                                    ))),
                                }
                            } else {
                                info!("Process {} exited gracefully after SIGTERM", pid);
                                Ok(())
                            }
                        }
                        Err(e) => Err(RunceptError::ProcessError(format!(
                            "Failed to send SIGTERM to process {pid}: {e}"
                        ))),
                    }
                }
            }
        }

        #[cfg(not(unix))]
        {
            // On non-Unix systems, we'll implement a different approach
            // For now, return an error indicating it's not supported
            Err(RunceptError::ProcessError(
                "Process killing by PID is not supported on this platform".to_string(),
            ))
        }
    }

    /// Process any pending exit notifications
    pub async fn process_exit_notifications(&mut self) -> Result<()> {
        // Process all pending notifications
        while let Ok(notification) = self.exit_notification_rx.try_recv() {
            debug!(
                "Processing exit notification for process '{}' in environment '{}'",
                notification.process_name, notification.environment_id
            );

            // Create process ID
            let process_id = format!(
                "{}:{}",
                notification.environment_id, notification.process_name
            );

            // Update process status to stopped in database
            if let Err(e) = self
                .process_repository
                .update_process_status(&process_id, "stopped")
                .await
            {
                error!(
                    "Failed to update process status in database for '{}' in environment '{}': {}",
                    notification.process_name, notification.environment_id, e
                );
            }

            // Also clear the PID in the database
            if let Err(e) = self.process_repository.clear_process_pid(&process_id).await {
                error!(
                    "Failed to clear process PID in database for '{}' in environment '{}': {}",
                    notification.process_name, notification.environment_id, e
                );
            }

            info!(
                "Process '{}' in environment '{}' marked as stopped after exit with status: {}",
                notification.process_name, notification.environment_id, notification.exit_status
            );

            // Remove the process handle since it's no longer needed
            if let Some(env_handles) = self.process_handles.get_mut(&notification.environment_id) {
                env_handles.remove(&notification.process_name);
            }
        }

        Ok(())
    }

    /// Clean up stale processes that are marked as running in the database but no longer exist
    pub async fn cleanup_stale_processes(&mut self) -> Result<()> {
        info!("Cleaning up stale processes from database");

        // Get all processes marked as running in the database
        let running_processes = match self.process_repository.get_running_processes().await {
            Ok(processes) => processes,
            Err(e) => {
                error!("Failed to query running processes: {}", e);
                return Ok(()); // Continue with startup even if we can't query
            }
        };

        let mut cleaned_up = 0;

        for (process_id, environment_id, pid) in running_processes {
            let should_cleanup = if let Some(pid) = pid {
                // Check if the process is still alive
                !Self::is_process_alive(pid as u32)
            } else {
                // No PID means it was never properly started
                true
            };

            if should_cleanup {
                info!(
                    "Cleaning up stale process '{}' in environment '{}' (PID: {:?})",
                    process_id, environment_id, pid
                );

                // Update status in database
                if let Err(e) = self
                    .process_repository
                    .update_process_status(&process_id, "stopped")
                    .await
                {
                    error!(
                        "Failed to update process status for '{}': {}",
                        process_id, e
                    );
                }

                // Clear PID in database
                if let Err(e) = self.process_repository.clear_process_pid(&process_id).await {
                    error!("Failed to clear PID for process '{}': {}", process_id, e);
                }

                cleaned_up += 1;
            }
        }

        if cleaned_up > 0 {
            info!("Cleaned up {} stale processes", cleaned_up);
        } else {
            info!("No stale processes found");
        }

        Ok(())
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
            "Starting health check for process '{}' with URL '{}', timeout: {}s, check_timeout: {}s",
            process_name,
            health_url,
            timeout_secs,
            check_timeout.as_secs()
        );

        let health_check = HealthCheck::from_url(health_url, check_timeout).map_err(|e| {
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
            info!(
                "Executing health check #{} for process '{}' (elapsed: {:.2}s)",
                check_count,
                process_name,
                start_time.elapsed().as_secs_f64()
            );

            match health_check.execute().await {
                Ok(result) if result.is_healthy => {
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
                        process_name,
                        environment_id,
                        check_count,
                        result
                            .error_message
                            .unwrap_or_else(|| "Unknown error".to_string())
                    );
                }
                Err(e) => {
                    warn!(
                        "Health check error for process '{}' in environment '{}' (attempt #{}): {}",
                        process_name, environment_id, check_count, e
                    );
                }
            }

            // Wait a bit before retrying
            info!("Waiting 500ms before next health check attempt...");
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }

        Err(RunceptError::ProcessError(format!(
            "Health check timeout for process '{process_name}' after {timeout_secs} seconds"
        )))
    }

    /// Spawn a task to capture and log output from a process stream
    fn spawn_output_logging_task<T>(stream: T, logger: Arc<Mutex<ProcessLogger>>, is_stdout: bool)
    where
        T: tokio::io::AsyncRead + Send + Unpin + 'static,
    {
        tokio::spawn(async move {
            let mut reader = BufReader::new(stream);
            let mut line = String::new();

            while let Ok(n) = reader.read_line(&mut line).await {
                if n == 0 {
                    break; // EOF
                }

                let trimmed = line.trim_end();
                if !trimmed.is_empty() {
                    let mut logger = logger.lock().await;
                    let result = if is_stdout {
                        logger.log_stdout(trimmed).await
                    } else {
                        logger.log_stderr(trimmed).await
                    };
                    let _ = result;
                }
                line.clear();
            }
        });
    }

    /// Spawn a task to monitor process completion and handle shutdown signals
    fn spawn_process_monitoring_task(
        child_handle: Arc<Mutex<Option<Child>>>,
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
                            "Received shutdown signal for process '{}' in environment '{}'",
                            process_name, environment_id
                        );
                        // The actual termination is handled by the stop_process method
                        // This task just needs to clean up and exit
                    }
                }
            }
        });
    }
}
