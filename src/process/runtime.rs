use crate::cli::commands::ProcessInfo;
use crate::config::ProcessDefinition;
use crate::database::ProcessRepository;
use crate::error::{Result, RunceptError};
use crate::process::configuration::ProcessConfigurationManager;
use crate::process::{LogEntry, Process, ProcessLogger, ProcessStatus};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, RwLock, mpsc, broadcast};
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

    /// Start a process in the specified environment
    pub async fn start_process(&mut self, name: &str, environment_id: &str) -> Result<String> {
        info!(
            "Starting process '{}' in environment '{}'",
            name, environment_id
        );

        // Get process definition and working directory from configuration manager
        let (process_def, working_dir) = {
            let config_manager = self.process_configuration_manager.read().await;
            let process_def = config_manager
                .get_process_definition(name, environment_id)
                .await?;
            let working_dir = config_manager.get_working_directory(environment_id).await?;
            (process_def, working_dir)
        };

        // Check if process is already running by querying the database
        if let Ok(Some(existing_process)) = self.process_repository.get_process_by_id(&format!("{environment_id}:{name}")).await {
            if existing_process.status == "running" || existing_process.status == "starting" {
                return Err(RunceptError::ProcessError(format!(
                    "Process '{name}' is already running in environment '{environment_id}'"
                )));
            }
        }

        // Create and start the process
        let process_id = self
            .start_process_with_definition(&process_def, working_dir.as_path(), environment_id)
            .await?;

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

        // Check if process exists and is running in the database
        let process_id = format!("{}:{}", environment_id, name);
        let process_status = match self.process_repository.get_process_by_id(&process_id).await {
            Ok(Some(process)) => process.status,
            Ok(None) => {
                return Err(RunceptError::ProcessError(format!(
                    "Process '{name}' not found in environment '{environment_id}'"
                )));
            }
            Err(e) => {
                return Err(RunceptError::ProcessError(format!(
                    "Failed to query process status for '{}': {}",
                    process_id, e
                )));
            }
        };

        // Check if process is already stopped
        if process_status == "stopped" || process_status == "failed" || process_status == "crashed" {
            return Ok(()); // Already stopped
        }

        // Update process status to stopping in database
        self.process_repository.update_process_status(&process_id, "stopping").await
            .map_err(|e| RunceptError::ProcessError(format!(
                "Failed to update process status to stopping: {}", e
            )))?;

        // Get process handle and attempt graceful shutdown
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

        // Update process status to stopped in database
        self.process_repository.update_process_status(&process_id, "stopped").await
            .map_err(|e| RunceptError::ProcessError(format!(
                "Failed to update process status to stopped: {}", e
            )))?;
        
        // Clear the PID in database
        self.process_repository.clear_process_pid(&process_id).await
            .map_err(|e| RunceptError::ProcessError(format!(
                "Failed to clear process PID: {}", e
            )))?;

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

        // Stop the process first
        self.stop_process(name, environment_id).await?;

        // Wait a bit for the process to stop
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Remove the old process from tracking
        self.remove_process_from_tracking(name, environment_id);

        // Start the process again
        self.start_process(name, environment_id).await?;

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
        match self.process_repository.get_processes_by_environment(environment_id).await {
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
                error!("Failed to query processes for environment '{}': {}", environment_id, e);
                Vec::new()
            }
        }
    }

    /// Get the status of a specific process in the specified environment
    pub async fn get_process_status(&self, name: &str, environment_id: &str) -> Option<ProcessStatus> {
        debug!(
            "Getting status for process '{}' in environment '{}'",
            name, environment_id
        );

        let process_id = format!("{}:{}", environment_id, name);
        match self.process_repository.get_process_by_id(&process_id).await {
            Ok(Some(process)) => {
                match process.status.as_str() {
                    "running" => Some(ProcessStatus::Running),
                    "stopped" => Some(ProcessStatus::Stopped),
                    "starting" => Some(ProcessStatus::Starting),
                    "stopping" => Some(ProcessStatus::Stopping),
                    "failed" => Some(ProcessStatus::Failed),
                    "crashed" => Some(ProcessStatus::Crashed),
                    _ => Some(ProcessStatus::Stopped),
                }
            },
            Ok(None) => None,
            Err(e) => {
                error!("Failed to query process status for '{}': {}", process_id, e);
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
        match self.process_repository.get_processes_by_environment(environment_id).await {
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
                error!("Failed to query processes for environment '{}': {}", environment_id, e);
            }
        }

        Ok(finished_processes)
    }

    /// Stop all processes in the specified environment
    pub async fn stop_all_processes(&mut self, environment_id: &str) -> Result<()> {
        info!("Stopping all processes in environment '{}'", environment_id);

        // Get all processes for this environment from database
        match self.process_repository.get_processes_by_environment(environment_id).await {
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
                error!("Failed to query processes for environment '{}': {}", environment_id, e);
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
        let mut process = Process::new(
            process_def.name.clone(),
            process_def.command.clone(),
            working_dir.to_string_lossy().to_string(),
            environment_id.to_string(),
        );

        // Set up environment variables
        for (key, value) in &process_def.env_vars {
            process.set_env_var(key.clone(), value.clone());
        }

        // Set auto-restart from definition or global config
        process.auto_restart = process_def
            .auto_restart
            .unwrap_or(self.global_config.process.auto_restart_on_crash);

        // Set health check if provided
        if let Some(url) = &process_def.health_check_url {
            let interval = process_def
                .health_check_interval
                .unwrap_or(self.global_config.process.health_check_interval);
            process.set_health_check(url.clone(), interval);
        }

        // Transition to starting
        process.transition_to(ProcessStatus::Starting);

        // Parse command and arguments
        let parts: Vec<&str> = process_def.command.split_whitespace().collect();
        if parts.is_empty() {
            return Err(RunceptError::ProcessError(
                "Command cannot be empty".to_string(),
            ));
        }

        let program = parts[0];
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

        // Add environment variables
        for (key, value) in &process.env_vars {
            cmd.env(key, value);
        }

        // Spawn the process
        let mut child = cmd.spawn().map_err(|e| {
            RunceptError::ProcessError(format!("Failed to start process '{}': {}", process.name, e))
        })?;

        // Get the PID
        let pid = child.id().map(|p| p as i64);
        
        // Create process ID (unique identifier)
        let process_id = format!("{}:{}", environment_id, process_def.name);

        // Store process in database with "starting" status
        self.process_repository
            .insert_process(
                &process_id,
                &process_def.name,
                &process_def.command,
                &working_dir.to_string_lossy(),
                environment_id,
                "starting",
                pid,
            )
            .await
            .map_err(|e| {
                RunceptError::ProcessError(format!(
                    "Failed to store process '{}' in database: {}",
                    process_def.name, e
                ))
            })?;

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
        );

        Ok(process_id)
    }

    /// Update process statuses by checking if their child processes are still alive
    async fn update_process_statuses(&mut self, environment_id: &str) -> Result<()> {
        if let Some(env_handles) = self.process_handles.get(environment_id) {
            let mut to_update = Vec::new();

            for (name, handle) in env_handles {
                let mut child_guard = handle.child.lock().await;
                if let Some(child) = child_guard.as_mut() {
                    match child.try_wait() {
                        Ok(Some(_exit_status)) => {
                            to_update.push((name.clone(), "stopped"));
                            child_guard.take();
                        }
                        Ok(None) => {
                            // Process is still running
                        }
                        Err(_e) => {
                            to_update.push((name.clone(), "crashed"));
                            child_guard.take();
                        }
                    }
                } else {
                    // No child handle means process should be marked as stopped
                    to_update.push((name.clone(), "stopped"));
                }
            }

            // Update process statuses in database
            for (name, new_status) in to_update {
                let process_id = format!("{}:{}", environment_id, name);
                if let Err(e) = self.process_repository.update_process_status(&process_id, &new_status).await {
                    error!("Failed to update process status for '{}': {}", process_id, e);
                }
                if let Err(e) = self.process_repository.clear_process_pid(&process_id).await {
                    error!("Failed to clear process PID for '{}': {}", process_id, e);
                }
            }
        }

        Ok(())
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
            match kill(nix_pid, None) {
                Ok(_) => true,
                Err(_) => false,
            }
        }

        #[cfg(not(unix))]
        {
            // On non-Unix systems, we'll implement a different approach
            // For now, just assume the process is still running
            true
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
            let process_id = format!("{}:{}", notification.environment_id, notification.process_name);
            
            // Update process status to stopped in database
            if let Err(e) = self.process_repository.update_process_status(&process_id, "stopped").await {
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
                if let Err(e) = self.process_repository.update_process_status(&process_id, "stopped").await {
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
