use crate::cli::commands::ProcessInfo;
use crate::config::ProcessDefinition;
use crate::error::{Result, RunceptError};
use crate::process::configuration::ProcessConfigurationManager;
use crate::process::{LogEntry, Process, ProcessLogger, ProcessStatus};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, RwLock, mpsc};
use tracing::{debug, error, info, warn};

/// Handle for a running process
pub struct ProcessHandle {
    pub child: Arc<Mutex<Option<Child>>>,
    pub shutdown_tx: mpsc::Sender<()>,
    pub logger: Arc<Mutex<ProcessLogger>>,
}

/// Manages the runtime execution of processes
pub struct ProcessRuntimeManager {
    /// Running processes mapped by environment_id -> process_name -> process
    pub processes: HashMap<String, HashMap<String, Process>>,
    /// Process handles mapped by environment_id -> process_name -> handle
    pub process_handles: HashMap<String, HashMap<String, ProcessHandle>>,
    /// Reference to configuration manager
    pub process_configuration_manager: Arc<RwLock<ProcessConfigurationManager>>,
    /// Global configuration
    pub global_config: crate::config::GlobalConfig,
}

impl ProcessRuntimeManager {
    /// Create a new ProcessRuntimeManager
    pub fn new(
        process_configuration_manager: Arc<RwLock<ProcessConfigurationManager>>,
        global_config: crate::config::GlobalConfig,
    ) -> Self {
        Self {
            processes: HashMap::new(),
            process_handles: HashMap::new(),
            process_configuration_manager,
            global_config,
        }
    }

