use crate::error::{Result, RunceptError};
use crate::process::{HealthCheckResult, HealthCheckTrait, Process, ProcessStatus};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::interval;
use tracing::{debug, error, info, warn};

/// Health monitoring service that periodically checks process health
#[derive(Debug)]
pub struct HealthMonitor<H: HealthCheckTrait> {
    /// Map of process ID to last health check result
    health_status: Arc<RwLock<HashMap<String, HealthStatus>>>,
    /// Health check interval
    check_interval: Duration,
    /// Whether the monitor is running
    is_running: Arc<RwLock<bool>>,
    /// Health check service
    health_check_service: Arc<H>,
}

/// Health status for a process
#[derive(Debug, Clone)]
pub struct HealthStatus {
    /// Whether the last health check passed
    pub is_healthy: bool,
    /// Last check timestamp
    pub last_check: Instant,
    /// Number of consecutive failures
    pub consecutive_failures: u32,
    /// Error message if unhealthy
    pub error_message: Option<String>,
    /// Last health check result
    pub last_result: Option<HealthCheckResult>,
}

impl<H: HealthCheckTrait> HealthMonitor<H> {
    /// Create a new health monitor
    pub fn new(check_interval: Duration, health_check_service: Arc<H>) -> Self {
        Self {
            health_status: Arc::new(RwLock::new(HashMap::new())),
            check_interval,
            is_running: Arc::new(RwLock::new(false)),
            health_check_service,
        }
    }

    /// Start the health monitoring service
    pub async fn start(&self) -> Result<()> {
        let mut is_running = self.is_running.write().await;
        if *is_running {
            return Err(RunceptError::ProcessError(
                "Health monitor is already running".to_string(),
            ));
        }
        *is_running = true;
        info!(
            "Health monitor started with interval {:?}",
            self.check_interval
        );
        Ok(())
    }

    /// Stop the health monitoring service
    pub async fn stop(&self) -> Result<()> {
        let mut is_running = self.is_running.write().await;
        if !*is_running {
            return Err(RunceptError::ProcessError(
                "Health monitor is not running".to_string(),
            ));
        }
        *is_running = false;
        info!("Health monitor stopped");
        Ok(())
    }

    /// Check if the monitor is running
    pub async fn is_running(&self) -> bool {
        *self.is_running.read().await
    }

    /// Add a process to health monitoring
    pub async fn add_process(&self, process: &Process) -> Result<()> {
        if process.health_check_url.is_none() {
            debug!("Process '{}' has no health check configured", process.name);
            return Ok(());
        }

        let mut health_status = self.health_status.write().await;
        health_status.insert(
            process.id.clone(),
            HealthStatus {
                is_healthy: true,
                last_check: Instant::now(),
                consecutive_failures: 0,
                error_message: None,
                last_result: None,
            },
        );
        info!("Added process '{}' to health monitoring", process.name);
        Ok(())
    }

    /// Remove a process from health monitoring
    pub async fn remove_process(&self, process_id: &str) -> Result<()> {
        let mut health_status = self.health_status.write().await;
        if health_status.remove(process_id).is_some() {
            info!("Removed process '{}' from health monitoring", process_id);
        }
        Ok(())
    }

    /// Perform health check for a specific process
    pub async fn check_process_health(&self, process: &Process) -> Result<bool> {
        let health_check_url = match &process.health_check_url {
            Some(url) => url,
            None => {
                debug!("Process '{}' has no health check configured", process.name);
                return Ok(true);
            }
        };

        // Only check if process is running
        if process.status != ProcessStatus::Running {
            debug!(
                "Skipping health check for non-running process '{}'",
                process.name
            );
            return Ok(true);
        }

        // Convert URL to HealthCheckConfig
        let config = crate::process::health_check_from_url(
            health_check_url,
            process.health_check_interval.unwrap_or(30) as u64,
            3, // default retries
        )?;

        // Execute health check
        let result = self
            .health_check_service
            .execute_health_check(&config)
            .await?;

        // Update health status
        let mut health_status = self.health_status.write().await;
        if let Some(status) = health_status.get_mut(&process.id) {
            status.last_check = Instant::now();
            status.last_result = Some(result.clone());

            if result.success {
                if !status.is_healthy {
                    info!("Process '{}' health check recovered", process.name);
                }
                status.is_healthy = true;
                status.consecutive_failures = 0;
                status.error_message = None;
            } else {
                status.is_healthy = false;
                status.consecutive_failures += 1;
                status.error_message = Some(result.message.clone());

                if status.consecutive_failures == 1 {
                    warn!(
                        "Process '{}' health check failed: {}",
                        process.name, result.message
                    );
                } else {
                    warn!(
                        "Process '{}' health check failed {} times consecutively",
                        process.name, status.consecutive_failures
                    );
                }
            }
        }

        Ok(result.success)
    }

