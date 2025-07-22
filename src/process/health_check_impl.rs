use crate::error::{Result, RunceptError};
use crate::process::traits::{
    HealthCheckConfig, HealthCheckResult, HealthCheckTrait, HealthCheckType,
};
use async_trait::async_trait;
use reqwest::Client;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{debug, error, warn};

/// Default implementation of HealthCheckTrait
///
/// Consolidates the existing health check functionality from monitor.rs
/// into the trait-based architecture for better testability and abstraction.
pub struct DefaultHealthCheckService {
    http_client: Client,
}

impl DefaultHealthCheckService {
    pub fn new() -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self { http_client }
    }

    /// Execute a single health check with retries
    async fn execute_with_retries(&self, config: &HealthCheckConfig) -> Result<HealthCheckResult> {
        let mut last_error = None;

        for attempt in 1..=config.retries.max(1) {
            match self.execute_single_check(config).await {
                Ok(result) => {
                    if result.success || attempt == config.retries.max(1) {
                        return Ok(result);
                    }
                    last_error = Some(result.message.clone());
                }
                Err(e) => {
                    last_error = Some(e.to_string());
                    if attempt == config.retries.max(1) {
                        break;
                    }
                }
            }

            // Wait a bit before retrying (exponential backoff)
            if attempt < config.retries.max(1) {
                let wait_time = Duration::from_millis(100 * (1 << (attempt - 1)));
                tokio::time::sleep(wait_time).await;
            }
        }

        // All retries failed, return failed result
        Ok(HealthCheckResult {
            check_type: config.check_type.clone(),
            success: false,
            message: last_error
                .unwrap_or_else(|| "Health check failed after all retries".to_string()),
            duration_ms: 0,
            timestamp: chrono::Utc::now(),
        })
    }

    /// Execute a single health check attempt
    async fn execute_single_check(&self, config: &HealthCheckConfig) -> Result<HealthCheckResult> {
        let start_time = Instant::now();
        let timeout_duration = Duration::from_secs(config.timeout_secs);

        let result = match timeout(timeout_duration, self.execute_check_inner(config)).await {
            Ok(Ok(success)) => HealthCheckResult {
                check_type: config.check_type.clone(),
                success,
                message: if success {
                    "Health check passed".to_string()
                } else {
                    "Health check failed".to_string()
                },
                duration_ms: start_time.elapsed().as_millis() as u64,
                timestamp: chrono::Utc::now(),
            },
            Ok(Err(e)) => HealthCheckResult {
                check_type: config.check_type.clone(),
                success: false,
                message: e.to_string(),
                duration_ms: start_time.elapsed().as_millis() as u64,
                timestamp: chrono::Utc::now(),
            },
            Err(_) => HealthCheckResult {
                check_type: config.check_type.clone(),
                success: false,
                message: format!(
                    "Health check timed out after {} seconds",
                    config.timeout_secs
                ),
                duration_ms: start_time.elapsed().as_millis() as u64,
                timestamp: chrono::Utc::now(),
            },
        };

        Ok(result)
    }

    /// Execute the actual health check logic
    async fn execute_check_inner(&self, config: &HealthCheckConfig) -> Result<bool> {
        match &config.check_type {
            HealthCheckType::Http {
                url,
                expected_status,
            } => self.execute_http_check(url, *expected_status).await,
            HealthCheckType::Tcp { host, port } => self.execute_tcp_check(host, *port).await,
            HealthCheckType::Command {
                command,
                expected_exit_code,
            } => {
                self.execute_command_check(command, *expected_exit_code)
                    .await
            }
        }
    }

    /// Execute HTTP health check
    async fn execute_http_check(&self, url: &str, expected_status: u16) -> Result<bool> {
        debug!("Executing HTTP health check: {}", url);

        let response =
            self.http_client.get(url).send().await.map_err(|e| {
                RunceptError::HealthCheckError(format!("HTTP request failed: {}", e))
            })?;

        let status_code = response.status().as_u16();
        let success = status_code == expected_status;

        debug!(
            "HTTP health check result: status={}, expected={}, success={}",
            status_code, expected_status, success
        );

        Ok(success)
    }

    /// Execute TCP health check
    async fn execute_tcp_check(&self, host: &str, port: u16) -> Result<bool> {
        debug!("Executing TCP health check: {}:{}", host, port);

        match TcpStream::connect((host, port)).await {
            Ok(_) => {
                debug!("TCP health check passed: {}:{}", host, port);
                Ok(true)
            }
            Err(e) => {
                warn!("TCP health check failed: {}:{} - {}", host, port, e);
                Ok(false)
            }
        }
    }

    /// Execute command health check
    async fn execute_command_check(&self, command: &str, expected_exit_code: i32) -> Result<bool> {
        debug!("Executing command health check: {}", command);

        // Parse command (simple space-based splitting for now)
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return Err(RunceptError::HealthCheckError("Empty command".to_string()));
        }

        let mut cmd = Command::new(parts[0]);
        if parts.len() > 1 {
            cmd.args(&parts[1..]);
        }

        let output = cmd.output().await.map_err(|e| {
            RunceptError::HealthCheckError(format!("Command execution failed: {}", e))
        })?;

        let exit_code = output.status.code().unwrap_or(-1);
        let success = exit_code == expected_exit_code;

        debug!(
            "Command health check result: exit_code={}, expected={}, success={}",
            exit_code, expected_exit_code, success
        );

        if !success {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.is_empty() {
                debug!("Command stderr: {}", stderr);
            }
        }

        Ok(success)
    }
}

#[async_trait]
impl HealthCheckTrait for DefaultHealthCheckService {
    async fn execute_health_check(&self, config: &HealthCheckConfig) -> Result<HealthCheckResult> {
        self.execute_with_retries(config).await
    }

