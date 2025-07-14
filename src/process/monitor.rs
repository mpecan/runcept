#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GlobalConfig;
    use crate::process::{Process, ProcessStatus};
    use std::time::Duration;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_process_monitor_creation() {
        let global_config = GlobalConfig::default();
        let monitor = ProcessMonitor::new(global_config);

        assert!(monitor.health_checkers.is_empty());
        assert!(monitor.process_watchers.is_empty());
    }

    #[tokio::test]
    async fn test_start_monitoring_process() {
        let global_config = GlobalConfig::default();
        let mut monitor = ProcessMonitor::new(global_config);

        let temp_dir = TempDir::new().unwrap();
        let mut process = Process::new(
            "test-process".to_string(),
            "echo test".to_string(),
            temp_dir.path().to_string_lossy().to_string(),
            "env-1".to_string(),
        );
        process.transition_to(ProcessStatus::Running);
        process.set_pid(12345);

        monitor.start_monitoring(&process).await.unwrap();

        assert_eq!(monitor.process_watchers.len(), 1);
        assert!(monitor.process_watchers.contains_key(&process.id));
    }

    #[tokio::test]
    async fn test_stop_monitoring_process() {
        let global_config = GlobalConfig::default();
        let mut monitor = ProcessMonitor::new(global_config);

        let temp_dir = TempDir::new().unwrap();
        let mut process = Process::new(
            "test-process".to_string(),
            "echo test".to_string(),
            temp_dir.path().to_string_lossy().to_string(),
            "env-1".to_string(),
        );
        process.transition_to(ProcessStatus::Running);

        monitor.start_monitoring(&process).await.unwrap();
        assert_eq!(monitor.process_watchers.len(), 1);

        monitor.stop_monitoring(&process.id).await.unwrap();
        assert_eq!(monitor.process_watchers.len(), 0);
    }

    #[tokio::test]
    async fn test_health_check_http_success() {
        let health_check = HealthCheck::Http {
            url: "https://httpbin.org/status/200".to_string(),
            timeout: Duration::from_secs(5),
            expected_status: 200,
        };

        let result = health_check.execute().await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_healthy);
    }

    #[tokio::test]
    async fn test_health_check_http_failure() {
        let health_check = HealthCheck::Http {
            url: "https://httpbin.org/status/500".to_string(),
            timeout: Duration::from_secs(5),
            expected_status: 200,
        };

        let result = health_check.execute().await;
        assert!(result.is_ok());
        assert!(!result.unwrap().is_healthy);
    }

    #[tokio::test]
    async fn test_health_check_tcp_success() {
        // Test TCP health check against a known service (Google DNS)
        let health_check = HealthCheck::Tcp {
            host: "8.8.8.8".to_string(),
            port: 53,
            timeout: Duration::from_secs(5),
        };

        let result = health_check.execute().await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_healthy);
    }

    #[tokio::test]
    async fn test_health_check_tcp_failure() {
        // Test TCP health check against a non-existent service
        let health_check = HealthCheck::Tcp {
            host: "192.0.2.1".to_string(), // TEST-NET-1 address (should not respond)
            port: 12345,
            timeout: Duration::from_millis(100),
        };

        let result = health_check.execute().await;
        assert!(result.is_ok());
        assert!(!result.unwrap().is_healthy);
    }

    #[tokio::test]
    async fn test_health_check_command_success() {
        let health_check = HealthCheck::Command {
            command: "echo".to_string(),
            args: vec!["healthy".to_string()],
            working_dir: None,
            timeout: Duration::from_secs(5),
            expected_exit_code: 0,
        };

        let result = health_check.execute().await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_healthy);
    }

    #[tokio::test]
    async fn test_health_check_command_failure() {
        let health_check = HealthCheck::Command {
            command: "false".to_string(), // Command that always fails
            args: vec![],
            working_dir: None,
            timeout: Duration::from_secs(5),
            expected_exit_code: 0,
        };

        let result = health_check.execute().await;
        assert!(result.is_ok());
        assert!(!result.unwrap().is_healthy);
    }

    #[tokio::test]
    async fn test_parse_health_check_url_http() {
        let url = "http://localhost:8080/health";
        let health_check = HealthCheck::from_url(url, Duration::from_secs(30)).unwrap();

        match health_check {
            HealthCheck::Http {
                url: parsed_url,
                expected_status,
                ..
            } => {
                assert_eq!(parsed_url, url);
                assert_eq!(expected_status, 200);
            }
            _ => panic!("Expected HTTP health check"),
        }
    }

    #[tokio::test]
    async fn test_parse_health_check_url_tcp() {
        let url = "tcp://localhost:5432";
        let health_check = HealthCheck::from_url(url, Duration::from_secs(30)).unwrap();

        match health_check {
            HealthCheck::Tcp { host, port, .. } => {
                assert_eq!(host, "localhost");
                assert_eq!(port, 5432);
            }
            _ => panic!("Expected TCP health check"),
        }
    }

    #[tokio::test]
    async fn test_parse_health_check_url_command() {
        let url = "cmd://echo hello";
        let health_check = HealthCheck::from_url(url, Duration::from_secs(30)).unwrap();

        match health_check {
            HealthCheck::Command {
                command,
                args,
                expected_exit_code,
                ..
            } => {
                assert_eq!(command, "echo");
                assert_eq!(args, vec!["hello"]);
                assert_eq!(expected_exit_code, 0);
            }
            _ => panic!("Expected Command health check"),
        }
    }

    #[tokio::test]
    async fn test_health_check_manager_creation() {
        let global_config = GlobalConfig::default();
        let manager = HealthCheckManager::new(global_config);

        assert!(manager.active_checks.is_empty());
    }

    #[tokio::test]
    async fn test_health_check_manager_schedule() {
        let global_config = GlobalConfig::default();
        let mut manager = HealthCheckManager::new(global_config);

        let temp_dir = TempDir::new().unwrap();
        let mut process = Process::new(
            "test-process".to_string(),
            "echo test".to_string(),
            temp_dir.path().to_string_lossy().to_string(),
            "env-1".to_string(),
        );
        process.set_health_check("cmd://echo healthy".to_string(), 1); // 1 second interval

        manager.schedule_health_check(&process).await.unwrap();

        assert_eq!(manager.active_checks.len(), 1);
        assert!(manager.active_checks.contains_key(&process.id));
    }

    #[tokio::test]
    async fn test_health_check_manager_unschedule() {
        let global_config = GlobalConfig::default();
        let mut manager = HealthCheckManager::new(global_config);

        let temp_dir = TempDir::new().unwrap();
        let mut process = Process::new(
            "test-process".to_string(),
            "echo test".to_string(),
            temp_dir.path().to_string_lossy().to_string(),
            "env-1".to_string(),
        );
        process.set_health_check("cmd://echo healthy".to_string(), 1);

        manager.schedule_health_check(&process).await.unwrap();
        assert_eq!(manager.active_checks.len(), 1);

        manager.unschedule_health_check(&process.id).await;
        assert_eq!(manager.active_checks.len(), 0);
    }

    #[tokio::test]
    async fn test_process_status_tracker() {
        let mut tracker = ProcessStatusTracker::new();

        let process_id = "test-process-1".to_string();

        // Track status changes
        tracker.update_status(&process_id, ProcessStatus::Starting);
        tracker.update_status(&process_id, ProcessStatus::Running);
        tracker.update_status(&process_id, ProcessStatus::Stopped);

        let history = tracker.get_status_history(&process_id).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].status, ProcessStatus::Starting);
        assert_eq!(history[1].status, ProcessStatus::Running);
        assert_eq!(history[2].status, ProcessStatus::Stopped);
    }

    #[tokio::test]
    async fn test_process_status_tracker_cleanup() {
        let mut tracker = ProcessStatusTracker::new();

        let process_id = "test-process-1".to_string();

        // Add many status updates
        for _i in 0..150 {
            tracker.update_status(&process_id, ProcessStatus::Running);
        }

        let history = tracker.get_status_history(&process_id).unwrap();
        // Should be limited to MAX_HISTORY_SIZE (100)
        assert_eq!(history.len(), 100);
    }

    #[tokio::test]
    async fn test_crash_detector() {
        let mut detector = CrashDetector::new();
        let global_config = GlobalConfig::default();

        let process_id = "test-process-1".to_string();

        // Simulate a crash
        let should_restart = detector
            .handle_process_exit(&process_id, ProcessStatus::Crashed, &global_config)
            .await
            .unwrap();

        // Should restart on first crash (within max attempts)
        assert!(should_restart);

        // Simulate multiple crashes in quick succession
        for _ in 0..global_config.process.max_restart_attempts {
            detector
                .handle_process_exit(&process_id, ProcessStatus::Crashed, &global_config)
                .await
                .unwrap();
        }

        // Should not restart after exceeding max attempts
        let should_restart = detector
            .handle_process_exit(&process_id, ProcessStatus::Crashed, &global_config)
            .await
            .unwrap();

        assert!(!should_restart);
    }
}

