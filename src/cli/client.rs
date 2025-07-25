use crate::cli::commands::{DaemonRequest, DaemonResponse};
use crate::error::{Result, RunceptError};
use crate::ipc::{IpcPath, connect};
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

/// Client for communicating with the runcept daemon
pub struct DaemonClient {
    pub socket_path: IpcPath,
    pub timeout: Duration,
}

impl DaemonClient {
    pub fn new(socket_path: PathBuf) -> Self {
        Self {
            socket_path: IpcPath::from(socket_path),
            timeout: Duration::from_secs(30), // 30 second timeout
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Send a request to the daemon and get response
    pub async fn send_request(&self, request: DaemonRequest) -> Result<DaemonResponse> {
        // Serialize the request
        let request_json = serde_json::to_string(&request).map_err(|e| {
            RunceptError::SerializationError(format!("Failed to serialize request: {e}"))
        })?;

        // Connect to daemon socket
        let stream = timeout(self.timeout, connect(&self.socket_path))
            .await
            .map_err(|_| RunceptError::ConnectionError("Timeout connecting to daemon".to_string()))?
            .map_err(|e| {
                RunceptError::ConnectionError(format!(
                    "Failed to connect to daemon at {}: {e}",
                    self.socket_path.as_str()
                ))
            })?;

        let (mut reader, mut writer) = stream.into_split();

        // Send request
        let request_data = format!("{request_json}\n");
        writer
            .write_all(request_data.as_bytes())
            .await
            .map_err(|e| RunceptError::ConnectionError(format!("Failed to send request: {e}")))?;

        // Read response
        let mut response_buffer = Vec::new();
        reader
            .read_to_end(&mut response_buffer)
            .await
            .map_err(|e| RunceptError::ConnectionError(format!("Failed to read response: {e}")))?;

        let response_str = String::from_utf8(response_buffer).map_err(|e| {
            RunceptError::SerializationError(format!("Invalid UTF-8 in response: {e}"))
        })?;

        // Parse response
        let response: DaemonResponse = serde_json::from_str(&response_str).map_err(|e| {
            RunceptError::SerializationError(format!(
                "Failed to deserialize response '{}': {e}",
                response_str.trim()
            ))
        })?;

        Ok(response)
    }

    /// Check if the daemon is running by trying to connect
    pub async fn is_daemon_running(&self) -> bool {
        matches!(
            timeout(Duration::from_secs(1), connect(&self.socket_path),).await,
            Ok(Ok(_))
        )
    }

    /// Wait for daemon to be available (useful after starting daemon)
    pub async fn wait_for_daemon(&self, max_wait: Duration) -> Result<()> {
        let start = std::time::Instant::now();

        while start.elapsed() < max_wait {
            if self.is_daemon_running().await {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Err(RunceptError::ConnectionError(
            "Timeout waiting for daemon to start".to_string(),
        ))
    }

    /// Get the socket path for the daemon
    pub fn get_socket_path(&self) -> Result<PathBuf> {
        Ok(self.socket_path.as_path().to_path_buf())
    }
}

impl Default for DaemonClient {
    fn default() -> Self {
        let socket_path =
            IpcPath::default_path().unwrap_or_else(|_| IpcPath::from(get_default_socket_path()));
        Self {
            socket_path,
            timeout: Duration::from_secs(30),
        }
    }
}

/// Get the default socket path for the daemon
pub fn get_default_socket_path() -> PathBuf {
    if let Ok(config_dir) = crate::config::global::get_config_dir() {
        config_dir.join("daemon.sock")
    } else {
        // Fallback to /tmp if config dir unavailable
        PathBuf::from("/tmp/runcept-daemon.sock")
    }
}

/// High-level daemon operations
impl DaemonClient {
    /// Activate an environment
    pub async fn activate_environment(&self, path: Option<String>) -> Result<DaemonResponse> {
        self.send_request(DaemonRequest::ActivateEnvironment { path })
            .await
    }

    /// Deactivate current environment
    pub async fn deactivate_environment(&self) -> Result<DaemonResponse> {
        self.send_request(DaemonRequest::DeactivateEnvironment)
            .await
    }

    /// Get environment status
    pub async fn get_environment_status(&self) -> Result<DaemonResponse> {
        self.send_request(DaemonRequest::GetEnvironmentStatus).await
    }

    /// Start a process
    pub async fn start_process(&self, name: String) -> Result<DaemonResponse> {
        self.send_request(DaemonRequest::StartProcess {
            name,
            environment: None,
        })
        .await
    }

    /// Start a process with environment override
    pub async fn start_process_with_env(
        &self,
        name: String,
        environment: Option<String>,
    ) -> Result<DaemonResponse> {
        self.send_request(DaemonRequest::StartProcess { name, environment })
            .await
    }

    /// Stop a process
    pub async fn stop_process(&self, name: String) -> Result<DaemonResponse> {
        self.send_request(DaemonRequest::StopProcess {
            name,
            environment: None,
        })
        .await
    }

    /// Stop a process with environment override
    pub async fn stop_process_with_env(
        &self,
        name: String,
        environment: Option<String>,
    ) -> Result<DaemonResponse> {
        self.send_request(DaemonRequest::StopProcess { name, environment })
            .await
    }

    /// Restart a process
    pub async fn restart_process(&self, name: String) -> Result<DaemonResponse> {
        self.send_request(DaemonRequest::RestartProcess {
            name,
            environment: None,
        })
        .await
    }

    /// Restart a process with environment override
    pub async fn restart_process_with_env(
        &self,
        name: String,
        environment: Option<String>,
    ) -> Result<DaemonResponse> {
        self.send_request(DaemonRequest::RestartProcess { name, environment })
            .await
    }

    /// List processes in current environment
    pub async fn list_processes(&self) -> Result<DaemonResponse> {
        self.send_request(DaemonRequest::ListProcesses { environment: None })
            .await
    }

    /// List processes with environment override
    pub async fn list_processes_with_env(
        &self,
        environment: Option<String>,
    ) -> Result<DaemonResponse> {
        self.send_request(DaemonRequest::ListProcesses { environment })
            .await
    }

    /// Get process logs
    pub async fn get_process_logs(
        &self,
        name: String,
        lines: Option<usize>,
    ) -> Result<DaemonResponse> {
        self.send_request(DaemonRequest::GetProcessLogs {
            name,
            lines,
            environment: None,
        })
        .await
    }

    /// Get process logs with environment override
    pub async fn get_process_logs_with_env(
        &self,
        name: String,
        lines: Option<usize>,
        environment: Option<String>,
    ) -> Result<DaemonResponse> {
        self.send_request(DaemonRequest::GetProcessLogs {
            name,
            lines,
            environment,
        })
        .await
    }

    /// List all processes across environments
    pub async fn list_all_processes(&self) -> Result<DaemonResponse> {
        self.send_request(DaemonRequest::ListAllProcesses).await
    }

    /// Kill all processes
    pub async fn kill_all_processes(&self) -> Result<DaemonResponse> {
        self.send_request(DaemonRequest::KillAllProcesses).await
    }

    /// Get daemon status
    pub async fn get_daemon_status(&self) -> Result<DaemonResponse> {
        self.send_request(DaemonRequest::GetDaemonStatus).await
    }

    /// Shutdown daemon
    pub async fn shutdown_daemon(&self) -> Result<DaemonResponse> {
        self.send_request(DaemonRequest::Shutdown).await
    }
}

/// Helper for testing daemon connectivity
pub async fn check_daemon_connection(socket_path: Option<PathBuf>) -> Result<()> {
    let client = if let Some(path) = socket_path {
        DaemonClient::new(path)
    } else {
        DaemonClient::default()
    };

    if !client.is_daemon_running().await {
        return Err(RunceptError::ConnectionError(format!(
            "Daemon not running. Socket path: {:?}\nTry running: runcept daemon start",
            client.socket_path
        )));
    }

    // Test with a simple status request
    client.get_daemon_status().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_daemon_client_creation() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        let client = DaemonClient::new(socket_path.clone());
        assert_eq!(client.socket_path, socket_path);
    }

    #[test]
    fn test_get_default_socket_path() {
        let path = get_default_socket_path();
        assert!(path.to_string_lossy().contains(".runcept"));
        assert!(path.to_string_lossy().ends_with("daemon.sock"));
    }

    #[test]
    fn test_request_serialization() {
        let request = DaemonRequest::StartProcess {
            name: "test".to_string(),
            environment: None,
        };
        let serialized = serde_json::to_string(&request).unwrap();
        let deserialized: DaemonRequest = serde_json::from_str(&serialized).unwrap();

        match deserialized {
            DaemonRequest::StartProcess { name, environment } => {
                assert_eq!(name, "test");
                assert_eq!(environment, None);
            }
            _ => panic!("Unexpected request type"),
        }
    }

    #[test]
    fn test_response_serialization() {
        let response = DaemonResponse::Success {
            message: "OK".to_string(),
        };
        let serialized = serde_json::to_string(&response).unwrap();
        let deserialized: DaemonResponse = serde_json::from_str(&serialized).unwrap();

        match deserialized {
            DaemonResponse::Success { message } => assert_eq!(message, "OK"),
            _ => panic!("Unexpected response type"),
        }
    }
}
