#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::commands::{CliArgs, Commands, DaemonAction};
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_cli_handler_creation() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        let handler = CliHandler::new(Some(socket_path.clone()));
        assert_eq!(handler.client.socket_path, socket_path);
    }

    #[tokio::test]
    async fn test_extract_daemon_request_from_commands() {
        // Test environment commands
        let args = CliArgs {
            verbose: false,
            socket: None,
            command: Commands::Activate {
                path: Some("/test".to_string()),
            },
        };

        let request = CliHandler::command_to_request(&args.command).await.unwrap();
        match request {
            Some(DaemonRequest::ActivateEnvironment { path }) => {
                assert_eq!(path, Some("/test".to_string()));
            }
            _ => panic!("Expected ActivateEnvironment request"),
        }

        // Test process commands - these now require environment resolution
        // Skip this test for now since it requires a valid .runcept.toml file
        // TODO: Create a proper test with temp directory and .runcept.toml file
    }

    #[tokio::test]
    async fn test_daemon_commands_not_converted() {
        let args = CliArgs {
            verbose: false,
            socket: None,
            command: Commands::Daemon {
                action: DaemonAction::Start { foreground: false },
            },
        };

        // Daemon commands should not be converted to requests
        let result = CliHandler::command_to_request(&args.command).await.unwrap();
        assert!(result.is_none());
    }
}

use crate::cli::client::DaemonClient;
use crate::cli::commands::{CliArgs, CliResult, Commands, DaemonAction, DaemonRequest};
use crate::config::project::find_project_config;
use crate::error::{Result, RunceptError};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

/// Main CLI handler that processes commands and communicates with daemon
pub struct CliHandler {
    pub client: DaemonClient,
    pub verbose: bool,
}

impl CliHandler {
    pub fn new(socket_path: Option<PathBuf>) -> Self {
        let client = if let Some(path) = socket_path {
            DaemonClient::new(path)
        } else {
            DaemonClient::default()
        };

        Self {
            client,
            verbose: false,
        }
    }

    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// Main entry point for handling CLI commands
    pub async fn handle_command(&mut self, args: CliArgs) -> Result<CliResult> {
        self.verbose = args.verbose;

        // Update client socket path if provided
        if let Some(socket_path) = args.socket {
            self.client = DaemonClient::new(socket_path);
        }

        match &args.command {
            // Daemon management commands are handled locally
            Commands::Daemon { action } => self.handle_daemon_command(action).await,

            // All other commands require daemon communication
            _ => self.handle_daemon_request(&args.command).await,
        }
    }

    /// Handle daemon management commands (start, stop, status, restart)
    async fn handle_daemon_command(&self, action: &DaemonAction) -> Result<CliResult> {
        match action {
            DaemonAction::Start { foreground } => self.start_daemon(*foreground).await,
            DaemonAction::Stop => self.stop_daemon().await,
            DaemonAction::Restart => self.restart_daemon().await,
            DaemonAction::Status => self.daemon_status().await,
        }
    }

    /// Handle commands that require daemon communication
    async fn handle_daemon_request(&self, command: &Commands) -> Result<CliResult> {
        // Ensure daemon is running, auto-start if necessary
        if let Err(e) = crate::daemon::ensure_daemon_running_for_cli(
            self.client.socket_path.clone(),
            self.verbose,
        )
        .await
        {
            return Ok(CliResult::Error(format!(
                "Cannot connect to daemon and auto-start failed: {e}\nTry running: runcept daemon start"
            )));
        }

        // Convert command to daemon request with environment resolution
        let request = Self::command_to_request(command)
            .await?
            .ok_or_else(|| RunceptError::ProcessError("Invalid command for daemon".to_string()))?;

        if self.verbose {
            eprintln!("Sending request: {request:?}");
        }

        // Send request to daemon
        let response = self.client.send_request(request).await?;

        if self.verbose {
            eprintln!("Received response: {response:?}");
        }

        Ok(response.into())
    }

