#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::commands::{DaemonRequest, DaemonResponse};
    use crate::config::GlobalConfig;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_daemon_server_creation() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        let config = ServerConfig {
            socket_path: socket_path.clone(),
            global_config: GlobalConfig::default(),
            database: None,
        };

        let server = DaemonServer::new(config).await.unwrap();
        assert_eq!(server.config.socket_path, socket_path);
    }

    #[test]
    fn test_request_processing() {
        let request = DaemonRequest::GetDaemonStatus;
        let json = serde_json::to_string(&request).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();

        match parsed {
            DaemonRequest::GetDaemonStatus => {}
            _ => panic!("Expected GetDaemonStatus"),
        }
    }

    #[test]
    fn test_response_serialization() {
        let response = DaemonResponse::Success {
            message: "Test message".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: DaemonResponse = serde_json::from_str(&json).unwrap();

        match parsed {
            DaemonResponse::Success { message } => {
                assert_eq!(message, "Test message");
            }
            _ => panic!("Expected Success response"),
        }
    }
}

use crate::cli::commands::{
    DaemonRequest, DaemonResponse, DaemonStatusResponse, EnvironmentStatusResponse, LogLine,
    LogsResponse, ProcessInfo,
};
use crate::config::{
    Environment, EnvironmentManager, GlobalConfig, ProcessDefinition, ProjectConfig,
};
use crate::database::Database;
use crate::error::{Result, RunceptError};
use crate::process::ProcessManager;
use crate::scheduler::InactivityScheduler;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{RwLock, mpsc};
use tokio::time::Duration;

/// Configuration for the daemon server
pub struct ServerConfig {
    pub socket_path: PathBuf,
    pub global_config: GlobalConfig,
    pub database: Option<Database>,
}

/// Main daemon server that handles RPC requests
pub struct DaemonServer {
    pub config: ServerConfig,
    pub process_manager: Arc<RwLock<ProcessManager>>,
    pub environment_manager: Arc<RwLock<EnvironmentManager>>,
    pub inactivity_scheduler: Arc<RwLock<Option<InactivityScheduler>>>,
    pub start_time: SystemTime,
    pub shutdown_tx: Option<mpsc::Sender<()>>,
    pub current_environment_id: Arc<RwLock<Option<String>>>,
}

impl DaemonServer {
    /// Create a new daemon server
    pub async fn new(config: ServerConfig) -> Result<Self> {
        let process_manager = Arc::new(RwLock::new(
            ProcessManager::new(config.global_config.clone()).await?,
        ));

        // Create environment manager with optional database
        let database_pool = config.database.as_ref().map(|db| db.get_pool().clone());
        let environment_manager = Arc::new(RwLock::new(
            EnvironmentManager::new_with_database(config.global_config.clone(), database_pool)
                .await?,
        ));

        let inactivity_scheduler = Arc::new(RwLock::new(None));
        let current_environment_id = Arc::new(RwLock::new(None));

        Ok(Self {
            config,
            process_manager,
            environment_manager,
            inactivity_scheduler,
            start_time: SystemTime::now(),
            shutdown_tx: None,
            current_environment_id,
        })
    }

    /// Start the daemon server
    pub async fn run(&mut self) -> Result<()> {
        // Remove existing socket file if it exists
        if self.config.socket_path.exists() {
            tokio::fs::remove_file(&self.config.socket_path)
                .await
                .map_err(RunceptError::IoError)?;
        }

        // Create parent directory if it doesn't exist
        if let Some(parent) = self.config.socket_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(RunceptError::IoError)?;
        }

        // Bind to Unix socket
        let listener = UnixListener::bind(&self.config.socket_path).map_err(|e| {
            RunceptError::ConnectionError(format!(
                "Failed to bind to socket {:?}: {}",
                self.config.socket_path, e
            ))
        })?;

        println!("Daemon listening on: {:?}", self.config.socket_path);

        // Create shutdown channel
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
        self.shutdown_tx = Some(shutdown_tx);