    /// Start a process in the specified environment
    pub async fn start_process(&mut self, name: &str, environment_id: &str) -> Result<String> {
        info!(
            "Starting process '{}' in environment '{}'",
            name, environment_id
        );

        // Get process definition from configuration manager
        let process_def = {
            let config_manager = self.process_configuration_manager.read().await;
            config_manager.get_process_definition(name, environment_id).await?
        };

        // Get working directory from configuration manager
        let working_dir = {
            let config_manager = self.process_configuration_manager.read().await;
            config_manager.get_working_directory(environment_id).await?
        };

        // Check if process is already running
        if let Some(env_processes) = self.processes.get(environment_id) {
            if env_processes.contains_key(name) {
                return Err(RunceptError::ProcessError(format!(
                    "Process '{}' is already running in environment '{}'",
                    name, environment_id
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

        // Get environment processes
        let env_processes = self.processes.get_mut(environment_id).ok_or_else(|| {
            RunceptError::ProcessError(format!(
                "No processes found in environment '{}'",
                environment_id
            ))
        })?;

        let process = env_processes.get_mut(name).ok_or_else(|| {
            RunceptError::ProcessError(format!(
                "Process '{}' not found in environment '{}'",
                name, environment_id
            ))
        })?;

        if !process.is_running() {
            return Ok(()); // Already stopped
        }

        process.transition_to(ProcessStatus::Stopping);

        // Get process handle and send shutdown signal
        if let Some(env_handles) = self.process_handles.get(environment_id) {
            if let Some(handle) = env_handles.get(name) {
                let _ = handle.shutdown_tx.send(()).await;
            }
        }

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
        if let Some(env_processes) = self.processes.get_mut(environment_id) {
            env_processes.remove(name);
        }
        if let Some(env_handles) = self.process_handles.get_mut(environment_id) {
            env_handles.remove(name);
        }

        // Start the process again
        self.start_process(name, environment_id).await?;

        info!(
            "Successfully restarted process '{}' in environment '{}'",
            name, environment_id
        );
        Ok(())
    }

    /// List running processes in the specified environment
    pub fn list_running_processes(&self, environment_id: &str) -> Vec<ProcessInfo> {
        debug!(
            "Listing running processes in environment '{}'",
            environment_id
        );

        if let Some(env_processes) = self.processes.get(environment_id) {
            env_processes
                .values()
                .map(|process| {
                    ProcessInfo {
                        id: process.id.clone(),
                        name: process.name.clone(),
                        status: process.status.to_string(),
                        pid: process.pid,
                        uptime: Some(process.created_at.format("%Y-%m-%d %H:%M:%S").to_string()),
                        environment: environment_id.to_string(),
                        health_status: None, // TODO: Implement health status tracking
                        restart_count: 0,    // TODO: Implement restart count tracking
                        last_activity: process.last_activity.map(|t| format!("{:?}", t)),
                    }
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get the status of a specific process in the specified environment
    pub fn get_process_status(&self, name: &str, environment_id: &str) -> Option<ProcessStatus> {
        debug!(
            "Getting status for process '{}' in environment '{}'",
            name, environment_id
        );

        self.processes
            .get(environment_id)
            .and_then(|env_processes| env_processes.get(name))
            .map(|process| process.status.clone())
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

        // First update process statuses
        self.update_process_statuses(environment_id).await?;

        let mut finished_processes = Vec::new();

        if let Some(env_processes) = self.processes.get(environment_id) {
            let finished_names: Vec<String> = env_processes
                .iter()
                .filter(|(_, process)| {
                    matches!(
                        process.status,
                        ProcessStatus::Stopped | ProcessStatus::Failed | ProcessStatus::Crashed
                    ) && !process.auto_restart
                })
                .map(|(name, _)| name.clone())
                .collect();

            for name in finished_names {
                if let Some(env_processes) = self.processes.get_mut(environment_id) {
                    env_processes.remove(&name);
                }
                if let Some(env_handles) = self.process_handles.get_mut(environment_id) {
                    env_handles.remove(&name);
                }
                finished_processes.push(name);
            }
        }

        Ok(finished_processes)
    }

    /// Stop all processes in the specified environment
    pub async fn stop_all_processes(&mut self, environment_id: &str) -> Result<()> {
        info!("Stopping all processes in environment '{}'", environment_id);

        if let Some(env_processes) = self.processes.get(environment_id) {
            let process_names: Vec<String> = env_processes.keys().cloned().collect();

            for name in process_names {
                if let Err(e) = self.stop_process(&name, environment_id).await {
                    warn!(
                        "Failed to stop process '{}' in environment '{}': {}",
                        name, environment_id, e
                    );
                }
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
        if let Some(pid) = child.id() {
            process.set_pid(pid);
        }

        // Create process logger
        let logger = ProcessLogger::new(process.name.clone(), working_dir.to_path_buf())
            .await
            .map_err(|e| {
                RunceptError::ProcessError(format!(
                    "Failed to create logger for process '{}': {}",
                    process.name, e
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

        // Transition to running
        process.transition_to(ProcessStatus::Running);
        process.update_activity();

        let process_name = process.name.clone();
        let process_id = process.id.clone();

        // Set up shutdown channel
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

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
            .or_insert_with(HashMap::new);
        self.processes
            .entry(environment_id.to_string())
            .or_insert_with(HashMap::new);

        // Store process and handle
        self.process_handles
            .get_mut(environment_id)
            .unwrap()
            .insert(process_name.clone(), handle);
        self.processes
            .get_mut(environment_id)
            .unwrap()
            .insert(process_name.clone(), process);

        // Spawn logging and monitoring tasks
        let stdout_logger = Arc::clone(&logger_handle);
        let stderr_logger = Arc::clone(&logger_handle);

        // Spawn stdout capture task
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            
            while let Ok(n) = reader.read_line(&mut line).await {
                if n == 0 {
                    break; // EOF
                }
                
                let trimmed = line.trim_end();
                if !trimmed.is_empty() {
                    let mut logger = stdout_logger.lock().await;
                    let _ = logger.log_stdout(trimmed).await;
                }
                line.clear();
            }
        });

        // Spawn stderr capture task
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            
            while let Ok(n) = reader.read_line(&mut line).await {
                if n == 0 {
                    break; // EOF
                }
                
                let trimmed = line.trim_end();
                if !trimmed.is_empty() {
                    let mut logger = stderr_logger.lock().await;
                    let _ = logger.log_stderr(trimmed).await;
                }
                line.clear();
            }
        });

        // Spawn process monitoring task
        let child_monitor = Arc::clone(&child_handle);
        let process_name_monitor = process_name.clone();
        
        tokio::spawn(async move {
            // Wait for the process to complete
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                let mut child_guard = child_monitor.lock().await;
                if let Some(child) = child_guard.as_mut() {
                    match child.try_wait() {
                        Ok(Some(exit_status)) => {
                            info!("Process {} exited with status: {}", process_name_monitor, exit_status);
                            child_guard.take();
                            break;
                        }
                        Ok(None) => {
                            // Process still running
                        }
                        Err(e) => {
                            error!("Error checking process status for {}: {}", process_name_monitor, e);
                            child_guard.take();
                            break;
                        }
                    }
                } else {
                    // Child already exited
                    break;
                }
            }
        });

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
                            to_update.push((name.clone(), ProcessStatus::Stopped));
                            child_guard.take();
                        }
                        Ok(None) => {
                            // Process is still running
                        }
                        Err(_e) => {
                            to_update.push((name.clone(), ProcessStatus::Crashed));
                            child_guard.take();
                        }
                    }
                } else if let Some(env_processes) = self.processes.get(environment_id) {
                    if let Some(process) = env_processes.get(name) {
                        if matches!(
                            process.status,
                            ProcessStatus::Running | ProcessStatus::Stopping
                        ) {
                            to_update.push((name.clone(), ProcessStatus::Stopped));
                        }
                    }
                }
            }

            // Update process statuses
            for (name, new_status) in to_update {
                if let Some(env_processes) = self.processes.get_mut(environment_id) {
                    if let Some(process) = env_processes.get_mut(&name) {
                        process.transition_to(new_status);
                        process.clear_pid();
                    }
                }
            }
        }

        Ok(())
    }
}