    /// Convert CLI command to daemon request with environment resolution
    async fn command_to_request(command: &Commands) -> Result<Option<DaemonRequest>> {
        let request = match command {
            // Environment commands
            Commands::Activate { path } => {
                // Convert path to absolute path for daemon
                let absolute_path = if let Some(p) = path {
                    // Convert to absolute path
                    std::path::Path::new(p)
                        .canonicalize()
                        .map(|abs_path| abs_path.to_string_lossy().to_string())
                        .unwrap_or_else(|_| p.clone()) // Fallback to original path if canonicalize fails
                } else {
                    // Use current working directory as absolute path
                    std::env::current_dir()
                        .map(|cwd| cwd.to_string_lossy().to_string())
                        .unwrap_or_else(|_| ".".to_string())
                };

                Some(DaemonRequest::ActivateEnvironment {
                    path: Some(absolute_path),
                })
            }
            Commands::Deactivate => Some(DaemonRequest::DeactivateEnvironment),
            Commands::Status => Some(DaemonRequest::GetEnvironmentStatus),
            Commands::Init { path, force } => {
                // Convert path to absolute path for daemon
                let absolute_path = if let Some(p) = path {
                    std::path::Path::new(p)
                        .canonicalize()
                        .map(|abs_path| abs_path.to_string_lossy().to_string())
                        .unwrap_or_else(|_| p.clone())
                } else {
                    std::env::current_dir()
                        .map(|cwd| cwd.to_string_lossy().to_string())
                        .unwrap_or_else(|_| ".".to_string())
                };
                Some(DaemonRequest::InitProject {
                    path: Some(absolute_path),
                    force: *force,
                })
            }

            // Process commands - require environment resolution
            Commands::Start { name, environment } => {
                let resolved_env = Self::resolve_environment(environment.as_ref()).await?;
                Some(DaemonRequest::StartProcess {
                    name: name.clone(),
                    environment: Some(resolved_env),
                })
            }
            Commands::Stop { name, environment } => {
                let resolved_env = Self::resolve_environment(environment.as_ref()).await?;
                Some(DaemonRequest::StopProcess {
                    name: name.clone(),
                    environment: Some(resolved_env),
                })
            }
            Commands::Restart { name, environment } => {
                let resolved_env = Self::resolve_environment(environment.as_ref()).await?;
                Some(DaemonRequest::RestartProcess {
                    name: name.clone(),
                    environment: Some(resolved_env),
                })
            }
            Commands::List { environment } => {
                let resolved_env = Self::resolve_environment(environment.as_ref()).await?;
                Some(DaemonRequest::ListProcesses {
                    environment: Some(resolved_env),
                })
            }
            Commands::Logs {
                name,
                lines,
                environment,
                ..
            } => {
                let resolved_env = Self::resolve_environment(environment.as_ref()).await?;
                Some(DaemonRequest::GetProcessLogs {
                    name: name.clone(),
                    lines: *lines,
                    environment: Some(resolved_env),
                })
            }

            // Global commands
            Commands::Ps => Some(DaemonRequest::ListAllProcesses),
            Commands::KillAll => Some(DaemonRequest::KillAllProcesses),

            // Daemon commands are handled locally, not sent to daemon
            Commands::Daemon { .. } => None,

            // MCP commands are handled locally, not sent to daemon
            Commands::Mcp => None,
        };

        Ok(request)
    }

    /// Resolve environment from CLI parameter or current working directory
    async fn resolve_environment(provided_env: Option<&String>) -> Result<String> {
        if let Some(env_path) = provided_env {
            // Use provided environment path, convert to absolute
            let absolute_path = std::path::Path::new(env_path)
                .canonicalize()
                .map(|abs_path| abs_path.to_string_lossy().to_string())
                .map_err(|e| {
                    RunceptError::EnvironmentError(format!(
                        "Invalid environment path '{env_path}': {e}"
                    ))
                })?;

            // Validate that this path contains a .runcept.toml file
            Self::validate_environment_path(&absolute_path).await?;
            Ok(absolute_path)
        } else {
            // Resolve from current working directory
            let cwd = std::env::current_dir().map_err(|e| {
                RunceptError::EnvironmentError(format!(
                    "Failed to get current working directory: {e}"
                ))
            })?;

            // Find .runcept.toml in current directory or parent directories
            let config_path = find_project_config(&cwd)
                .await?
                .ok_or_else(|| RunceptError::EnvironmentError(format!(
                    "No .runcept.toml configuration found in current directory or parent directories. \
                    Current directory: {}\n\n\
                    To fix this:\n\
                    1. Run 'runcept init' to create a new configuration\n\
                    2. Navigate to a directory with a .runcept.toml file\n\
                    3. Use --environment flag to specify an environment path",
                    cwd.display()
                )))?;

            // Return the directory containing the config file
            let project_dir = config_path
                .parent()
                .ok_or_else(|| {
                    RunceptError::EnvironmentError("Invalid project configuration path".to_string())
                })?
                .to_string_lossy()
                .to_string();

            Ok(project_dir)
        }
    }

