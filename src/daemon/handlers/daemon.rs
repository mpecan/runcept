use crate::cli::commands::{DaemonResponse, DaemonStatusResponse};
use crate::config::EnvironmentManager;
use crate::error::{Result, RunceptError};
use crate::process::DefaultProcessOrchestrationService;
use crate::scheduler::InactivityScheduler;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{RwLock, mpsc};

/// Handles for daemon-related operations
#[derive(Clone)]
pub struct DaemonHandles {
    orchestration_service: Arc<RwLock<DefaultProcessOrchestrationService>>,
    environment_manager: Arc<RwLock<EnvironmentManager>>,
    inactivity_scheduler: Arc<RwLock<Option<InactivityScheduler>>>,
    #[allow(dead_code)]
    current_environment_id: Arc<RwLock<Option<String>>>,
    socket_path: PathBuf,
    start_time: SystemTime,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl DaemonHandles {
    pub fn new(
        orchestration_service: Arc<RwLock<DefaultProcessOrchestrationService>>,
        environment_manager: Arc<RwLock<EnvironmentManager>>,
        inactivity_scheduler: Arc<RwLock<Option<InactivityScheduler>>>,
        current_environment_id: Arc<RwLock<Option<String>>>,
        socket_path: PathBuf,
        start_time: SystemTime,
        shutdown_tx: Option<mpsc::Sender<()>>,
    ) -> Self {
        Self {
            orchestration_service,
            environment_manager,
            inactivity_scheduler,
            current_environment_id,
            socket_path,
            start_time,
            shutdown_tx,
        }
    }

    /// Get daemon status including uptime, process counts, and environment counts
    pub async fn get_daemon_status(&self) -> Result<DaemonResponse> {
        let orchestration_service = self.orchestration_service.read().await;
        let env_manager = self.environment_manager.read().await;

        let uptime = self.start_time.elapsed().unwrap_or(Duration::from_secs(0));
        let uptime_str = Self::format_uptime(uptime);

        // Count active environments using the environment manager
        let active_environments = env_manager.get_active_environments();
        let environment_count = active_environments.len();

        // Count total processes across all active environments
        let total_processes = self.count_total_processes(&orchestration_service, &active_environments).await;

        let status = DaemonStatusResponse {
            running: true,
            uptime: uptime_str,
            version: env!("CARGO_PKG_VERSION").to_string(),
            socket_path: self.socket_path.to_string_lossy().to_string(),
            total_processes,
            active_environments: environment_count,
            memory_usage: None, // TODO: Implement memory usage tracking
        };

        Ok(DaemonResponse::DaemonStatus(status))
    }

    /// Format uptime duration into a human-readable string
    pub fn format_uptime(uptime: Duration) -> String {
        format!(
            "{}d {}h {}m {}s",
            uptime.as_secs() / 86400,
            (uptime.as_secs() % 86400) / 3600,
            (uptime.as_secs() % 3600) / 60,
            uptime.as_secs() % 60
        )
    }

    /// Count total processes across all active environments
    async fn count_total_processes(
        &self,
        orchestration_service: &DefaultProcessOrchestrationService,
        active_environments: &Vec<(&String, &crate::config::Environment)>,
    ) -> usize {
        let mut total_processes = 0;
        for (env_id, _) in active_environments {
            if let Ok(processes) = orchestration_service
                .list_processes_for_environment(env_id)
                .await
            {
                total_processes += processes.len();
            }
        }
        total_processes
    }

    /// Record environment activity for inactivity tracking
    pub async fn record_environment_activity(
        &self,
        environment_id: String,
    ) -> Result<DaemonResponse> {
        let inactivity_scheduler = self.inactivity_scheduler.read().await;

        if let Some(scheduler) = inactivity_scheduler.as_ref() {
            scheduler.record_process_activity(&environment_id).await;
        }

        Ok(DaemonResponse::Success {
            message: format!("Activity recorded for environment: {environment_id}"),
        })
    }

    /// Request daemon shutdown
    pub async fn request_shutdown(&self) -> Result<DaemonResponse> {
        if let Some(tx) = &self.shutdown_tx {
            tx.send(()).await.map_err(|e| {
                RunceptError::EnvironmentError(format!("Failed to send shutdown signal: {e}"))
            })?;
        }

        Ok(DaemonResponse::Success {
            message: "Shutdown initiated".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_format_uptime_seconds() {
        let uptime = Duration::from_secs(45);
        let formatted = DaemonHandles::format_uptime(uptime);
        assert_eq!(formatted, "0d 0h 0m 45s");
    }

    #[test]
    fn test_format_uptime_minutes() {
        let uptime = Duration::from_secs(125); // 2m 5s
        let formatted = DaemonHandles::format_uptime(uptime);
        assert_eq!(formatted, "0d 0h 2m 5s");
    }

    #[test]
    fn test_format_uptime_hours() {
        let uptime = Duration::from_secs(3665); // 1h 1m 5s
        let formatted = DaemonHandles::format_uptime(uptime);
        assert_eq!(formatted, "0d 1h 1m 5s");
    }

    #[test]
    fn test_format_uptime_days() {
        let uptime = Duration::from_secs(90061); // 1d 1h 1m 1s
        let formatted = DaemonHandles::format_uptime(uptime);
        assert_eq!(formatted, "1d 1h 1m 1s");
    }

    #[test]
    fn test_format_uptime_complex() {
        let uptime = Duration::from_secs(266645); // 3d 2h 4m 5s
        let formatted = DaemonHandles::format_uptime(uptime);
        assert_eq!(formatted, "3d 2h 4m 5s");
    }

    #[test]
    fn test_format_uptime_zero() {
        let uptime = Duration::from_secs(0);
        let formatted = DaemonHandles::format_uptime(uptime);
        assert_eq!(formatted, "0d 0h 0m 0s");
    }

    #[test]
    fn test_format_uptime_edge_cases() {
        // Exactly 1 minute
        let uptime = Duration::from_secs(60);
        let formatted = DaemonHandles::format_uptime(uptime);
        assert_eq!(formatted, "0d 0h 1m 0s");

        // Exactly 1 hour
        let uptime = Duration::from_secs(3600);
        let formatted = DaemonHandles::format_uptime(uptime);
        assert_eq!(formatted, "0d 1h 0m 0s");

        // Exactly 1 day
        let uptime = Duration::from_secs(86400);
        let formatted = DaemonHandles::format_uptime(uptime);
        assert_eq!(formatted, "1d 0h 0m 0s");
    }

    // Note: Integration tests with full daemon setup are complex due to dependencies.
    // These tests focus on the pure functions and simple logic that can be tested in isolation.
}