use crate::config::GlobalConfig;
use crate::error::{Result, RunceptError};
use crate::process::{Process, ProcessStatus};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{Instant, interval, timeout};

pub struct ProcessMonitor {
    pub health_checkers: HashMap<String, HealthCheckManager>,
    pub process_watchers: HashMap<String, ProcessWatcher>,
    pub status_tracker: ProcessStatusTracker,
    pub crash_detector: CrashDetector,
    pub global_config: GlobalConfig,
}

impl ProcessMonitor {
    pub fn new(global_config: GlobalConfig) -> Self {
        Self {
            health_checkers: HashMap::new(),
            process_watchers: HashMap::new(),
            status_tracker: ProcessStatusTracker::new(),
            crash_detector: CrashDetector::new(),
            global_config,
        }
    }

    pub async fn start_monitoring(&mut self, process: &Process) -> Result<()> {
        let process_id = process.id.clone();

        // Create process watcher
        let watcher = ProcessWatcher::new(process.clone()).await?;
        self.process_watchers.insert(process_id.clone(), watcher);

        // Set up health checking if configured
        if process.health_check_url.is_some() {
            let mut health_manager = HealthCheckManager::new(self.global_config.clone());
            health_manager.schedule_health_check(process).await?;
            self.health_checkers
                .insert(process_id.clone(), health_manager);
        }

        // Track initial status
        self.status_tracker
            .update_status(&process_id, process.status.clone());

        Ok(())
    }