        loop {
            tokio::select! {
                // Handle incoming connections
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, _)) => {
                            let server = self.clone_handles();
                            tokio::spawn(async move {
                                if let Err(e) = server.handle_connection(stream).await {
                                    eprintln!("Error handling connection: {e}");
                                }
                            });
                        }
                        Err(e) => {
                            eprintln!("Error accepting connection: {e}");
                        }
                    }
                }

                // Handle shutdown signal
                _ = shutdown_rx.recv() => {
                    println!("Received shutdown signal");
                    break;
                }
            }
        }

        // Cleanup
        self.shutdown().await?;
        Ok(())
    }

    /// Clone handles for spawned tasks
    pub fn clone_handles(&self) -> ServerHandles {
        ServerHandles {
            process_manager: Arc::clone(&self.process_manager),
            environment_manager: Arc::clone(&self.environment_manager),
            inactivity_scheduler: Arc::clone(&self.inactivity_scheduler),
            current_environment_id: Arc::clone(&self.current_environment_id),
            start_time: self.start_time,
            socket_path: self.config.socket_path.clone(),
            global_config: self.config.global_config.clone(),
            shutdown_tx: self.shutdown_tx.clone(),
        }
    }

    /// Shutdown the daemon
    pub async fn shutdown(&mut self) -> Result<()> {
        // Stop inactivity scheduler
        if let Some(mut scheduler) = self.inactivity_scheduler.write().await.take() {
            let _ = scheduler.stop().await;
        }

        // Stop all processes
        let mut process_manager = self.process_manager.write().await;
        process_manager.stop_all_processes().await?;

        // Remove socket file
        if self.config.socket_path.exists() {
            tokio::fs::remove_file(&self.config.socket_path)
                .await
                .map_err(RunceptError::IoError)?;
        }

        Ok(())
    }

    /// Send shutdown signal
    pub async fn request_shutdown(&self) -> Result<()> {
        if let Some(tx) = &self.shutdown_tx {
            tx.send(()).await.map_err(|_| {
                RunceptError::ProcessError("Failed to send shutdown signal".to_string())
            })?;
        }
        Ok(())
    }
}

