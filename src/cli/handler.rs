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

    #[test]
    fn test_extract_daemon_request_from_commands() {
        // Test environment commands
        let args = CliArgs {
            verbose: false,
            socket: None,
            command: Commands::Activate {
                path: Some("/test".to_string()),
            },
        };

        let request = CliHandler::command_to_request(&args.command).unwrap();
        match request {
            DaemonRequest::ActivateEnvironment { path } => {
                assert_eq!(path, Some("/test".to_string()));
            }
            _ => panic!("Expected ActivateEnvironment request"),
        }

        // Test process commands
        let args = CliArgs {
            verbose: false,
            socket: None,
            command: Commands::Start {
                name: "web-server".to_string(),
            },
        };

        let request = CliHandler::command_to_request(&args.command).unwrap();
        match request {
            DaemonRequest::StartProcess { name } => {
                assert_eq!(name, "web-server");
            }
            _ => panic!("Expected StartProcess request"),
        }
    }

    #[test]
    fn test_daemon_commands_not_converted() {
        let args = CliArgs {
            verbose: false,
            socket: None,
            command: Commands::Daemon {
                action: DaemonAction::Start { foreground: false },
            },
        };

        // Daemon commands should not be converted to requests
        let result = CliHandler::command_to_request(&args.command);
        assert!(result.is_none());
    }
}

use crate::cli::client::{DaemonClient, check_daemon_connection};
use crate::cli::commands::{CliArgs, CliResult, Commands, DaemonAction, DaemonRequest};
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
        // First check if daemon is running
        if let Err(e) = check_daemon_connection(Some(self.client.socket_path.clone())).await {
            return Ok(CliResult::Error(format!(
                "Cannot connect to daemon: {e}\nTry running: runcept daemon start"
            )));
        }

        // Convert command to daemon request
        let request = Self::command_to_request(command)
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

    /// Convert CLI command to daemon request
    fn command_to_request(command: &Commands) -> Option<DaemonRequest> {
        match command {
            // Environment commands
            Commands::Activate { path } => {
                Some(DaemonRequest::ActivateEnvironment { path: path.clone() })
            }
            Commands::Deactivate => Some(DaemonRequest::DeactivateEnvironment),
            Commands::Status => Some(DaemonRequest::GetEnvironmentStatus),

            // Process commands
            Commands::Start { name } => Some(DaemonRequest::StartProcess { name: name.clone() }),
            Commands::Stop { name } => Some(DaemonRequest::StopProcess { name: name.clone() }),
            Commands::Restart { name } => {
                Some(DaemonRequest::RestartProcess { name: name.clone() })
            }
            Commands::List => Some(DaemonRequest::ListProcesses),
            Commands::Logs { name, lines, .. } => Some(DaemonRequest::GetProcessLogs {
                name: name.clone(),
                lines: *lines,
            }),

            // Global commands
            Commands::Ps => Some(DaemonRequest::ListAllProcesses),
            Commands::KillAll => Some(DaemonRequest::KillAllProcesses),

            // Daemon commands are handled locally, not sent to daemon
            Commands::Daemon { .. } => None,
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
            // Wait for daemon to be ready
            match self
                .client
                .wait_for_daemon(std::time::Duration::from_secs(10))
                .await
            {
                Ok(_) => Ok(CliResult::Success(
                    "Daemon started successfully".to_string(),
                )),
                Err(_) => Ok(CliResult::Error(
                    "Daemon failed to start within timeout".to_string(),
                )),
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
