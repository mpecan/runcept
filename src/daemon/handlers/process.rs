use crate::cli::commands::{DaemonResponse, LogLine, LogsResponse, ProcessInfo};
use crate::config::ProcessDefinition;
use crate::error::{Result, RunceptError};
use crate::logging::{log_debug, log_error, log_info};
use crate::process::DefaultProcessOrchestrationService;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Handles for process-related operations - thin delegation layer
#[derive(Clone)]
pub struct ProcessHandles {
    orchestration_service: Arc<RwLock<DefaultProcessOrchestrationService>>,
    current_environment_id: Arc<RwLock<Option<String>>>,
}

impl ProcessHandles {
    pub fn new(
        orchestration_service: Arc<RwLock<DefaultProcessOrchestrationService>>,
        current_environment_id: Arc<RwLock<Option<String>>>,
    ) -> Self {
        Self {
            orchestration_service,
            current_environment_id,
        }
    }

    /// Start a process - delegates to ProcessOrchestrationService
    pub async fn start_process(
        &self,
        name: String,
        environment: Option<String>,
    ) -> Result<DaemonResponse> {
        let environment_id = self.resolve_environment_id(environment).await?;
        let mut orchestration_service = self.orchestration_service.write().await;

        match orchestration_service
            .start_process_by_name_in_environment(&name, &environment_id)
            .await
        {
            Ok(_) => Ok(DaemonResponse::Success {
                message: format!("Process '{name}' started"),
            }),
            Err(e) => Ok(DaemonResponse::Error {
                error: format!("Failed to start process '{name}': {e}"),
            }),
        }
    }

    /// Stop a process - delegates to ProcessOrchestrationService
    pub async fn stop_process(
        &self,
        name: String,
        environment: Option<String>,
    ) -> Result<DaemonResponse> {
        let environment_id = self.resolve_environment_id(environment).await?;
        let mut orchestration_service = self.orchestration_service.write().await;

        match orchestration_service
            .stop_process_by_name_in_environment(&name, &environment_id)
            .await
        {
            Ok(_) => Ok(DaemonResponse::Success {
                message: format!("Process '{name}' stopped"),
            }),
            Err(e) => Ok(DaemonResponse::Error {
                error: format!("Failed to stop process '{name}': {e}"),
            }),
        }
    }

    /// Restart a process - delegates to ProcessOrchestrationService
    pub async fn restart_process(
        &self,
        name: String,
        environment: Option<String>,
    ) -> Result<DaemonResponse> {
        let environment_id = self.resolve_environment_id(environment).await?;
        let mut orchestration_service = self.orchestration_service.write().await;

        match orchestration_service
            .restart_process_by_name_in_environment(&name, &environment_id)
            .await
        {
            Ok(_) => Ok(DaemonResponse::Success {
                message: format!("Process '{name}' restarted"),
            }),
            Err(e) => Ok(DaemonResponse::Error {
                error: format!("Failed to restart process '{name}': {e}"),
            }),
        }
    }

    /// List processes in current environment - delegates to ProcessManager
    pub async fn list_processes(&self, environment: Option<String>) -> Result<DaemonResponse> {
        let environment_id = self.resolve_environment_id(environment).await?;
        let orchestration_service = self.orchestration_service.read().await;

        match orchestration_service
            .list_processes_for_environment(&environment_id)
            .await
        {
            Ok(processes) => Ok(DaemonResponse::ProcessList(processes)),
            Err(e) => Ok(DaemonResponse::Error {
                error: format!("Failed to list processes: {e}"),
            }),
        }
    }

    /// List all processes across environments
    pub async fn list_all_processes(&self) -> Result<DaemonResponse> {
        // For now, same as list_processes with current environment
        self.list_processes(None).await
    }

    /// Kill all processes
    pub async fn kill_all_processes(&self) -> Result<DaemonResponse> {
        let environment_id = self.resolve_environment_id(None).await?;
        let mut orchestration_service = self.orchestration_service.write().await;

        match orchestration_service
            .stop_all_processes_in_environment(&environment_id)
            .await
        {
            Ok(_) => Ok(DaemonResponse::Success {
                message: "All processes stopped".to_string(),
            }),
            Err(e) => Ok(DaemonResponse::Error {
                error: format!("Failed to stop processes: {e}"),
            }),
        }
    }

    /// Get process logs - delegates to ProcessManager
    pub async fn get_process_logs(
        &self,
        name: String,
        lines: Option<usize>,
        environment: Option<String>,
    ) -> Result<DaemonResponse> {
        log_debug(
            "daemon",
            &format!("Getting logs for process '{name}'"),
            None,
        );
        let environment_id = self.resolve_environment_id(environment).await?;
        let orchestration_service = self.orchestration_service.read().await;

        // Delegate to ProcessManager
        match orchestration_service
            .get_process_logs_by_name_in_environment(&name, &environment_id, lines)
            .await
        {
            Ok(log_entries) => {
                // Convert LogEntry to LogLine format expected by the response
                let log_lines: Vec<LogLine> = log_entries
                    .into_iter()
                    .map(|entry| LogLine {
                        timestamp: entry.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
                        level: entry.level.to_string(),
                        message: entry.message,
                    })
                    .collect();

                let total_lines = log_lines.len();

                Ok(DaemonResponse::ProcessLogs(LogsResponse {
                    process_name: name,
                    lines: log_lines,
                    total_lines,
                }))
            }
            Err(e) => Ok(DaemonResponse::Error {
                error: format!("Failed to get logs for process '{name}': {e}"),
            }),
        }
    }