    async fn execute_health_checks(
        &self,
        configs: &[HealthCheckConfig],
    ) -> Result<Vec<HealthCheckResult>> {
        let mut results = Vec::new();

        // Execute all health checks concurrently
        let mut handles = Vec::new();

        for config in configs {
            let config_clone = config.clone();
            let service = DefaultHealthCheckService::new(); // Each task gets its own instance

            let handle =
                tokio::spawn(async move { service.execute_health_check(&config_clone).await });
            handles.push(handle);
        }

        // Collect all results
        for handle in handles {
            match handle.await {
                Ok(Ok(result)) => results.push(result),
                Ok(Err(e)) => {
                    error!("Health check failed: {}", e);
                    // Create a failed result
                    results.push(HealthCheckResult {
                        check_type: HealthCheckType::Http {
                            url: "unknown".to_string(),
                            expected_status: 200,
                        },
                        success: false,
                        message: e.to_string(),
                        duration_ms: 0,
                        timestamp: chrono::Utc::now(),
                    });
                }
                Err(e) => {
                    error!("Health check task panicked: {}", e);
                    results.push(HealthCheckResult {
                        check_type: HealthCheckType::Http {
                            url: "unknown".to_string(),
                            expected_status: 200,
                        },
                        success: false,
                        message: "Health check task panicked".to_string(),
                        duration_ms: 0,
                        timestamp: chrono::Utc::now(),
                    });
                }
            }
        }

        Ok(results)
    }
}

impl Default for DefaultHealthCheckService {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper function to create HealthCheckConfig from the legacy URL format
///
/// This maintains compatibility with existing health_check_url strings
/// while providing the new structured configuration.
pub fn health_check_from_url(
    url: &str,
    timeout_secs: u64,
    retries: u32,
) -> Result<HealthCheckConfig> {
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(HealthCheckConfig {
            check_type: HealthCheckType::Http {
                url: url.to_string(),
                expected_status: 200,
            },
            timeout_secs,
            retries,
        })
    } else if url.starts_with("tcp://") {
        let without_scheme = &url[6..]; // Remove "tcp://"
        let parts: Vec<&str> = without_scheme.split(':').collect();

        if parts.len() != 2 {
            return Err(RunceptError::HealthCheckError(format!(
                "Invalid TCP URL format: {}",
                url
            )));
        }

        let host = parts[0].to_string();
        let port: u16 = parts[1].parse().map_err(|_| {
            RunceptError::HealthCheckError(format!("Invalid port in TCP URL: {}", url))
        })?;

        Ok(HealthCheckConfig {
            check_type: HealthCheckType::Tcp { host, port },
            timeout_secs,
            retries,
        })
    } else if url.starts_with("cmd://") {
        let command = &url[6..]; // Remove "cmd://"

        Ok(HealthCheckConfig {
            check_type: HealthCheckType::Command {
                command: command.to_string(),
                expected_exit_code: 0,
            },
            timeout_secs,
            retries,
        })
    } else {
        Err(RunceptError::HealthCheckError(format!(
            "Unsupported health check URL scheme: {}",
            url
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_check_from_url_http() {
        let config = health_check_from_url("http://example.com/health", 10, 3).unwrap();

        match config.check_type {
            HealthCheckType::Http {
                url,
                expected_status,
            } => {
                assert_eq!(url, "http://example.com/health");
                assert_eq!(expected_status, 200);
            }
            _ => panic!("Expected HTTP health check"),
        }

        assert_eq!(config.timeout_secs, 10);
        assert_eq!(config.retries, 3);
    }

    #[tokio::test]
    async fn test_health_check_from_url_tcp() {
        let config = health_check_from_url("tcp://localhost:8080", 5, 1).unwrap();

        match config.check_type {
            HealthCheckType::Tcp { host, port } => {
                assert_eq!(host, "localhost");
                assert_eq!(port, 8080);
            }
            _ => panic!("Expected TCP health check"),
        }
    }

    #[tokio::test]
    async fn test_health_check_from_url_command() {
        let config = health_check_from_url("cmd://echo hello", 2, 1).unwrap();

        match config.check_type {
            HealthCheckType::Command {
                command,
                expected_exit_code,
            } => {
                assert_eq!(command, "echo hello");
                assert_eq!(expected_exit_code, 0);
            }
            _ => panic!("Expected Command health check"),
        }
    }

    #[tokio::test]
    async fn test_command_health_check_success() {
        let service = DefaultHealthCheckService::new();
        let config = HealthCheckConfig {
            check_type: HealthCheckType::Command {
                command: "echo hello".to_string(),
                expected_exit_code: 0,
            },
            timeout_secs: 5,
            retries: 1,
        };

        let result = service.execute_health_check(&config).await.unwrap();
        assert!(result.success);
        assert!(result.duration_ms < 1000); // Should be very fast
    }

    #[tokio::test]
    async fn test_command_health_check_failure() {
        let service = DefaultHealthCheckService::new();
        let config = HealthCheckConfig {
            check_type: HealthCheckType::Command {
                command: "exit 1".to_string(),
                expected_exit_code: 0,
            },
            timeout_secs: 5,
            retries: 1,
        };

        let result = service.execute_health_check(&config).await.unwrap();
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_tcp_health_check_failure() {
        let service = DefaultHealthCheckService::new();
        let config = HealthCheckConfig {
            check_type: HealthCheckType::Tcp {
                host: "localhost".to_string(),
                port: 65432, // Unlikely to be in use
            },
            timeout_secs: 1,
            retries: 1,
        };

        let result = service.execute_health_check(&config).await.unwrap();
        assert!(!result.success);
    }
}