    /// Validate that a path contains a valid .runcept.toml file
    async fn validate_environment_path(path: &str) -> Result<()> {
        let config_path = std::path::Path::new(path).join(".runcept.toml");

        if !config_path.exists() {
            return Err(RunceptError::EnvironmentError(format!(
                "Environment path '{path}' does not contain a .runcept.toml configuration file.\n\n\
                To fix this:\n\
                1. Run 'runcept init {path}' to create a new configuration\n\
                2. Verify the path points to a valid project directory"
            )));
        }

        // Try to load the config to ensure it's valid
        match crate::config::project::ProjectConfig::load_from_path(&config_path).await {
            Ok(_) => Ok(()),
            Err(e) => Err(RunceptError::EnvironmentError(format!(
                "Environment path '{path}' contains an invalid .runcept.toml file: {e}\n\n\
                To fix this:\n\
                1. Check the file syntax and format\n\
                2. Run 'runcept init {path} --force' to recreate the configuration"
            ))),
        }
    }

    /// Start the daemon process
    async fn start_daemon(&self, foreground: bool) -> Result<CliResult> {
        // Check if daemon is already running
        if self.client.is_daemon_running().await {
            return Ok(CliResult::Error("Daemon is already running".to_string()));
        }

        if self.verbose {
            eprintln!("Starting daemon with socket: {:?}", self.client.socket_path);
        }

        // Get current executable path
        let current_exe = std::env::current_exe().map_err(|e| {
            RunceptError::ProcessError(format!("Failed to get current executable: {e}"))
        })?;

        let mut cmd = Command::new(&current_exe);
        cmd.arg("daemon-process") // Special internal command for daemon process
            .arg("--socket")
            .arg(&self.client.socket_path);

        if !foreground {
            // Run in background
            cmd.stdout(Stdio::null())
                .stderr(Stdio::null())
                .stdin(Stdio::null());
        }

        if self.verbose {
            cmd.arg("--verbose");
        }

        let child = cmd
            .spawn()
            .map_err(|e| RunceptError::ProcessError(format!("Failed to start daemon: {e}")))?;

        if foreground {
            // Wait for process in foreground mode
            let status = child
                .wait_with_output()
                .await
                .map_err(|e| RunceptError::ProcessError(format!("Daemon process failed: {e}")))?;

            if status.status.success() {
                Ok(CliResult::Success("Daemon stopped".to_string()))
            } else {
                Ok(CliResult::Error("Daemon exited with error".to_string()))
            }
        } else {
            // In background mode, detach the child process
            // Give it a moment to start up
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            // Wait for daemon to be ready
            match self
                .client
                .wait_for_daemon(std::time::Duration::from_secs(10))
                .await
            {
                Ok(_) => Ok(CliResult::Success(
                    "Daemon started successfully".to_string(),
                )),
                Err(_) => {
                    // If daemon failed to start, try to clean up the socket
                    if let Ok(socket_path) = self.client.get_socket_path() {
                        let _ = std::fs::remove_file(&socket_path);
                    }
                    Ok(CliResult::Error(
                        "Daemon failed to start within timeout".to_string(),
                    ))
                }
            }
        }
    }

    /// Stop the daemon process
    async fn stop_daemon(&self) -> Result<CliResult> {
        if !self.client.is_daemon_running().await {
            return Ok(CliResult::Error("Daemon is not running".to_string()));
        }

        match self.client.shutdown_daemon().await {
            Ok(_) => Ok(CliResult::Success(
                "Daemon stopped successfully".to_string(),
            )),
            Err(e) => Ok(CliResult::Error(format!("Failed to stop daemon: {e}"))),
        }
    }

    /// Restart the daemon process
    async fn restart_daemon(&self) -> Result<CliResult> {
        if self.verbose {
            eprintln!("Restarting daemon...");
        }

        // Stop daemon if running
        if self.client.is_daemon_running().await {
            if let Err(e) = self.client.shutdown_daemon().await {
                return Ok(CliResult::Error(format!("Failed to stop daemon: {e}")));
            }

            // Wait for daemon to stop
            let mut attempts = 0;
            while self.client.is_daemon_running().await && attempts < 50 {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                attempts += 1;
            }

            if self.client.is_daemon_running().await {
                return Ok(CliResult::Error(
                    "Daemon failed to stop within timeout".to_string(),
                ));
            }
        }

        // Start daemon again
        self.start_daemon(false).await
    }

    /// Get daemon status
    async fn daemon_status(&self) -> Result<CliResult> {
        if !self.client.is_daemon_running().await {
            return Ok(CliResult::Success("Daemon Status: Not running".to_string()));
        }

        match self.client.get_daemon_status().await {
            Ok(response) => Ok(response.into()),
            Err(e) => Ok(CliResult::Error(format!(
                "Failed to get daemon status: {e}"
            ))),
        }
    }
}

/// Create a CLI handler from command line arguments
pub fn create_handler_from_args(args: &CliArgs) -> CliHandler {
    CliHandler::new(args.socket.clone()).with_verbose(args.verbose)
}
