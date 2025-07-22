use crate::cli::commands::{DaemonResponse, EnvironmentStatusResponse};
use crate::config::{EnvironmentManager, EnvironmentManagerTrait, GlobalConfig, ProjectConfig};
use crate::error::{Result, RunceptError};
use crate::process::ProcessOrchestrationTrait;
use crate::scheduler::InactivityScheduler;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Handles for environment-related operations
pub struct EnvironmentHandles<P: ProcessOrchestrationTrait, E: EnvironmentManagerTrait> {
    process_handles: crate::daemon::handlers::process::ProcessHandles<P>,
    environment_manager: Arc<RwLock<E>>,
    inactivity_scheduler: Arc<RwLock<Option<InactivityScheduler>>>,
    current_environment_id: Arc<RwLock<Option<String>>>,
    global_config: GlobalConfig,
}

impl<P: ProcessOrchestrationTrait, E: EnvironmentManagerTrait> Clone for EnvironmentHandles<P, E> {
    fn clone(&self) -> Self {
        Self {
            process_handles: self.process_handles.clone(),
            environment_manager: Arc::clone(&self.environment_manager),
            inactivity_scheduler: Arc::clone(&self.inactivity_scheduler),
            current_environment_id: Arc::clone(&self.current_environment_id),
            global_config: self.global_config.clone(),
        }
    }
}

/// Type alias for the default EnvironmentHandles implementation
pub type DefaultEnvironmentHandles =
    EnvironmentHandles<crate::process::DefaultProcessOrchestrationService, EnvironmentManager>;

