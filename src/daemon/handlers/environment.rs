use crate::cli::commands::{DaemonResponse, EnvironmentStatusResponse};
use crate::config::{EnvironmentManager, GlobalConfig, ProjectConfig};
use crate::daemon::handlers::ProcessHandles;
use crate::error::{Result, RunceptError};
use crate::scheduler::InactivityScheduler;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Handles for environment-related operations
#[derive(Clone)]
pub struct EnvironmentHandles {
    process_handles: ProcessHandles,
    environment_manager: Arc<RwLock<EnvironmentManager>>,
    inactivity_scheduler: Arc<RwLock<Option<InactivityScheduler>>>,
    current_environment_id: Arc<RwLock<Option<String>>>,
    global_config: GlobalConfig,
}

impl EnvironmentHandles {
    pub fn new(
        process_handles: ProcessHandles,
        environment_manager: Arc<RwLock<EnvironmentManager>>,
        inactivity_scheduler: Arc<RwLock<Option<InactivityScheduler>>>,
        current_environment_id: Arc<RwLock<Option<String>>>,
        global_config: GlobalConfig,
    ) -> Self {
        Self {
            process_handles,
            environment_manager,
            inactivity_scheduler,
            current_environment_id,
            global_config,
        }
    }

    /// Activate an environment
    pub async fn activate_environment(&self, path: Option<String>) -> Result<DaemonResponse> {
        let project_path = if let Some(path) = path {
            PathBuf::from(path)
        } else {
            std::env::current_dir().map_err(|e| {
                RunceptError::EnvironmentError(format!("Failed to get current directory: {e}"))
            })?
        };

        // Delegate to EnvironmentManager - it handles registration, activation, and config watching
        let mut env_manager = self.environment_manager.write().await;
        match env_manager
            .activate_environment_at_path(&project_path)
            .await
        {
            Ok(env_id) => {
                // Store current environment ID
                *self.current_environment_id.write().await = Some(env_id);

                // Start inactivity scheduler for the environment
                let mut scheduler_guard = self.inactivity_scheduler.write().await;
                if scheduler_guard.is_none() {
                    let scheduler = InactivityScheduler::new(self.global_config.clone());
                    *scheduler_guard = Some(scheduler);
                }

                Ok(DaemonResponse::Success {
                    message: format!("Environment activated: {}", project_path.display()),
                })
            }
            Err(e) => Ok(DaemonResponse::Error {
                error: format!("Failed to activate environment: {e}"),
            }),
        }
    }

    /// Deactivate current environment
    pub async fn deactivate_environment(&self) -> Result<DaemonResponse> {
        let current_env_id = self.current_environment_id.read().await.clone();

        if let Some(env_id) = current_env_id {
            // Delegate to EnvironmentManager - it handles deactivation and config unwatching
            let mut env_manager = self.environment_manager.write().await;
            match env_manager
                .deactivate_environment_and_unwatch(&env_id)
                .await
            {
                Ok(_) => {
                    // Clear current environment
                    *self.current_environment_id.write().await = None;

                    // Stop inactivity scheduler
                    let mut scheduler_guard = self.inactivity_scheduler.write().await;
                    if let Some(mut scheduler) = scheduler_guard.take() {
                        let _ = scheduler.stop().await;
                    }

                    Ok(DaemonResponse::Success {
                        message: "Environment deactivated".to_string(),
                    })
                }
                Err(e) => Ok(DaemonResponse::Error {
                    error: format!("Failed to deactivate environment: {e}"),
                }),
            }
        } else {
            Ok(DaemonResponse::Error {
                error: "No active environment".to_string(),
            })
        }
    }

    /// Get environment status
    pub async fn get_environment_status(&self) -> Result<DaemonResponse> {
        let env_manager = self.environment_manager.read().await;
        let current_env_id = self.current_environment_id.read().await.clone();

        let (active_environment, project_path) = if let Some(env_id) = &current_env_id {
            if let Some(env) = env_manager.get_environment(env_id) {
                (
                    Some(env.name.clone()),
                    Some(env.project_path.to_string_lossy().to_string()),
                )
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        // Delegate to ProcessHandles for process information
        let (processes, total_configured, running_processes) = if let Some(env_id) = &current_env_id
        {
            self.process_handles
                .get_environment_process_summary(env_id)
                .await
                .unwrap_or_else(|_| (Vec::new(), 0, 0))
        } else {
            (Vec::new(), 0, 0)
        };

        let status = EnvironmentStatusResponse {
            active_environment,
            project_path,
            processes: processes.clone(),
            total_processes: total_configured,
            running_processes,
        };

        Ok(DaemonResponse::EnvironmentStatus(status))
    }

    /// Initialize a new project with default configuration
    pub async fn init_project(&self, path: Option<String>, force: bool) -> Result<DaemonResponse> {
        use std::path::Path;

        let project_path = if let Some(p) = path {
            Path::new(&p).to_path_buf()
        } else {
            std::env::current_dir().map_err(|e| {
                RunceptError::IoError(std::io::Error::other(format!(
                    "Failed to get current directory: {e}"
                )))
            })?
        };

        let config_path = project_path.join(".runcept.toml");

        // Check if config already exists
        if config_path.exists() && !force {
            return Ok(DaemonResponse::Error {
                error: format!(
                    "Configuration file already exists: {}. Use --force to overwrite.",
                    config_path.display()
                ),
            });
        }

        // Get project name from directory name
        let project_name = project_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("my-project")
            .to_string();

        // Create default configuration
        let default_config = ProjectConfig::create_default(&project_name);

        // Save the configuration
        match default_config.save_to_path(&config_path).await {
            Ok(()) => Ok(DaemonResponse::Success {
                message: format!(
                    "Initialized new runcept project '{}' in {}",
                    project_name,
                    project_path.display()
                ),
            }),
            Err(e) => Ok(DaemonResponse::Error {
                error: format!("Failed to create configuration file: {e}"),
            }),
        }
    }
}