    pub async fn stop_monitoring(&mut self, process_id: &str) -> Result<()> {
        // Stop health checking
        if let Some(mut health_manager) = self.health_checkers.remove(process_id) {
            health_manager.unschedule_health_check(process_id).await;
        }

        // Stop process watching
        if let Some(watcher) = self.process_watchers.remove(process_id) {
            watcher.stop().await?;
        }

        Ok(())
    }

    pub async fn handle_process_status_change(
        &mut self,
        process_id: &str,
        new_status: ProcessStatus,
    ) -> Result<bool> {
        // Update status tracking
        self.status_tracker
            .update_status(process_id, new_status.clone());

        // Handle crashes and determine if restart is needed
        if matches!(new_status, ProcessStatus::Crashed | ProcessStatus::Failed) {
            self.crash_detector
                .handle_process_exit(process_id, new_status, &self.global_config)
                .await
        } else {
            Ok(false)
        }
    }

    pub fn get_process_status_history(&self, process_id: &str) -> Option<&Vec<StatusEntry>> {
        self.status_tracker.get_status_history(process_id)
    }

    pub async fn check_all_health(&mut self) -> Result<HashMap<String, HealthResult>> {
        let mut results = HashMap::new();

        for (process_id, health_manager) in &mut self.health_checkers {
            if let Some(result) = health_manager.get_last_result(process_id) {
                results.insert(process_id.clone(), result);
            }
        }

        Ok(results)
    }
}