/// Handles for spawned connection tasks
#[derive(Clone)]
pub struct ServerHandles {
    process_manager: Arc<RwLock<ProcessManager>>,
    environment_manager: Arc<RwLock<EnvironmentManager>>,
    inactivity_scheduler: Arc<RwLock<Option<InactivityScheduler>>>,
    current_environment_id: Arc<RwLock<Option<String>>>,
    start_time: SystemTime,
    socket_path: PathBuf,
    global_config: GlobalConfig,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl ServerHandles {
    /// Get all processes for the current environment, combining configured and running processes
    async fn get_environment_processes(&self) -> Result<(String, Vec<ProcessInfo>)> {
        let env_manager = self.environment_manager.read().await;
        let process_manager = self.process_manager.read().await;
        let current_env_id = self.current_environment_id.read().await.clone();

        let (environment_name, configured_processes) = if let Some(env_id) = &current_env_id {
            if let Some(env) = env_manager.get_environment(env_id) {
                (env.name.clone(), env.project_config.processes.clone())
            } else {
                ("unknown".to_string(), std::collections::HashMap::new())
            }
        } else {
            ("none".to_string(), std::collections::HashMap::new())
        };

        // Create a map of running processes for quick lookup
        let running_processes_map: std::collections::HashMap<String, &crate::process::Process> =
            process_manager
                .list_processes()
                .into_iter()
                .map(|(_id, process)| (process.name.clone(), process))
                .collect();

        // Build process list from configured processes, showing their current status
        let processes: Vec<ProcessInfo> = configured_processes
            .keys()
            .map(|name| {
                if let Some(running_process) = running_processes_map.get(name) {
                    // Process is running, show actual status
                    ProcessInfo {
                        id: running_process.id.clone(),
                        name: running_process.name.clone(),
                        status: running_process.status.to_string(),
                        pid: running_process.pid,
                        uptime: Some(
                            running_process
                                .created_at
                                .format("%Y-%m-%d %H:%M:%S")
                                .to_string(),
                        ),
                        environment: environment_name.clone(),
                        health_status: None, // TODO: Implement health status tracking
                        restart_count: 0,    // TODO: Implement restart count tracking
                        last_activity: running_process.last_activity.map(|t| format!("{t:?}")),
                    }
                } else {
                    // Process is configured but not running
                    ProcessInfo {
                        id: format!("{name}-configured"),
                        name: name.clone(),
                        status: "configured".to_string(),
                        pid: None,
                        uptime: None,
                        environment: environment_name.clone(),
                        health_status: None,
                        restart_count: 0,
                        last_activity: None,
                    }
                }
            })
            .collect();

        Ok((environment_name, processes))
    }
    /// Handle a single client connection
    async fn handle_connection(&self, stream: UnixStream) -> Result<()> {
        let mut reader = BufReader::new(stream);
        let mut request_line = String::new();

        // Read request line
        let bytes_read = reader
            .read_line(&mut request_line)
            .await
            .map_err(|e| RunceptError::ConnectionError(format!("Failed to read request: {e}")))?;

        if bytes_read == 0 {
            return Ok(()); // Client disconnected
        }

        // Parse request
        let request: DaemonRequest = serde_json::from_str(request_line.trim())
            .map_err(|e| RunceptError::SerializationError(format!("Invalid request: {e}")))?;

        // Process request
        let response = self.process_request(request).await?;

        // Send response
        let response_json = serde_json::to_string(&response).map_err(|e| {
            RunceptError::SerializationError(format!("Failed to serialize response: {e}"))
        })?;

        let mut stream = reader.into_inner();
        stream
            .write_all(format!("{response_json}\n").as_bytes())
            .await
            .map_err(|e| RunceptError::ConnectionError(format!("Failed to send response: {e}")))?;

        Ok(())
    }

    /// Process a daemon request and return response
    async fn process_request(&self, request: DaemonRequest) -> Result<DaemonResponse> {
        match request {
            // Environment commands
            DaemonRequest::ActivateEnvironment { path } => self.activate_environment(path).await,
            DaemonRequest::DeactivateEnvironment => self.deactivate_environment().await,
            DaemonRequest::GetEnvironmentStatus => self.get_environment_status().await,
            DaemonRequest::InitProject { path, force } => self.init_project(path, force).await,

            // Process commands
            DaemonRequest::StartProcess { name } => self.start_process(name).await,
            DaemonRequest::StopProcess { name } => self.stop_process(name).await,
            DaemonRequest::RestartProcess { name } => self.restart_process(name).await,
            DaemonRequest::ListProcesses => self.list_processes().await,
            DaemonRequest::GetProcessLogs { name, lines } => {
                self.get_process_logs(name, lines).await
            }

            // Global commands
            DaemonRequest::ListAllProcesses => self.list_all_processes().await,
            DaemonRequest::KillAllProcesses => self.kill_all_processes().await,

            // Daemon commands
            DaemonRequest::GetDaemonStatus => self.get_daemon_status().await,
            DaemonRequest::Shutdown => {
                // Trigger actual shutdown
                if let Err(e) = self.request_shutdown().await {
                    Ok(DaemonResponse::Error {
                        error: format!("Failed to initiate shutdown: {e}"),
                    })
                } else {
                    Ok(DaemonResponse::Success {
                        message: "Shutting down".to_string(),
                    })
                }
            }

            // Activity tracking commands
            DaemonRequest::RecordEnvironmentActivity { environment_id } => {
                self.record_environment_activity(environment_id).await
            }
            // Dynamic process management commands
            DaemonRequest::AddProcess {
                process,
                environment,
            } => self.add_process(process, environment).await,
            DaemonRequest::RemoveProcess { name, environment } => {
                self.remove_process(name, environment).await
            }
            DaemonRequest::UpdateProcess {
                name,
                process,
                environment,
            } => self.update_process(name, process, environment).await,
        }
    }

    /// Request shutdown from server handles
    pub async fn request_shutdown(&self) -> Result<()> {
        if let Some(shutdown_tx) = &self.shutdown_tx {
            shutdown_tx.send(()).await.map_err(|_| {
                RunceptError::ProcessError("Failed to send shutdown signal".to_string())
            })?;
        }
        Ok(())
    }

    /// Activate an environment
    async fn activate_environment(&self, path: Option<String>) -> Result<DaemonResponse> {
        let mut env_manager = self.environment_manager.write().await;

        let project_path = if let Some(path) = path {
            PathBuf::from(path)
        } else {
            std::env::current_dir().map_err(|e| {
                RunceptError::EnvironmentError(format!("Failed to get current directory: {e}"))
            })?
        };

        // Register project to create environment
        match env_manager.register_project(&project_path).await {
            Ok(env_id) => {
                // Activate the environment
                if let Err(e) = env_manager.activate_environment(&env_id).await {
                    return Ok(DaemonResponse::Error {
                        error: format!("Failed to activate environment: {e}"),
                    });
                }

                // Store current environment ID
                *self.current_environment_id.write().await = Some(env_id);

                // Start inactivity scheduler for the environment
                let mut scheduler_guard = self.inactivity_scheduler.write().await;
                if scheduler_guard.is_none() {
                    let scheduler = InactivityScheduler::new(self.global_config.clone());
                    *scheduler_guard = Some(scheduler);
                }

                Ok(DaemonResponse::Success {
                    message: format!("Environment activated: {}", project_path.display()),
                })
            }
            Err(e) => Ok(DaemonResponse::Error {
                error: format!("Failed to create environment: {e}"),
            }),
        }
    }

    /// Deactivate current environment
    async fn deactivate_environment(&self) -> Result<DaemonResponse> {
        let current_env_id = self.current_environment_id.read().await.clone();

        if let Some(env_id) = current_env_id {
            let mut env_manager = self.environment_manager.write().await;

            match env_manager.deactivate_environment(&env_id).await {
                Ok(_) => {
                    // Clear current environment
                    *self.current_environment_id.write().await = None;

                    // Stop inactivity scheduler
                    let mut scheduler_guard = self.inactivity_scheduler.write().await;
                    if let Some(mut scheduler) = scheduler_guard.take() {
                        let _ = scheduler.stop().await;
                    }

                    Ok(DaemonResponse::Success {
                        message: "Environment deactivated".to_string(),
                    })
                }
                Err(e) => Ok(DaemonResponse::Error {
                    error: format!("Failed to deactivate environment: {e}"),
                }),
            }
        } else {
            Ok(DaemonResponse::Error {
                error: "No active environment".to_string(),
            })
        }
    }

    /// Get environment status
    async fn get_environment_status(&self) -> Result<DaemonResponse> {
        let env_manager = self.environment_manager.read().await;
        let current_env_id = self.current_environment_id.read().await.clone();

        let (active_environment, project_path) = if let Some(env_id) = &current_env_id {
            if let Some(env) = env_manager.get_environment(env_id) {
                (
                    Some(env.name.clone()),
                    Some(env.project_path.to_string_lossy().to_string()),
                )
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        let (_env_name, processes) = self.get_environment_processes().await?;
        let running_processes = processes.iter().filter(|p| p.status == "running").count();
        let total_configured = processes.len();

        let status = EnvironmentStatusResponse {
            active_environment,
            project_path,
            processes: processes.clone(),
            total_processes: total_configured,
            running_processes,
        };

        Ok(DaemonResponse::EnvironmentStatus(status))
    }

    /// Initialize a new project with default configuration
    async fn init_project(&self, path: Option<String>, force: bool) -> Result<DaemonResponse> {
        use crate::config::project::ProjectConfig;
        use std::path::Path;

        let project_path = if let Some(p) = path {
            Path::new(&p).to_path_buf()
        } else {
            std::env::current_dir().map_err(|e| {
                RunceptError::IoError(std::io::Error::other(format!(
                    "Failed to get current directory: {e}"
                )))
            })?
        };

        let config_path = project_path.join(".runcept.toml");

        // Check if config already exists
        if config_path.exists() && !force {
            return Ok(DaemonResponse::Error {
                error: format!(
                    "Configuration file already exists: {}. Use --force to overwrite.",
                    config_path.display()
                ),
            });
        }

        // Get project name from directory name
        let project_name = project_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("my-project")
            .to_string();

        // Create default configuration
        let default_config = ProjectConfig::create_default(&project_name);

        // Save the configuration
        match default_config.save_to_path(&config_path).await {
            Ok(()) => Ok(DaemonResponse::Success {
                message: format!(
                    "Initialized new runcept project '{}' in {}",
                    project_name,
                    project_path.display()
                ),
            }),
            Err(e) => Ok(DaemonResponse::Error {
                error: format!("Failed to create configuration file: {e}"),
            }),
        }
    }

    /// Start a process
    async fn start_process(&self, name: String) -> Result<DaemonResponse> {
        let env_manager = self.environment_manager.read().await;
        let mut process_manager = self.process_manager.write().await;
        let current_env_id = self.current_environment_id.read().await.clone();

        // Get current environment
        let env_id = current_env_id
            .ok_or_else(|| RunceptError::EnvironmentError("No active environment".to_string()))?;
        let environment = env_manager.get_environment(&env_id).ok_or_else(|| {
            RunceptError::EnvironmentError(format!("Environment '{env_id}' not found"))
        })?;

        // Find process definition
        let process_def = environment
            .project_config
            .processes
            .get(&name)
            .ok_or_else(|| {
                RunceptError::ProcessError(format!("Process '{name}' not found in environment"))
            })?;

        match process_manager
            .start_process(process_def, &environment.project_path)
            .await
        {
            Ok(_) => Ok(DaemonResponse::Success {
                message: format!("Process '{name}' started"),
            }),
            Err(e) => Ok(DaemonResponse::Error {
                error: format!("Failed to start process '{name}': {e}"),
            }),
        }
    }

    /// Stop a process
    async fn stop_process(&self, name: String) -> Result<DaemonResponse> {
        let mut process_manager = self.process_manager.write().await;

        // Find process by name
        let process_id = process_manager
            .list_processes()
            .into_iter()
            .find(|(_, p)| p.name == name)
            .map(|(id, _)| id.clone())
            .ok_or_else(|| RunceptError::ProcessError(format!("Process '{name}' not found")))?;

        match process_manager.stop_process(&process_id).await {
            Ok(_) => Ok(DaemonResponse::Success {
                message: format!("Process '{name}' stopped"),
            }),
            Err(e) => Ok(DaemonResponse::Error {
                error: format!("Failed to stop process '{name}': {e}"),
            }),
        }
    }

    /// Restart a process
    async fn restart_process(&self, name: String) -> Result<DaemonResponse> {
        let mut process_manager = self.process_manager.write().await;

        // Find process by name
        let process_id = process_manager
            .list_processes()
            .into_iter()
            .find(|(_, p)| p.name == name)
            .map(|(id, _)| id.clone())
            .ok_or_else(|| RunceptError::ProcessError(format!("Process '{name}' not found")))?;

        match process_manager.restart_process(&process_id).await {
            Ok(_) => Ok(DaemonResponse::Success {
                message: format!("Process '{name}' restarted"),
            }),
            Err(e) => Ok(DaemonResponse::Error {
                error: format!("Failed to restart process '{name}': {e}"),
            }),
        }
    }

    /// List processes in current environment
    async fn list_processes(&self) -> Result<DaemonResponse> {
        let (_env_name, processes) = self.get_environment_processes().await?;
        Ok(DaemonResponse::ProcessList(processes))
    }

    /// Get process logs
    async fn get_process_logs(&self, name: String, lines: Option<usize>) -> Result<DaemonResponse> {
        use crate::logging::log_debug;

        log_debug(
            "daemon",
            &format!("Getting logs for process '{name}'"),
            None,
        );
        let process_manager = self.process_manager.read().await;
        let env_manager = self.environment_manager.read().await;
        let current_env_id = self.current_environment_id.read().await.clone();
        log_debug(
            "daemon",
            &format!("Current environment ID: {current_env_id:?}"),
            None,
        );

        // First check if the process is currently running
        if let Some((process_id, _process)) = process_manager
            .list_processes()
            .into_iter()
            .find(|(_, p)| p.name == name)
        {
            log_debug(
                "daemon",
                &format!("Found running process with ID: {process_id}"),
                None,
            );
            // Process is running, get logs from process manager
            let log_entries = process_manager.get_process_logs(process_id, lines).await?;

            // Convert LogEntry to LogLine format expected by the response
            let log_lines: Vec<LogLine> = log_entries
                .into_iter()
                .map(|entry| LogLine {
                    timestamp: entry.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
                    level: entry.level.to_string(),
                    message: entry.message,
                })
                .collect();

            let total_lines = log_lines.len();

            return Ok(DaemonResponse::ProcessLogs(LogsResponse {
                process_name: name,
                lines: log_lines,
                total_lines,
            }));
        }

        // Process is not running, try to read logs from filesystem
        log_debug(
            "daemon",
            &format!("Process '{name}' not running, trying filesystem"),
            None,
        );

        // We need to get the current environment's project path
        let project_path = if let Some(env_id) = &current_env_id {
            if let Some(env) = env_manager.get_environment(env_id) {
                log_debug(
                    "daemon",
                    &format!("Using project path: {}", env.project_path.display()),
                    None,
                );
                env.project_path.clone()
            } else {
                return Err(RunceptError::EnvironmentError(
                    "No active environment".to_string(),
                ));
            }
        } else {
            return Err(RunceptError::EnvironmentError(
                "No active environment".to_string(),
            ));
        };

        // Check if this process name exists in the current environment configuration
        let env_id = current_env_id
            .ok_or_else(|| RunceptError::EnvironmentError("No active environment".to_string()))?;
        let environment = env_manager.get_environment(&env_id).ok_or_else(|| {
            RunceptError::EnvironmentError(format!("Environment '{env_id}' not found"))
        })?;

        if !environment.project_config.processes.contains_key(&name) {
            return Err(RunceptError::ProcessError(format!(
                "Process '{name}' not found in environment"
            )));
        }

        // Read logs from filesystem
        log_debug(
            "daemon",
            &format!(
                "Reading logs for '{name}' from '{}'",
                project_path.display()
            ),
            None,
        );
        let log_entries = crate::process::read_process_logs(&name, &project_path, lines).await?;
        log_debug(
            "daemon",
            &format!("Found {} log entries", log_entries.len()),
            None,
        );

        // Convert LogEntry to LogLine format expected by the response
        let log_lines: Vec<LogLine> = log_entries
            .into_iter()
            .map(|entry| LogLine {
                timestamp: entry.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
                level: entry.level.to_string(),
                message: entry.message,
            })
            .collect();

        let total_lines = log_lines.len();

        let logs = LogsResponse {
            process_name: name,
            lines: log_lines,
            total_lines,
        };

        Ok(DaemonResponse::ProcessLogs(logs))
    }

    /// List all processes across environments
    async fn list_all_processes(&self) -> Result<DaemonResponse> {
        // For now, same as list_processes since we only support one active environment
        self.list_processes().await
    }

    /// Kill all processes
    async fn kill_all_processes(&self) -> Result<DaemonResponse> {
        let mut process_manager = self.process_manager.write().await;

        match process_manager.stop_all_processes().await {
            Ok(_) => Ok(DaemonResponse::Success {
                message: "All processes stopped".to_string(),
            }),
            Err(e) => Ok(DaemonResponse::Error {
                error: format!("Failed to stop all processes: {e}"),
            }),
        }
    }

    /// Get daemon status
    async fn get_daemon_status(&self) -> Result<DaemonResponse> {
        // Clean up finished processes before reporting status
        if let Ok(mut process_manager) = self.process_manager.try_write() {
            let _ = process_manager.cleanup_finished_processes().await;
        }

        let process_manager = self.process_manager.read().await;
        let env_manager = self.environment_manager.read().await;

        let uptime = self.start_time.elapsed().unwrap_or(Duration::from_secs(0));
        let uptime_str = format!(
            "{}d {}h {}m {}s",
            uptime.as_secs() / 86400,
            (uptime.as_secs() % 86400) / 3600,
            (uptime.as_secs() % 3600) / 60,
            uptime.as_secs() % 60
        );

        let processes = process_manager.list_processes();

        // Count active environments using the environment manager
        let active_environments = env_manager.get_active_environments().len();

        let status = DaemonStatusResponse {
            running: true,
            uptime: uptime_str,
            version: env!("CARGO_PKG_VERSION").to_string(),
            socket_path: self.socket_path.to_string_lossy().to_string(),
            total_processes: processes.len(),
            active_environments,
            memory_usage: None, // TODO: Implement memory usage tracking
        };

        Ok(DaemonResponse::DaemonStatus(status))
    }

    /// Record environment activity for inactivity tracking
    async fn record_environment_activity(&self, environment_id: String) -> Result<DaemonResponse> {
        let inactivity_scheduler = self.inactivity_scheduler.read().await;

        if let Some(scheduler) = inactivity_scheduler.as_ref() {
            scheduler
                .activity_tracker
                .record_environment_activity(&environment_id)
                .await;
            Ok(DaemonResponse::Success {
                message: format!("Recorded activity for environment '{environment_id}'"),
            })
        } else {
            Ok(DaemonResponse::Error {
                error: "Inactivity scheduler not available".to_string(),
            })
        }
    }

    /// Resolve environment context - use provided environment or active environment
    async fn resolve_environment(
        &self,
        environment_override: Option<String>,
    ) -> Result<Option<(String, Environment)>> {
        let env_manager = self.environment_manager.read().await;

        if let Some(env_name) = environment_override {
            // Try to find the specified environment
            let environments = env_manager.get_active_environments();
            if let Some((id, env)) = environments.iter().find(|(id, _)| *id == &env_name) {
                Ok(Some((id.to_string(), (*env).clone())))
            } else {
                // Environment not found or not active
                Ok(None)
            }
        } else {
            // Use the first active environment (context-aware)
            // TODO: This might need to be enhanced based on context (e.g., current working directory)
            let environments = env_manager.get_active_environments();
            if let Some((id, env)) = environments.first() {
                Ok(Some((id.to_string(), (*env).clone())))
            } else {
                Ok(None)
            }
        }
    }

    /// Add a new process to the project configuration
    async fn add_process(
        &self,
        process: ProcessDefinition,
        environment: Option<String>,
    ) -> Result<DaemonResponse> {
        let (_env_id, resolved_env) = match self.resolve_environment(environment.clone()).await? {
            Some((id, env)) => (id, env),
            None => {
                let env_msg = environment
                    .map(|e| format!("Environment '{e}' not found or not active"))
                    .unwrap_or_else(|| "No active environment".to_string());
                return Ok(DaemonResponse::Error { error: env_msg });
            }
        };

        // Get the project config path
        let project_path = resolved_env.project_path.clone();
        let config_path = project_path.join(".runcept.toml");

        // Load current config
        let mut project_config = match ProjectConfig::load_from_path(&config_path).await {
            Ok(config) => config,
            Err(e) => {
                return Ok(DaemonResponse::Error {
                    error: format!("Failed to load project config: {e}"),
                });
            }
        };

        // Check if process already exists
        if project_config.processes.contains_key(&process.name) {
            return Ok(DaemonResponse::Error {
                error: format!(
                    "Process '{}' already exists in environment '{}'",
                    process.name, resolved_env.name
                ),
            });
        }

        // Add the new process
        project_config
            .processes
            .insert(process.name.clone(), process.clone());

        // Save the updated config
        match project_config.save_to_path(&config_path).await {
            Ok(_) => Ok(DaemonResponse::Success {
                message: format!(
                    "Process '{}' added successfully to environment '{}'",
                    process.name, resolved_env.name
                ),
            }),
            Err(e) => Ok(DaemonResponse::Error {
                error: format!("Failed to save project config: {e}"),
            }),
        }
    }

    /// Remove a process from the project configuration
    async fn remove_process(
        &self,
        name: String,
        environment: Option<String>,
    ) -> Result<DaemonResponse> {
        let (_env_id, resolved_env) = match self.resolve_environment(environment.clone()).await? {
            Some((id, env)) => (id, env),
            None => {
                let env_msg = environment
                    .map(|e| format!("Environment '{e}' not found or not active"))
                    .unwrap_or_else(|| "No active environment".to_string());
                return Ok(DaemonResponse::Error { error: env_msg });
            }
        };

        // Get the project config path
        let project_path = resolved_env.project_path.clone();
        let config_path = project_path.join(".runcept.toml");

        // Load current config
        let mut project_config = match ProjectConfig::load_from_path(&config_path).await {
            Ok(config) => config,
            Err(e) => {
                return Ok(DaemonResponse::Error {
                    error: format!("Failed to load project config: {e}"),
                });
            }
        };

        // Check if process exists
        if !project_config.processes.contains_key(&name) {
            return Ok(DaemonResponse::Error {
                error: format!(
                    "Process '{}' not found in environment '{}'",
                    name, resolved_env.name
                ),
            });
        }

        // Stop the process if it's running
        let mut process_manager = self.process_manager.write().await;
        let _ = process_manager.stop_process(&name).await;

        // Remove the process
        project_config.processes.remove(&name);

        // Save the updated config
        match project_config.save_to_path(&config_path).await {
            Ok(_) => Ok(DaemonResponse::Success {
                message: format!(
                    "Process '{}' removed successfully from environment '{}'",
                    name, resolved_env.name
                ),
            }),
            Err(e) => Ok(DaemonResponse::Error {
                error: format!("Failed to save project config: {e}"),
            }),
        }
    }

    /// Update an existing process configuration
    async fn update_process(
        &self,
        name: String,
        process: ProcessDefinition,
        environment: Option<String>,
    ) -> Result<DaemonResponse> {
        let (_env_id, resolved_env) = match self.resolve_environment(environment.clone()).await? {
            Some((id, env)) => (id, env),
            None => {
                let env_msg = environment
                    .map(|e| format!("Environment '{e}' not found or not active"))
                    .unwrap_or_else(|| "No active environment".to_string());
                return Ok(DaemonResponse::Error { error: env_msg });
            }
        };

        // Get the project config path
        let project_path = resolved_env.project_path.clone();
        let config_path = project_path.join(".runcept.toml");

        // Load current config
        let mut project_config = match ProjectConfig::load_from_path(&config_path).await {
            Ok(config) => config,
            Err(e) => {
                return Ok(DaemonResponse::Error {
                    error: format!("Failed to load project config: {e}"),
                });
            }
        };

        // Check if process exists
        if !project_config.processes.contains_key(&name) {
            return Ok(DaemonResponse::Error {
                error: format!(
                    "Process '{}' not found in environment '{}'",
                    name, resolved_env.name
                ),
            });
        }

        // Stop the process if it's running (will be restarted if needed)
        let mut process_manager = self.process_manager.write().await;
        let _ = process_manager.stop_process(&name).await;

        // Update the process (ensure the name matches)
        let mut updated_process = process;
        updated_process.name = name.clone();
        project_config
            .processes
            .insert(name.clone(), updated_process);

        // Save the updated config
        match project_config.save_to_path(&config_path).await {
            Ok(_) => Ok(DaemonResponse::Success {
                message: format!(
                    "Process '{}' updated successfully in environment '{}'",
                    name, resolved_env.name
                ),
            }),
            Err(e) => Ok(DaemonResponse::Error {
                error: format!("Failed to save project config: {e}"),
            }),
        }
    }
}