    /// Get health status for a process
    pub async fn get_health_status(&self, process_id: &str) -> Option<HealthStatus> {
        let health_status = self.health_status.read().await;
        health_status.get(process_id).cloned()
    }

    /// Get health status for all monitored processes
    pub async fn get_all_health_status(&self) -> HashMap<String, HealthStatus> {
        let health_status = self.health_status.read().await;
        health_status.clone()
    }

    /// Run periodic health checks
    pub async fn run_periodic_checks<F>(&self, get_processes: F) -> Result<()>
    where
        F: Fn() -> Vec<Process> + Send + Sync,
    {
        let mut interval = interval(self.check_interval);

        while self.is_running().await {
            interval.tick().await;

            let processes = get_processes();
            for process in processes {
                if let Err(e) = self.check_process_health(&process).await {
                    error!(
                        "Failed to check health for process '{}': {}",
                        process.name, e
                    );
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{Process, ProcessStatus};
    use crate::process::{MockHealthCheckTrait, HealthCheckResult, HealthCheckType};
    use std::sync::Arc;
    use std::time::Duration;

    fn create_test_process(name: &str, health_check_url: Option<String>) -> Process {
        let mut process = Process::new(
            name.to_string(),
            "echo test".to_string(),
            "/tmp".to_string(),
            "test-env".to_string(),
        );

        if let Some(url) = health_check_url {
            process.set_health_check(url, 30);
        }

        // Transition to running state
        process.transition_to(ProcessStatus::Starting);
        process.transition_to(ProcessStatus::Running);
        process
    }

    #[tokio::test]
    async fn test_health_monitor_lifecycle() {
        let mock_health = Arc::new(MockHealthCheckTrait::new());
        let monitor = HealthMonitor::new(Duration::from_secs(1), mock_health);

        // Initially not running
        assert!(!monitor.is_running().await);

        // Start monitor
        monitor.start().await.unwrap();
        assert!(monitor.is_running().await);

        // Cannot start again
        assert!(monitor.start().await.is_err());

        // Stop monitor
        monitor.stop().await.unwrap();
        assert!(!monitor.is_running().await);

        // Cannot stop again
        assert!(monitor.stop().await.is_err());
    }

    #[tokio::test]
    async fn test_add_remove_process() {
        let mock_health = Arc::new(MockHealthCheckTrait::new());
        let monitor = HealthMonitor::new(Duration::from_secs(1), mock_health);

        let process = create_test_process("test", Some("http://localhost:8080/health".to_string()));

        // Add process
        monitor.add_process(&process).await.unwrap();

        // Check it was added
        let status = monitor.get_health_status(&process.id).await;
        assert!(status.is_some());
        assert!(status.unwrap().is_healthy);

        // Remove process
        monitor.remove_process(&process.id).await.unwrap();

        // Check it was removed
        let status = monitor.get_health_status(&process.id).await;
        assert!(status.is_none());
    }

    #[tokio::test]
    async fn test_process_without_health_check() {
        let mock_health = Arc::new(MockHealthCheckTrait::new());
        let monitor = HealthMonitor::new(Duration::from_secs(1), mock_health);

        let process = create_test_process("test", None);

        // Add process without health check
        monitor.add_process(&process).await.unwrap();

        // Should not be in health status map
        let status = monitor.get_health_status(&process.id).await;
        assert!(status.is_none());

        // Health check should return true (healthy by default)
        let is_healthy = monitor.check_process_health(&process).await.unwrap();
        assert!(is_healthy);
    }

    #[tokio::test]
    async fn test_tcp_health_check() {
        let mut mock_health = MockHealthCheckTrait::new();
        mock_health
            .expect_execute_health_check()
            .returning(|_| {
                Ok(HealthCheckResult {
                    check_type: HealthCheckType::Http {
                        url: "http://localhost:8080".to_string(),
                        expected_status: 200,
                    },
                    success: true,
                    message: "Mock health check passed".to_string(),
                    duration_ms: 10,
                    timestamp: chrono::Utc::now(),
                })
            });
        let mock_health = Arc::new(mock_health);
        let monitor = HealthMonitor::new(Duration::from_secs(1), mock_health);

        let process = create_test_process("test", Some("tcp://127.0.0.1:22".to_string()));

        monitor.add_process(&process).await.unwrap();

        // This should succeed with mock
        let result = monitor.check_process_health(&process).await.unwrap();
        assert!(result);

        // Check that status was updated
        let status = monitor.get_health_status(&process.id).await;
        assert!(status.is_some());
        assert!(status.unwrap().is_healthy);
    }

    #[tokio::test]
    async fn test_command_health_check() {
        let mut mock_health = MockHealthCheckTrait::new();
        mock_health
            .expect_execute_health_check()
            .returning(|_| {
                Ok(HealthCheckResult {
                    check_type: HealthCheckType::Http {
                        url: "http://localhost:8080".to_string(),
                        expected_status: 200,
                    },
                    success: true,
                    message: "Mock health check passed".to_string(),
                    duration_ms: 10,
                    timestamp: chrono::Utc::now(),
                })
            });
        let mock_health = Arc::new(mock_health);
        let monitor = HealthMonitor::new(Duration::from_secs(1), mock_health);

        let process = create_test_process("test", Some("cmd://echo 'healthy'".to_string()));

        monitor.add_process(&process).await.unwrap();

        let is_healthy = monitor.check_process_health(&process).await.unwrap();
        assert!(is_healthy);

        let status = monitor.get_health_status(&process.id).await.unwrap();
        assert!(status.is_healthy);
        assert_eq!(status.consecutive_failures, 0);
    }

    #[tokio::test]
    async fn test_failing_command_health_check() {
        // Configure mock to return failure
        let mut mock_health = MockHealthCheckTrait::new();
        mock_health
            .expect_execute_health_check()
            .returning(|_| {
                Ok(HealthCheckResult {
                    check_type: HealthCheckType::Http {
                        url: "http://localhost:8080".to_string(),
                        expected_status: 200,
                    },
                    success: false,
                    message: "Mock health check failed".to_string(),
                    duration_ms: 10,
                    timestamp: chrono::Utc::now(),
                })
            });
        let mock_health = Arc::new(mock_health);
        let monitor = HealthMonitor::new(Duration::from_secs(1), mock_health);

        let process = create_test_process("test", Some("cmd://exit 1".to_string()));

        monitor.add_process(&process).await.unwrap();

        let is_healthy = monitor.check_process_health(&process).await.unwrap();
        assert!(!is_healthy);

        let status = monitor.get_health_status(&process.id).await.unwrap();
        assert!(!status.is_healthy);
        assert_eq!(status.consecutive_failures, 1);
        assert!(status.error_message.is_some());
    }

    #[tokio::test]
    async fn test_get_all_health_status() {
        let mock_health = Arc::new(MockHealthCheckTrait::new());
        let monitor = HealthMonitor::new(Duration::from_secs(1), mock_health);

        let process1 = create_test_process("test1", Some("cmd://echo 'healthy'".to_string()));
        let process2 = create_test_process("test2", Some("cmd://echo 'healthy'".to_string()));

        monitor.add_process(&process1).await.unwrap();
        monitor.add_process(&process2).await.unwrap();

        let all_status = monitor.get_all_health_status().await;
        assert_eq!(all_status.len(), 2);
        assert!(all_status.contains_key(&process1.id));
        assert!(all_status.contains_key(&process2.id));
    }

    #[tokio::test]
    async fn test_non_running_process_health_check() {
        let mock_health = Arc::new(MockHealthCheckTrait::new());
        let monitor = HealthMonitor::new(Duration::from_secs(1), mock_health);

        let mut process = Process::new(
            "test".to_string(),
            "echo test".to_string(),
            "/tmp".to_string(),
            "test-env".to_string(),
        );
        process.set_health_check("cmd://echo 'healthy'".to_string(), 30);
        // Keep process in Stopped state (default)

        monitor.add_process(&process).await.unwrap();

        // Should return true (healthy) for non-running processes
        let is_healthy = monitor.check_process_health(&process).await.unwrap();
        assert!(is_healthy);
    }
}