pub struct ProcessWatcher {
    process: Process,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl ProcessWatcher {
    pub async fn new(process: Process) -> Result<Self> {
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        let process_clone = process.clone();

        // Spawn a task to watch the process
        tokio::spawn(async move {
            if let Some(pid) = process_clone.pid {
                loop {
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(1)) => {
                            // Check if process is still running by sending signal 0 (no-op signal)
                            #[cfg(unix)]
                            {
                                use nix::sys::signal::kill;
                                use nix::unistd::Pid;

                                let nix_pid = Pid::from_raw(pid as i32);
                                match kill(nix_pid, None) {
                                    Ok(_) => {
                                        // Process is still running
                                        continue;
                                    }
                                    Err(_) => {
                                        // Process is no longer running
                                        break;
                                    }
                                }
                            }

                            #[cfg(not(unix))]
                            {
                                // On non-Unix systems, we'll implement a different approach
                                // For now, just continue monitoring
                                continue;
                            }
                        }
                        _ = shutdown_rx.recv() => {
                            // Shutdown requested
                            break;
                        }
                    }
                }
            }
        });

        Ok(Self {
            process,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    pub async fn stop(mut self) -> Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }
        Ok(())
    }

    pub fn get_process(&self) -> &Process {
        &self.process
    }

    pub fn get_process_id(&self) -> &str {
        &self.process.id
    }

    pub fn get_process_name(&self) -> &str {
        &self.process.name
    }
}

#[derive(Debug, Clone)]
pub enum HealthCheck {
    Http {
        url: String,
        timeout: Duration,
        expected_status: u16,
    },
    Tcp {
        host: String,
        port: u16,
        timeout: Duration,
    },
    Command {
        command: String,
        args: Vec<String>,
        working_dir: Option<String>,
        timeout: Duration,
        expected_exit_code: i32,
    },
}

impl HealthCheck {
    pub fn from_url(url: &str, default_timeout: Duration) -> Result<Self> {
        if url.starts_with("http://") || url.starts_with("https://") {
            Ok(HealthCheck::Http {
                url: url.to_string(),
                timeout: default_timeout,
                expected_status: 200,
            })
        } else if url.starts_with("tcp://") {
            let url_without_scheme = url.strip_prefix("tcp://").unwrap();
            let parts: Vec<&str> = url_without_scheme.split(':').collect();
            if parts.len() != 2 {
                return Err(RunceptError::ConfigError(
                    "Invalid TCP URL format. Expected tcp://host:port".to_string(),
                ));
            }
            let host = parts[0].to_string();
            let port = parts[1].parse::<u16>().map_err(|_| {
                RunceptError::ConfigError("Invalid port number in TCP URL".to_string())
            })?;

            Ok(HealthCheck::Tcp {
                host,
                port,
                timeout: default_timeout,
            })
        } else if url.starts_with("cmd://") {
            let command_str = url.strip_prefix("cmd://").unwrap();
            let parts: Vec<&str> = command_str.split_whitespace().collect();
            if parts.is_empty() {
                return Err(RunceptError::ConfigError(
                    "Command cannot be empty in cmd:// URL".to_string(),
                ));
            }

            let command = parts[0].to_string();
            let args = parts[1..].iter().map(|s| s.to_string()).collect();

            Ok(HealthCheck::Command {
                command,
                args,
                working_dir: None,
                timeout: default_timeout,
                expected_exit_code: 0,
            })
        } else {
            Err(RunceptError::ConfigError(format!(
                "Unsupported health check URL scheme: {url}"
            )))
        }
    }