    /// Add a new process to the project configuration - delegates to ProcessManager
    pub async fn add_process(
        &self,
        process: ProcessDefinition,
        environment: Option<String>,
    ) -> Result<DaemonResponse> {
        log_info(
            "daemon",
            &format!(
                "Adding process '{}' to environment {:?}",
                process.name, environment
            ),
            None,
        );

        let environment_id = self.resolve_environment_id(environment).await?;
        let mut orchestration_service = self.orchestration_service.write().await;

        match orchestration_service
            .add_process_to_environment(process.clone(), &environment_id)
            .await
        {
            Ok(_) => {
                let success_msg = format!("Process '{}' added successfully", process.name);
                log_info("daemon", &success_msg, None);
                Ok(DaemonResponse::Success {
                    message: success_msg,
                })
            }
            Err(e) => {
                let error_msg = format!("Failed to add process '{}': {e}", process.name);
                log_error("daemon", &error_msg, None);
                Ok(DaemonResponse::Error { error: error_msg })
            }
        }
    }

    /// Remove a process from the project configuration - delegates to ProcessManager
    pub async fn remove_process(
        &self,
        name: String,
        environment: Option<String>,
    ) -> Result<DaemonResponse> {
        let environment_id = self.resolve_environment_id(environment).await?;
        let mut orchestration_service = self.orchestration_service.write().await;

        match orchestration_service
            .remove_process_from_environment(&name, &environment_id)
            .await
        {
            Ok(_) => Ok(DaemonResponse::Success {
                message: format!("Process '{name}' removed successfully"),
            }),
            Err(e) => Ok(DaemonResponse::Error {
                error: format!("Failed to remove process '{name}': {e}"),
            }),
        }
    }

    /// Update an existing process configuration - delegates to ProcessManager
    pub async fn update_process(
        &self,
        name: String,
        process: ProcessDefinition,
        environment: Option<String>,
    ) -> Result<DaemonResponse> {
        let environment_id = self.resolve_environment_id(environment).await?;
        let mut orchestration_service = self.orchestration_service.write().await;

        match orchestration_service
            .update_process_in_environment(&name, process, &environment_id)
            .await
        {
            Ok(_) => Ok(DaemonResponse::Success {
                message: format!("Process '{name}' updated successfully"),
            }),
            Err(e) => Ok(DaemonResponse::Error {
                error: format!("Failed to update process '{name}': {e}"),
            }),
        }
    }

    /// Get processes for a specific environment, combining configured and running processes
    pub async fn get_processes_for_environment(
        &self,
        environment_id: &str,
    ) -> Result<Vec<ProcessInfo>> {
        let orchestration_service = self.orchestration_service.read().await;
        orchestration_service
            .get_processes_for_environment(environment_id)
            .await
    }

    /// Get process summary for an environment (processes, total count, running count)
    pub async fn get_environment_process_summary(
        &self,
        environment_id: &str,
    ) -> Result<(Vec<ProcessInfo>, usize, usize)> {
        let orchestration_service = self.orchestration_service.read().await;
        orchestration_service
            .get_environment_process_summary(environment_id)
            .await
    }

    /// Environment resolution logic - validate and resolve environment with comprehensive error handling
    async fn resolve_environment_id(&self, environment_override: Option<String>) -> Result<String> {
        match environment_override {
            Some(env_id) => {
                // Validate that the environment exists and is active
                let orchestration_service = self.orchestration_service.read().await;
                if let Some(env_manager) = &orchestration_service.environment_manager {
                    let env_manager_guard = env_manager.read().await;
                    let env = env_manager_guard.get_environment(&env_id).ok_or_else(|| {
                        RunceptError::EnvironmentError(format!(
                            "Environment '{}' not found or not activated.\n\n\
                            Available environments: {}\n\n\
                            To fix this:\n\
                            1. Run 'runcept activate {}' to activate the environment\n\
                            2. Ensure the path contains a valid .runcept.toml file\n\
                            3. Check that the environment path is correct",
                            env_id,
                            env_manager_guard
                                .get_active_environments()
                                .into_iter()
                                .map(|(name, _)| name.as_str())
                                .collect::<Vec<_>>()
                                .join(", "),
                            env_id
                        ))
                    })?;

                    if !env.is_active() {
                        return Err(RunceptError::EnvironmentError(format!(
                            "Environment '{env_id}' is registered but not active.\n\n\
                            To fix this:\n\
                            1. Run 'runcept activate {env_id}' to activate the environment\n\
                            2. Ensure the environment configuration is valid"
                        )));
                    }

                    // Additional validation: ensure the environment has a valid project configuration
                    if env.project_config.processes.is_empty() {
                        log_debug(
                            "daemon",
                            &format!("Environment '{env_id}' has no processes defined"),
                            None,
                        );
                    }
                } else {
                    return Err(RunceptError::EnvironmentError(
                        "Environment manager not initialized".to_string(),
                    ));
                }
                Ok(env_id)
            }
            None => {
                // Fall back to current active environment
                let current_env_id = self.current_environment_id.read().await.clone();
                current_env_id.ok_or_else(|| {
                    RunceptError::EnvironmentError(
                        "No active environment available.\n\n\
                        This should not happen when using CLI environment resolution.\n\
                        To fix this:\n\
                        1. Run 'runcept activate' in a directory with .runcept.toml\n\
                        2. Use --environment flag to specify a project directory\n\
                        3. Check that the daemon is properly initialized"
                            .to_string(),
                    )
                })
            }
        }
    }
}
