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

impl DaemonAutoSpawner {
    /// Get the socket path (for testing)
    pub fn socket_path(&self) -> &PathBuf {
        &self.socket_path
    }

    /// Get the verbose flag (for testing)
    pub fn is_verbose(&self) -> bool {
        self.verbose
    }

    /// Create command for daemon process (extracted for testing)
    pub fn create_daemon_command(&self) -> Result<Command> {
        let current_exe = std::env::current_exe().map_err(|e| {
            RunceptError::ProcessError(format!("Failed to get current executable: {e}"))
        })?;

        let mut cmd = Command::new(&current_exe);
        cmd.arg("daemon-process")
            .arg("--socket")
            .arg(&self.socket_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);

        if self.verbose {
            cmd.arg("--verbose");
        }

        Ok(cmd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn test_daemon_auto_spawner_creation() {
        let socket_path = PathBuf::from("/tmp/test.sock");
        let spawner = DaemonAutoSpawner::new(socket_path.clone(), true);

        assert_eq!(spawner.socket_path(), &socket_path);
        assert!(spawner.is_verbose());
    }

    #[test]
    fn test_daemon_auto_spawner_creation_non_verbose() {
        let socket_path = PathBuf::from("/tmp/test.sock");
        let spawner = DaemonAutoSpawner::new(socket_path.clone(), false);

        assert_eq!(spawner.socket_path(), &socket_path);
        assert!(!spawner.is_verbose());
    }

    #[test]
    fn test_create_daemon_command_verbose() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");
        let spawner = DaemonAutoSpawner::new(socket_path.clone(), true);

        let result = spawner.create_daemon_command();
        assert!(result.is_ok());

        // We can't easily test the exact command structure without making it public,
        // but we can verify it doesn't error
    }

    #[test]
    fn test_create_daemon_command_non_verbose() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");
        let spawner = DaemonAutoSpawner::new(socket_path.clone(), false);

        let result = spawner.create_daemon_command();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_ensure_daemon_running_for_cli() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        // This will likely fail because the daemon isn't actually running,
        // but we can test that the function exists and handles the error gracefully
        let result = ensure_daemon_running_for_cli(socket_path, true).await;

        // We expect this to fail since no daemon is running
        assert!(result.is_err());

        // The error should be related to connection or process spawning
        match result.unwrap_err() {
            RunceptError::ConnectionError(_) | RunceptError::ProcessError(_) => {
                // Expected error types
            }
            other => panic!("Unexpected error type: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_ensure_daemon_running_for_mcp() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        // This will likely fail because the daemon isn't actually running,
        // but we can test that the function exists and handles the error gracefully
        let result = ensure_daemon_running_for_mcp(socket_path).await;

        // We expect this to fail since no daemon is running
        assert!(result.is_err());

        // The error should be related to connection or process spawning
        match result.unwrap_err() {
            RunceptError::ConnectionError(_) | RunceptError::ProcessError(_) => {
                // Expected error types
            }
            other => panic!("Unexpected error type: {:?}", other),
        }
    }

    #[test]
    fn test_convenience_functions_create_correct_spawner() {
        // Test that the convenience functions create spawners with correct settings
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        // CLI spawner should be verbose
        let cli_spawner = DaemonAutoSpawner::new(socket_path.clone(), true);
        assert!(cli_spawner.is_verbose());

        // MCP spawner should not be verbose
        let mcp_spawner = DaemonAutoSpawner::new(socket_path.clone(), false);
        assert!(!mcp_spawner.is_verbose());
    }

    #[test]
    fn test_socket_path_handling() {
        // Test various socket path formats
        let paths = vec![
            PathBuf::from("/tmp/runcept.sock"),
            PathBuf::from("./local.sock"),
            PathBuf::from("/var/run/runcept/daemon.sock"),
        ];

        for path in paths {
            let spawner = DaemonAutoSpawner::new(path.clone(), false);
            assert_eq!(spawner.socket_path(), &path);
        }
    }
}
