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
    use async_trait::async_trait;
    use crate::process::{LogEntry, LogLevel};
    use std::collections::HashMap;
    use tokio::sync::Mutex;

    /// Mock implementation of ProcessOrchestrationTrait for testing
    #[derive(Default)]
    pub struct MockProcessOrchestrationService {
        pub start_calls: Arc<Mutex<Vec<(String, String)>>>,
        pub stop_calls: Arc<Mutex<Vec<(String, String)>>>,
        pub restart_calls: Arc<Mutex<Vec<(String, String)>>>,
        pub add_calls: Arc<Mutex<Vec<(ProcessDefinition, String)>>>,
        pub remove_calls: Arc<Mutex<Vec<(String, String)>>>,
        pub update_calls: Arc<Mutex<Vec<(String, ProcessDefinition, String)>>>,
        pub logs_calls: Arc<Mutex<Vec<(String, String, Option<usize>)>>>,
        pub list_calls: Arc<Mutex<Vec<String>>>,
        pub get_processes_calls: Arc<Mutex<Vec<String>>>,
        pub summary_calls: Arc<Mutex<Vec<String>>>,
        pub stop_all_calls: Arc<Mutex<Vec<String>>>,
        
        // Mock responses
        pub should_fail: bool,
        pub mock_processes: Vec<ProcessInfo>,
        pub mock_logs: Vec<LogEntry>,
    }

    impl MockProcessOrchestrationService {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn with_failure(mut self) -> Self {
            self.should_fail = true;
            self
        }

        pub fn with_processes(mut self, processes: Vec<ProcessInfo>) -> Self {
            self.mock_processes = processes;
            self
        }

        pub fn with_logs(mut self, logs: Vec<LogEntry>) -> Self {
            self.mock_logs = logs;
            self
        }
    }

    #[async_trait]
    impl ProcessOrchestrationTrait for MockProcessOrchestrationService {
        async fn start_process_by_name_in_environment(
            &mut self,
            process_name: &str,
            environment_id: &str,
        ) -> Result<String> {
            self.start_calls.lock().await.push((process_name.to_string(), environment_id.to_string()));
            if self.should_fail {
                Err(RunceptError::ProcessError("Mock start failure".to_string()))
            } else {
                Ok("mock-process-id".to_string())
            }
        }

        async fn stop_process_by_name_in_environment(
            &mut self,
            process_name: &str,
            environment_id: &str,
        ) -> Result<()> {
            self.stop_calls.lock().await.push((process_name.to_string(), environment_id.to_string()));
            if self.should_fail {
                Err(RunceptError::ProcessError("Mock stop failure".to_string()))
            } else {
                Ok(())
            }
        }

        async fn restart_process_by_name_in_environment(
            &mut self,
            process_name: &str,
            environment_id: &str,
        ) -> Result<()> {
            self.restart_calls.lock().await.push((process_name.to_string(), environment_id.to_string()));
            if self.should_fail {
                Err(RunceptError::ProcessError("Mock restart failure".to_string()))
            } else {
                Ok(())
            }
        }

        async fn add_process_to_environment(
            &mut self,
            process_def: ProcessDefinition,
            environment_id: &str,
        ) -> Result<()> {
            self.add_calls.lock().await.push((process_def, environment_id.to_string()));
            if self.should_fail {
                Err(RunceptError::ProcessError("Mock add failure".to_string()))
            } else {
                Ok(())
            }
        }

        async fn remove_process_from_environment(
            &mut self,
            process_name: &str,
            environment_id: &str,
        ) -> Result<()> {
            self.remove_calls.lock().await.push((process_name.to_string(), environment_id.to_string()));
            if self.should_fail {
                Err(RunceptError::ProcessError("Mock remove failure".to_string()))
            } else {
                Ok(())
            }
        }

        async fn update_process_in_environment(
            &mut self,
            process_name: &str,
            process_def: ProcessDefinition,
            environment_id: &str,
        ) -> Result<()> {
            self.update_calls.lock().await.push((process_name.to_string(), process_def, environment_id.to_string()));
            if self.should_fail {
                Err(RunceptError::ProcessError("Mock update failure".to_string()))
            } else {
                Ok(())
            }
        }

        async fn get_process_logs_by_name_in_environment(
            &self,
            process_name: &str,
            environment_id: &str,
            lines: Option<usize>,
        ) -> Result<Vec<LogEntry>> {
            self.logs_calls.lock().await.push((process_name.to_string(), environment_id.to_string(), lines));
            if self.should_fail {
                Err(RunceptError::ProcessError("Mock logs failure".to_string()))
            } else {
                Ok(self.mock_logs.clone())
            }
        }

        async fn list_processes_for_environment(&self, environment_id: &str) -> Result<Vec<ProcessInfo>> {
            self.list_calls.lock().await.push(environment_id.to_string());
            if self.should_fail {
                Err(RunceptError::ProcessError("Mock list failure".to_string()))
            } else {
                Ok(self.mock_processes.clone())
            }
        }

        async fn get_processes_for_environment(&self, environment_id: &str) -> Result<Vec<ProcessInfo>> {
            self.get_processes_calls.lock().await.push(environment_id.to_string());
            if self.should_fail {
                Err(RunceptError::ProcessError("Mock get processes failure".to_string()))
            } else {
                Ok(self.mock_processes.clone())
            }
        }

        async fn get_environment_process_summary(
            &self,
            environment_id: &str,
        ) -> Result<(Vec<ProcessInfo>, usize, usize)> {
            self.summary_calls.lock().await.push(environment_id.to_string());
            if self.should_fail {
                Err(RunceptError::ProcessError("Mock summary failure".to_string()))
            } else {
                let processes = self.mock_processes.clone();
                let total = processes.len();
                let running = processes.iter().filter(|p| p.status == "running").count();
                Ok((processes, total, running))
            }
        }

        async fn stop_all_processes_in_environment(&mut self, environment_id: &str) -> Result<()> {
            self.stop_all_calls.lock().await.push(environment_id.to_string());
            if self.should_fail {
                Err(RunceptError::ProcessError("Mock stop all failure".to_string()))
            } else {
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn test_start_process_success() {
        let mock_service = MockProcessOrchestrationService::new();
        let handles = ProcessHandles::new(
            Arc::new(RwLock::new(mock_service)),
            Arc::new(RwLock::new(Some("test-env".to_string()))),
        );

        let result = handles.start_process("test-process".to_string(), None).await;
        
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
        let mock_service = MockProcessOrchestrationService::new().with_failure();
        let handles = ProcessHandles::new(
            Arc::new(RwLock::new(mock_service)),
            Arc::new(RwLock::new(Some("test-env".to_string()))),
        );

        let result = handles.start_process("test-process".to_string(), None).await;
        
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
        let mock_service = MockProcessOrchestrationService::new();
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
        let mock_service = MockProcessOrchestrationService::new();
        let handles = ProcessHandles::new(
            Arc::new(RwLock::new(mock_service)),
            Arc::new(RwLock::new(Some("test-env".to_string()))),
        );

        let result = handles.restart_process("test-process".to_string(), None).await;
        
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
        let mock_processes = vec![
            ProcessInfo {
                id: "1".to_string(),
                name: "test-process".to_string(),
                status: "running".to_string(),
                pid: Some(1234),
                uptime: Some("5m".to_string()),
                environment: "test-env".to_string(),
                health_status: Some("healthy".to_string()),
                restart_count: 0,
                last_activity: Some("2024-01-01 12:00:00".to_string()),
            }
        ];
        
        let mock_service = MockProcessOrchestrationService::new().with_processes(mock_processes.clone());
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
        let mock_service = MockProcessOrchestrationService::new();
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
        use chrono::Utc;
        use crate::process::LogEntry;
        
        let mock_logs = vec![
            LogEntry {
                timestamp: Utc::now(),
                level: LogLevel::Info,
                message: "Test log message".to_string(),
                process_id: Some("test-process".to_string()),
                process_name: Some("test-process".to_string()),
            }
        ];
        
        let mock_service = MockProcessOrchestrationService::new().with_logs(mock_logs);
        let handles = ProcessHandles::new(
            Arc::new(RwLock::new(mock_service)),
            Arc::new(RwLock::new(Some("test-env".to_string()))),
        );

        let result = handles.get_process_logs("test-process".to_string(), Some(10), None).await;
        
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
        let mock_service = MockProcessOrchestrationService::new();
        let handles = ProcessHandles::new(
            Arc::new(RwLock::new(mock_service)),
            Arc::new(RwLock::new(None)), // No active environment
        );

        let result = handles.start_process("test-process".to_string(), None).await;
        
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
