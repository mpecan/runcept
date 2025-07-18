use crate::cli::commands::{DaemonResponse, DaemonStatusResponse};
use crate::config::EnvironmentManager;
use crate::error::Result;
use crate::process::ProcessManager;
use crate::scheduler::InactivityScheduler;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;

/// Handles for daemon-related operations
#[derive(Clone)]
pub struct DaemonHandles {
    process_manager: Arc<RwLock<ProcessManager>>,
    environment_manager: Arc<RwLock<EnvironmentManager>>,
    inactivity_scheduler: Arc<RwLock<Option<InactivityScheduler>>>,
    current_environment_id: Arc<RwLock<Option<String>>>,
    socket_path: PathBuf,
    start_time: SystemTime,
}

impl DaemonHandles {
    pub fn new(
        process_manager: Arc<RwLock<ProcessManager>>,
        environment_manager: Arc<RwLock<EnvironmentManager>>,
        inactivity_scheduler: Arc<RwLock<Option<InactivityScheduler>>>,
        current_environment_id: Arc<RwLock<Option<String>>>,
        socket_path: PathBuf,
        start_time: SystemTime,
    ) -> Self {
        Self {
            process_manager,
            environment_manager,
            inactivity_scheduler,
            current_environment_id,
            socket_path,
            start_time,
        }
    }

    /// Get daemon status including uptime, process counts, and environment counts
    pub async fn get_daemon_status(&self) -> Result<DaemonResponse> {
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

        // Count active environments using the environment manager
        let active_environments = env_manager.get_active_environments();
        let environment_count = active_environments.len();

        // Count total processes across all active environments
        let mut total_processes = 0;
        for (env_id, _) in &active_environments {
            if let Ok(processes) = process_manager.list_processes_for_environment(env_id).await {
                total_processes += processes.len();
            }
        }

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
            message: format!("Activity recorded for environment: {}", environment_id),
        })
    }
}
