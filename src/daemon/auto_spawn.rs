use crate::cli::client::DaemonClient;
use crate::error::{Result, RunceptError};
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use tracing::{debug, info};

/// Auto-spawn daemon functionality shared between CLI and MCP clients
pub struct DaemonAutoSpawner {
    socket_path: PathBuf,
    verbose: bool,
}

impl DaemonAutoSpawner {
    /// Create a new auto-spawner for the given socket path
    pub fn new(socket_path: PathBuf, verbose: bool) -> Self {
        Self {
            socket_path,
            verbose,
        }
    }

    /// Check if daemon is running and auto-start if not
    pub async fn ensure_daemon_running(&self) -> Result<()> {
        let client = DaemonClient::new(self.socket_path.clone());

        if client.is_daemon_running().await {
            debug!("Daemon is already running");
            return Ok(());
        }

        if self.verbose {
            info!("Daemon not running, attempting to auto-start...");
        }

        self.auto_start_daemon().await?;

        // Wait for daemon to be ready
        self.wait_for_daemon_ready(&client).await?;

        if self.verbose {
            info!("Daemon auto-started successfully");
        }

        Ok(())
    }

    /// Auto-start the daemon process
    async fn auto_start_daemon(&self) -> Result<()> {
        // Get current executable path
        let current_exe = std::env::current_exe().map_err(|e| {
            RunceptError::ProcessError(format!("Failed to get current executable: {e}"))
        })?;

        let mut cmd = Command::new(&current_exe);
        cmd.arg("daemon-process") // Special internal command for daemon process
            .arg("--socket")
            .arg(&self.socket_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);

        if self.verbose {
            cmd.arg("--verbose");
            debug!("Starting daemon with socket: {:?}", self.socket_path);
        }

        let child = cmd.spawn().map_err(|e| {
            RunceptError::ProcessError(format!("Failed to spawn daemon process: {e}"))
        })?;

        debug!("Daemon process spawned with PID: {:?}", child.id());

        // We don't wait for the child process - it should run in the background
        // The daemon will detach itself
        std::mem::forget(child);

        Ok(())
    }

    /// Wait for daemon to be ready for connections
    async fn wait_for_daemon_ready(&self, client: &DaemonClient) -> Result<()> {
        // Wait up to 10 seconds for daemon to start
        for attempt in 0..100 {
            if client.is_daemon_running().await {
                // Give it a moment to fully initialize
                tokio::time::sleep(Duration::from_millis(100)).await;
                return Ok(());
            }

            if attempt % 10 == 0 && self.verbose {
                debug!("Waiting for daemon to start... (attempt {})", attempt + 1);
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Err(RunceptError::ConnectionError(
            "Timeout waiting for daemon to start".to_string(),
        ))
    }
}

/// Convenience function for CLI usage
pub async fn ensure_daemon_running_for_cli(socket_path: PathBuf, verbose: bool) -> Result<()> {
    let spawner = DaemonAutoSpawner::new(socket_path, verbose);
    spawner.ensure_daemon_running().await
}

/// Convenience function for MCP usage
pub async fn ensure_daemon_running_for_mcp(socket_path: PathBuf) -> Result<()> {
    let spawner = DaemonAutoSpawner::new(socket_path, false); // MCP doesn't output to stdout
    spawner.ensure_daemon_running().await
}