impl<P: ProcessOrchestrationTrait, E: EnvironmentManagerTrait> EnvironmentHandles<P, E> {
    pub fn new(
        process_handles: crate::daemon::handlers::process::ProcessHandles<P>,
        environment_manager: Arc<RwLock<E>>,
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
                    Some(env.name),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MockEnvironmentManagerTrait;
    use crate::process::MockProcessOrchestrationTrait;
    use mockall::predicate::*;

    #[tokio::test]
    async fn test_activate_environment_success() {
        let mut mock_process_service = MockProcessOrchestrationTrait::new();
        mock_process_service
            .expect_get_environment_process_summary()
            .returning(|_| Ok((vec![], 0, 0)));

        let process_handles = crate::daemon::handlers::process::ProcessHandles::new(
            Arc::new(RwLock::new(mock_process_service)),
            Arc::new(RwLock::new(Some("test-env".to_string()))),
        );

        let mut mock_env_manager = MockEnvironmentManagerTrait::new();
        mock_env_manager
            .expect_activate_environment_at_path()
            .with(function(|path: &std::path::Path| {
                path.to_string_lossy().contains("test")
            }))
            .times(1)
            .returning(|_| Ok("test-env-123".to_string()));

        let handles = EnvironmentHandles::new(
            process_handles,
            Arc::new(RwLock::new(mock_env_manager)),
            Arc::new(RwLock::new(None)),
            Arc::new(RwLock::new(None)),
            GlobalConfig::default(),
        );

        let result = handles
            .activate_environment(Some("/tmp/test".to_string()))
            .await;

        assert!(result.is_ok());
        match result.unwrap() {
            DaemonResponse::Success { message } => {
                assert!(message.contains("Environment activated"));
            }
            _ => panic!("Expected Success response"),
        }
    }

    #[tokio::test]
    async fn test_activate_environment_failure() {
        let mut mock_process_service = MockProcessOrchestrationTrait::new();
        mock_process_service
            .expect_get_environment_process_summary()
            .returning(|_| Ok((vec![], 0, 0)));

        let process_handles = crate::daemon::handlers::process::ProcessHandles::new(
            Arc::new(RwLock::new(mock_process_service)),
            Arc::new(RwLock::new(Some("test-env".to_string()))),
        );

        let mut mock_env_manager = MockEnvironmentManagerTrait::new();
        mock_env_manager
            .expect_activate_environment_at_path()
            .with(function(|path: &std::path::Path| {
                path.to_string_lossy().contains("test")
            }))
            .times(1)
            .returning(|_| Err(RunceptError::EnvironmentError("Mock failure".to_string())));

        let handles = EnvironmentHandles::new(
            process_handles,
            Arc::new(RwLock::new(mock_env_manager)),
            Arc::new(RwLock::new(None)),
            Arc::new(RwLock::new(None)),
            GlobalConfig::default(),
        );

        let result = handles
            .activate_environment(Some("/tmp/test".to_string()))
            .await;

        assert!(result.is_ok());
        match result.unwrap() {
            DaemonResponse::Error { error } => {
                assert!(error.contains("Failed to activate environment"));
            }
            _ => panic!("Expected Error response"),
        }
    }

    #[tokio::test]
    async fn test_deactivate_environment_success() {
        let mock_process_service = MockProcessOrchestrationTrait::new();
        let process_handles = crate::daemon::handlers::process::ProcessHandles::new(
            Arc::new(RwLock::new(mock_process_service)),
            Arc::new(RwLock::new(Some("test-env".to_string()))),
        );

        let mut mock_env_manager = MockEnvironmentManagerTrait::new();
        mock_env_manager
            .expect_deactivate_environment_and_unwatch()
            .with(eq("test-env"))
            .times(1)
            .returning(|_| Ok(()));

        let handles = EnvironmentHandles::new(
            process_handles,
            Arc::new(RwLock::new(mock_env_manager)),
            Arc::new(RwLock::new(None)),
            Arc::new(RwLock::new(Some("test-env".to_string()))),
            GlobalConfig::default(),
        );

        let result = handles.deactivate_environment().await;

        assert!(result.is_ok());
        match result.unwrap() {
            DaemonResponse::Success { message } => {
                assert_eq!(message, "Environment deactivated");
            }
            _ => panic!("Expected Success response"),
        }
    }

    #[tokio::test]
    async fn test_deactivate_environment_no_active() {
        let mock_process_service = MockProcessOrchestrationTrait::new();
        let process_handles = crate::daemon::handlers::process::ProcessHandles::new(
            Arc::new(RwLock::new(mock_process_service)),
            Arc::new(RwLock::new(Some("test-env".to_string()))),
        );

        let mock_env_manager = MockEnvironmentManagerTrait::new();
        let handles = EnvironmentHandles::new(
            process_handles,
            Arc::new(RwLock::new(mock_env_manager)),
            Arc::new(RwLock::new(None)),
            Arc::new(RwLock::new(None)), // No active environment
            GlobalConfig::default(),
        );

        let result = handles.deactivate_environment().await;

        assert!(result.is_ok());
        match result.unwrap() {
            DaemonResponse::Error { error } => {
                assert_eq!(error, "No active environment");
            }
            _ => panic!("Expected Error response"),
        }
    }

    #[tokio::test]
    async fn test_init_project_success() {
        use tempfile::TempDir;

        let mock_process_service = MockProcessOrchestrationTrait::new();
        let process_handles = crate::daemon::handlers::process::ProcessHandles::new(
            Arc::new(RwLock::new(mock_process_service)),
            Arc::new(RwLock::new(Some("test-env".to_string()))),
        );

        let mock_env_manager = MockEnvironmentManagerTrait::new();
        let handles = EnvironmentHandles::new(
            process_handles,
            Arc::new(RwLock::new(mock_env_manager)),
            Arc::new(RwLock::new(None)),
            Arc::new(RwLock::new(None)),
            GlobalConfig::default(),
        );

        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_string_lossy().to_string();

        let result = handles
            .init_project(Some(project_path.clone()), false)
            .await;

        assert!(result.is_ok());
        match result.unwrap() {
            DaemonResponse::Success { message } => {
                assert!(message.contains("Initialized new runcept project"));
            }
            _ => panic!("Expected Success response"),
        }

        // Verify config file was created
        let config_path = temp_dir.path().join(".runcept.toml");
        assert!(config_path.exists());
    }

    #[tokio::test]
    async fn test_init_project_already_exists() {
        use tempfile::TempDir;

        let mock_process_service = MockProcessOrchestrationTrait::new();
        let process_handles = crate::daemon::handlers::process::ProcessHandles::new(
            Arc::new(RwLock::new(mock_process_service)),
            Arc::new(RwLock::new(Some("test-env".to_string()))),
        );

        let mock_env_manager = MockEnvironmentManagerTrait::new();
        let handles = EnvironmentHandles::new(
            process_handles,
            Arc::new(RwLock::new(mock_env_manager)),
            Arc::new(RwLock::new(None)),
            Arc::new(RwLock::new(None)),
            GlobalConfig::default(),
        );

        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_string_lossy().to_string();

        // Create config file first
        let config_path = temp_dir.path().join(".runcept.toml");
        tokio::fs::write(&config_path, "# existing config")
            .await
            .unwrap();

        let result = handles.init_project(Some(project_path), false).await;

        assert!(result.is_ok());
        match result.unwrap() {
            DaemonResponse::Error { error } => {
                assert!(error.contains("Configuration file already exists"));
            }
            _ => panic!("Expected Error response"),
        }
    }
}
