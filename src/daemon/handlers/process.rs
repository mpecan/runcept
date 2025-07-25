use crate::cli::commands::{DaemonResponse, LogLine, LogsResponse, ProcessInfo};
use crate::config::ProcessDefinition;
use crate::error::{Result, RunceptError};
use crate::logging::{log_debug, log_error, log_info};
use crate::process::{DefaultProcessOrchestrationService, ProcessOrchestrationTrait};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Handles for process-related operations - thin delegation layer
pub struct ProcessHandles<T: ProcessOrchestrationTrait> {
    orchestration_service: Arc<RwLock<T>>,
    current_environment_id: Arc<RwLock<Option<String>>>,
}

impl<T: ProcessOrchestrationTrait> Clone for ProcessHandles<T> {
    fn clone(&self) -> Self {
        Self {
            orchestration_service: Arc::clone(&self.orchestration_service),
            current_environment_id: Arc::clone(&self.current_environment_id),
        }
    }
}

/// Type alias for the default ProcessHandles implementation
pub type DefaultProcessHandles = ProcessHandles<DefaultProcessOrchestrationService>;

impl<T: ProcessOrchestrationTrait> ProcessHandles<T> {
    pub fn new(
        orchestration_service: Arc<RwLock<T>>,
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
                // For now, just return the provided environment ID
                // Environment validation will be handled by the orchestration service
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

    /// Helper method to validate environment exists and is active
    pub async fn validate_environment_exists(&self, _env_id: &str) -> Result<()> {
        // Environment validation is now handled by the orchestration service
        // This method is kept for compatibility but delegates validation
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::LogEntry;
    use crate::process::{LogLevel, MockProcessOrchestrationTrait};
    use chrono::Utc;
    use mockall::predicate::*;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_start_process_success() {
        let mut mock_service = MockProcessOrchestrationTrait::new();
        mock_service
            .expect_start_process_by_name_in_environment()
            .with(eq("test-process"), eq("test-env"))
            .times(1)
            .returning(|_, _| Ok("mock-process-id".to_string()));

        let handles = ProcessHandles::new(
            Arc::new(RwLock::new(mock_service)),
            Arc::new(RwLock::new(Some("test-env".to_string()))),
        );

        let result = handles
            .start_process("test-process".to_string(), None)
            .await;

        assert!(result.is_ok());
        match result.unwrap() {
            DaemonResponse::Success { message } => {
                assert_eq!(message, "Process 'test-process' started");
            }
            _ => panic!("Expected Success response"),
        }
    }

    #[tokio::test]
    async fn test_start_process_failure() {
        let mut mock_service = MockProcessOrchestrationTrait::new();
        mock_service
            .expect_start_process_by_name_in_environment()
            .with(eq("test-process"), eq("test-env"))
            .times(1)
            .returning(|_, _| Err(RunceptError::ProcessError("Mock failure".to_string())));

        let handles = ProcessHandles::new(
            Arc::new(RwLock::new(mock_service)),
            Arc::new(RwLock::new(Some("test-env".to_string()))),
        );

        let result = handles
            .start_process("test-process".to_string(), None)
            .await;

        assert!(result.is_ok());
        match result.unwrap() {
            DaemonResponse::Error { error } => {
                assert!(error.contains("Failed to start process 'test-process'"));
            }
            _ => panic!("Expected Error response"),
        }
    }

    #[tokio::test]
    async fn test_stop_process_success() {
        let mut mock_service = MockProcessOrchestrationTrait::new();
        mock_service
            .expect_stop_process_by_name_in_environment()
            .with(eq("test-process"), eq("test-env"))
            .times(1)
            .returning(|_, _| Ok(()));

        let handles = ProcessHandles::new(
            Arc::new(RwLock::new(mock_service)),
            Arc::new(RwLock::new(Some("test-env".to_string()))),
        );

        let result = handles.stop_process("test-process".to_string(), None).await;

        assert!(result.is_ok());
        match result.unwrap() {
            DaemonResponse::Success { message } => {
                assert_eq!(message, "Process 'test-process' stopped");
            }
            _ => panic!("Expected Success response"),
        }
    }

    #[tokio::test]
    async fn test_restart_process_success() {
        let mut mock_service = MockProcessOrchestrationTrait::new();
        mock_service
            .expect_restart_process_by_name_in_environment()
            .with(eq("test-process"), eq("test-env"))
            .times(1)
            .returning(|_, _| Ok(()));

        let handles = ProcessHandles::new(
            Arc::new(RwLock::new(mock_service)),
            Arc::new(RwLock::new(Some("test-env".to_string()))),
        );

        let result = handles
            .restart_process("test-process".to_string(), None)
            .await;

        assert!(result.is_ok());
        match result.unwrap() {
            DaemonResponse::Success { message } => {
                assert_eq!(message, "Process 'test-process' restarted");
            }
            _ => panic!("Expected Success response"),
        }
    }

    #[tokio::test]
    async fn test_list_processes() {
        let mock_processes = vec![ProcessInfo {
            id: "1".to_string(),
            name: "test-process".to_string(),
            status: "running".to_string(),
            pid: Some(1234),
            uptime: Some("5m".to_string()),
            environment: "test-env".to_string(),
            health_status: Some("healthy".to_string()),
            restart_count: 0,
            last_activity: Some("2024-01-01 12:00:00".to_string()),
        }];

        let mut mock_service = MockProcessOrchestrationTrait::new();
        mock_service
            .expect_list_processes_for_environment()
            .with(eq("test-env"))
            .times(1)
            .returning(move |_| Ok(mock_processes.clone()));

        let handles = ProcessHandles::new(
            Arc::new(RwLock::new(mock_service)),
            Arc::new(RwLock::new(Some("test-env".to_string()))),
        );

        let result = handles.list_processes(None).await;

        assert!(result.is_ok());
        match result.unwrap() {
            DaemonResponse::ProcessList(processes) => {
                assert_eq!(processes.len(), 1);
                assert_eq!(processes[0].name, "test-process");
                assert_eq!(processes[0].status, "running");
            }
            _ => panic!("Expected ProcessList response"),
        }
    }

    #[tokio::test]
    async fn test_add_process_success() {
        let mut mock_service = MockProcessOrchestrationTrait::new();
        mock_service
            .expect_add_process_to_environment()
            .with(always(), eq("test-env"))
            .times(1)
            .returning(|_, _| Ok(()));

        let handles = ProcessHandles::new(
            Arc::new(RwLock::new(mock_service)),
            Arc::new(RwLock::new(Some("test-env".to_string()))),
        );

        let process_def = ProcessDefinition {
            name: "new-process".to_string(),
            command: "echo hello".to_string(),
            working_dir: None,
            auto_restart: None,
            health_check_url: None,
            health_check_interval: None,
            depends_on: vec![],
            env_vars: HashMap::new(),
        };

        let result = handles.add_process(process_def, None).await;

        assert!(result.is_ok());
        match result.unwrap() {
            DaemonResponse::Success { message } => {
                assert_eq!(message, "Process 'new-process' added successfully");
            }
            _ => panic!("Expected Success response"),
        }
    }

    #[tokio::test]
    async fn test_get_process_logs() {
        let mock_logs = vec![LogEntry {
            timestamp: Utc::now(),
            level: LogLevel::Info,
            message: "Test log message".to_string(),
            process_id: Some("test-process".to_string()),
            process_name: Some("test-process".to_string()),
        }];

        let mut mock_service = MockProcessOrchestrationTrait::new();
        mock_service
            .expect_get_process_logs_by_name_in_environment()
            .with(eq("test-process"), eq("test-env"), eq(Some(10)))
            .times(1)
            .returning(move |_, _, _| Ok(mock_logs.clone()));

        let handles = ProcessHandles::new(
            Arc::new(RwLock::new(mock_service)),
            Arc::new(RwLock::new(Some("test-env".to_string()))),
        );

        let result = handles
            .get_process_logs("test-process".to_string(), Some(10), None)
            .await;

        assert!(result.is_ok());
        match result.unwrap() {
            DaemonResponse::ProcessLogs(logs_response) => {
                assert_eq!(logs_response.process_name, "test-process");
                assert_eq!(logs_response.lines.len(), 1);
                assert_eq!(logs_response.lines[0].level, "INFO");
                assert_eq!(logs_response.lines[0].message, "Test log message");
            }
            _ => panic!("Expected ProcessLogs response"),
        }
    }

    #[tokio::test]
    async fn test_no_active_environment_error() {
        let mock_service = MockProcessOrchestrationTrait::new();
        let handles = ProcessHandles::new(
            Arc::new(RwLock::new(mock_service)),
            Arc::new(RwLock::new(None)), // No active environment
        );

        let result = handles
            .start_process("test-process".to_string(), None)
            .await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("No active environment"));
    }

    #[test]
    fn test_process_definition_creation() {
        // Test creating a ProcessDefinition for testing
        let process_def = ProcessDefinition {
            name: "test-process".to_string(),
            command: "echo hello".to_string(),
            working_dir: None,
            auto_restart: None,
            health_check_url: None,
            health_check_interval: None,
            depends_on: vec![],
            env_vars: std::collections::HashMap::new(),
        };

        assert_eq!(process_def.name, "test-process");
        assert_eq!(process_def.command, "echo hello");
        assert!(process_def.working_dir.is_none());
        assert!(process_def.auto_restart.is_none());
        assert!(process_def.depends_on.is_empty());
        assert!(process_def.env_vars.is_empty());
    }
}