    pub async fn execute(&self) -> Result<HealthResult> {
        match self {
            HealthCheck::Http {
                url,
                timeout: check_timeout,
                expected_status,
            } => {
                let client = reqwest::Client::new();
                let start_time = Instant::now();

                match timeout(*check_timeout, client.get(url).send()).await {
                    Ok(Ok(response)) => {
                        let is_healthy = response.status().as_u16() == *expected_status;
                        Ok(HealthResult {
                            is_healthy,
                            response_time: start_time.elapsed(),
                            error_message: if is_healthy {
                                None
                            } else {
                                Some(format!(
                                    "Expected status {}, got {}",
                                    expected_status,
                                    response.status()
                                ))
                            },
                            checked_at: Utc::now(),
                        })
                    }
                    Ok(Err(e)) => Ok(HealthResult {
                        is_healthy: false,
                        response_time: start_time.elapsed(),
                        error_message: Some(e.to_string()),
                        checked_at: Utc::now(),
                    }),
                    Err(_) => Ok(HealthResult {
                        is_healthy: false,
                        response_time: start_time.elapsed(),
                        error_message: Some("Request timeout".to_string()),
                        checked_at: Utc::now(),
                    }),
                }
            }
            HealthCheck::Tcp {
                host,
                port,
                timeout: check_timeout,
            } => {
                let start_time = Instant::now();
                let addr = format!("{host}:{port}");

                match timeout(*check_timeout, tokio::net::TcpStream::connect(&addr)).await {
                    Ok(Ok(_)) => Ok(HealthResult {
                        is_healthy: true,
                        response_time: start_time.elapsed(),
                        error_message: None,
                        checked_at: Utc::now(),
                    }),
                    Ok(Err(e)) => Ok(HealthResult {
                        is_healthy: false,
                        response_time: start_time.elapsed(),
                        error_message: Some(e.to_string()),
                        checked_at: Utc::now(),
                    }),
                    Err(_) => Ok(HealthResult {
                        is_healthy: false,
                        response_time: start_time.elapsed(),
                        error_message: Some("Connection timeout".to_string()),
                        checked_at: Utc::now(),
                    }),
                }
            }
            HealthCheck::Command {
                command,
                args,
                working_dir,
                timeout: check_timeout,
                expected_exit_code,
            } => {
                let start_time = Instant::now();
                let mut cmd = tokio::process::Command::new(command);
                cmd.args(args);

                if let Some(wd) = working_dir {
                    cmd.current_dir(wd);
                }

                match timeout(*check_timeout, cmd.output()).await {
                    Ok(Ok(output)) => {
                        let exit_code = output.status.code().unwrap_or(-1);
                        let is_healthy = exit_code == *expected_exit_code;
                        Ok(HealthResult {
                            is_healthy,
                            response_time: start_time.elapsed(),
                            error_message: if is_healthy {
                                None
                            } else {
                                Some(format!(
                                    "Expected exit code {expected_exit_code}, got {exit_code}"
                                ))
                            },
                            checked_at: Utc::now(),
                        })
                    }
                    Ok(Err(e)) => Ok(HealthResult {
                        is_healthy: false,
                        response_time: start_time.elapsed(),
                        error_message: Some(e.to_string()),
                        checked_at: Utc::now(),
                    }),
                    Err(_) => Ok(HealthResult {
                        is_healthy: false,
                        response_time: start_time.elapsed(),
                        error_message: Some("Command timeout".to_string()),
                        checked_at: Utc::now(),
                    }),
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct HealthResult {
    pub is_healthy: bool,
    pub response_time: Duration,
    pub error_message: Option<String>,
    pub checked_at: DateTime<Utc>,
}

pub struct HealthCheckManager {
    pub active_checks: HashMap<String, mpsc::Sender<()>>,
    pub last_results: HashMap<String, HealthResult>,
    pub global_config: GlobalConfig,
}

impl HealthCheckManager {
    pub fn new(global_config: GlobalConfig) -> Self {
        Self {
            active_checks: HashMap::new(),
            last_results: HashMap::new(),
            global_config,
        }
    }

    pub async fn schedule_health_check(&mut self, process: &Process) -> Result<()> {
        if let Some(health_url) = &process.health_check_url {
            let interval_secs = process
                .health_check_interval
                .unwrap_or(self.global_config.process.health_check_interval);

            let health_check = HealthCheck::from_url(
                health_url,
                Duration::from_secs(self.global_config.process.health_check_interval as u64),
            )?;

            let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
            let process_id = process.id.clone();
            let mut check_interval = interval(Duration::from_secs(interval_secs as u64));

            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = check_interval.tick() => {
                            if let Ok(_result) = health_check.execute().await {
                                // TODO: Handle health check result
                                // For now, we just execute the check
                            }
                        }
                        _ = shutdown_rx.recv() => {
                            break;
                        }
                    }
                }
            });

            self.active_checks.insert(process_id, shutdown_tx);
        }

        Ok(())
    }

    pub async fn unschedule_health_check(&mut self, process_id: &str) {
        if let Some(shutdown_tx) = self.active_checks.remove(process_id) {
            let _ = shutdown_tx.send(()).await;
        }
        self.last_results.remove(process_id);
    }

    pub fn get_last_result(&self, process_id: &str) -> Option<HealthResult> {
        self.last_results.get(process_id).cloned()
    }
}

#[derive(Debug, Clone)]
pub struct StatusEntry {
    pub status: ProcessStatus,
    pub timestamp: DateTime<Utc>,
}

#[derive(Default)]
pub struct ProcessStatusTracker {
    pub status_history: HashMap<String, Vec<StatusEntry>>,
}

impl ProcessStatusTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_status(&mut self, process_id: &str, status: ProcessStatus) {
        let entry = StatusEntry {
            status,
            timestamp: Utc::now(),
        };

        let history = self
            .status_history
            .entry(process_id.to_string())
            .or_default();

        history.push(entry);

        // Limit history size
        const MAX_HISTORY_SIZE: usize = 100;
        if history.len() > MAX_HISTORY_SIZE {
            history.remove(0);
        }
    }

    pub fn get_status_history(&self, process_id: &str) -> Option<&Vec<StatusEntry>> {
        self.status_history.get(process_id)
    }

    pub fn get_current_status(&self, process_id: &str) -> Option<ProcessStatus> {
        self.status_history
            .get(process_id)?
            .last()
            .map(|entry| entry.status.clone())
    }
}

#[derive(Default)]
pub struct CrashDetector {
    pub restart_attempts: HashMap<String, Vec<DateTime<Utc>>>,
}

impl CrashDetector {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn handle_process_exit(
        &mut self,
        process_id: &str,
        exit_status: ProcessStatus,
        global_config: &GlobalConfig,
    ) -> Result<bool> {
        // Only handle crashes and failures
        if !matches!(exit_status, ProcessStatus::Crashed | ProcessStatus::Failed) {
            return Ok(false);
        }

        let now = Utc::now();
        let max_attempts = global_config.process.max_restart_attempts;
        let restart_delay = Duration::from_secs(global_config.process.restart_delay as u64);

        // Get or create restart attempt history
        let attempts = self
            .restart_attempts
            .entry(process_id.to_string())
            .or_default();

        // Clean up old attempts (older than 1 hour)
        let cutoff_time = now - chrono::Duration::hours(1);
        attempts.retain(|&attempt_time| attempt_time > cutoff_time);

        // Check if we've exceeded max attempts
        if attempts.len() >= max_attempts as usize {
            return Ok(false); // Don't restart
        }

        // Record this restart attempt
        attempts.push(now);

        // Wait for restart delay
        tokio::time::sleep(restart_delay).await;

        Ok(true) // Should restart
    }

    pub fn get_restart_attempts(&self, process_id: &str) -> Vec<DateTime<Utc>> {
        self.restart_attempts
            .get(process_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn reset_restart_attempts(&mut self, process_id: &str) {
        self.restart_attempts.remove(process_id);
    }
}
