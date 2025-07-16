use crate::cli::commands::{DaemonRequest, DaemonResponse};
use rmcp::handler::server::tool::Parameters;
use rmcp::{
    Error as McpError, RoleServer, ServerHandler, handler::server::router::tool::ToolRouter,
    model::*, schemars, service::RequestContext, tool, tool_handler, tool_router,
};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::RwLock;

/// Helper function to send request to daemon and get response
async fn send_daemon_request(
    request: DaemonRequest,
) -> std::result::Result<DaemonResponse, McpError> {
    let config_dir = crate::config::global::get_config_dir()
        .map_err(|e| McpError::internal_error(format!("Failed to get config dir: {e}"), None))?;
    let socket_path = config_dir.join("daemon.sock");

    let stream = UnixStream::connect(socket_path)
        .await
        .map_err(|e| McpError::internal_error(format!("Failed to connect to daemon: {e}"), None))?;

    let mut reader = BufReader::new(stream);
    let request_json = serde_json::to_string(&request)
        .map_err(|e| McpError::internal_error(format!("Failed to serialize request: {e}"), None))?;

    // Send request
    {
        let writer = reader.get_mut();
        writer
            .write_all(format!("{request_json}\n").as_bytes())
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to send request: {e}"), None))?;
    }

    // Read response
    let mut response_line = String::new();
    reader
        .read_line(&mut response_line)
        .await
        .map_err(|e| McpError::internal_error(format!("Failed to read response: {e}"), None))?;

    let response: DaemonResponse = serde_json::from_str(&response_line)
        .map_err(|e| McpError::internal_error(format!("Failed to parse response: {e}"), None))?;

    Ok(response)
}

/// Format daemon response as string for MCP tool output
fn format_daemon_response(response: DaemonResponse) -> String {
    match response {
        DaemonResponse::Success { message } => message,
        DaemonResponse::Error { error } => format!("Error: {error}"),
        DaemonResponse::DaemonStatus(status) => {
            format!(
                "Daemon Status:\n- Running: {}\n- Uptime: {}\n- Version: {}\n- Total Processes: {}\n- Active Environments: {}",
                status.running,
                status.uptime,
                status.version,
                status.total_processes,
                status.active_environments
            )
        }
        DaemonResponse::EnvironmentStatus(status) => {
            let mut output = String::new();

            if let Some(active_env) = &status.active_environment {
                output.push_str(&format!("Active Environment: {active_env}\n"));
            } else {
                output.push_str("No active environment\n");
            }

            if let Some(project_path) = &status.project_path {
                output.push_str(&format!("Project Path: {project_path}\n"));
            }

            output.push_str(&format!("Total Processes: {}\n", status.total_processes));
            output.push_str(&format!(
                "Running Processes: {}\n",
                status.running_processes
            ));

            if !status.processes.is_empty() {
                output.push_str("\nProcesses:\n");
                for process in &status.processes {
                    output.push_str(&format!(
                        "  - {} [{}] ({})\n",
                        process.name, process.status, process.environment
                    ));
                    if let Some(pid) = process.pid {
                        output.push_str(&format!("    PID: {pid}\n"));
                    }
                    if let Some(uptime) = &process.uptime {
                        output.push_str(&format!("    Uptime: {uptime}\n"));
                    }
                }
            }

            output
        }
        DaemonResponse::ProcessList(processes) => {
            if processes.is_empty() {
                "No processes found".to_string()
            } else {
                let mut output = String::new();
                output.push_str("Processes:\n");
                for process in processes {
                    output.push_str(&format!(
                        "  - {} [{}] ({})\n",
                        process.name, process.status, process.environment
                    ));
                    if let Some(pid) = process.pid {
                        output.push_str(&format!("    PID: {pid}\n"));
                    }
                    if let Some(uptime) = &process.uptime {
                        output.push_str(&format!("    Uptime: {uptime}\n"));
                    }
                }
                output
            }
        }
        DaemonResponse::ProcessLogs(logs) => {
            if logs.lines.is_empty() {
                format!("No logs found for process '{}'", logs.process_name)
            } else {
                let mut output = String::new();
                output.push_str(&format!(
                    "Logs for process '{}' ({} lines):\n",
                    logs.process_name, logs.total_lines
                ));
                for line in logs.lines {
                    output.push_str(&format!(
                        "{} [{}] {}\n",
                        line.timestamp, line.level, line.message
                    ));
                }
                output
            }
        }
    }
}

/// Main tools struct for Runcept MCP server
#[derive(Clone, Debug)]
pub struct RunceptTools {
    pub tool_router: ToolRouter<RunceptTools>,
    pub current_environment: Arc<RwLock<Option<String>>>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct LogsParam {
    /// Name of the process to get logs for
    pub name: String,
    /// Number of lines to retrieve from the logs
    pub lines: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EnvironmentPathParam {
    /// Path to the .runcept.toml file or directory containing it
    pub path: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NameParam {
    /// Name of the process to manage
    pub name: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct AddProcessParam {
    /// Name of the process to add
    pub name: String,
    /// Command to execute
    pub command: String,
    /// Working directory (optional)
    pub working_dir: Option<String>,
    /// Whether to auto-restart the process
    pub auto_restart: Option<bool>,
    /// Health check URL (optional)
    pub health_check_url: Option<String>,
    /// Health check interval in seconds (optional)
    pub health_check_interval: Option<u32>,
    /// Environment variables (optional)
    pub env_vars: Option<std::collections::HashMap<String, String>>,
    /// Processes this one depends on (optional)
    pub depends_on: Option<Vec<String>>,
    /// Override environment (optional)
    pub environment: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RemoveProcessParam {
    /// Name of the process to remove
    pub name: String,
    /// Override environment (optional)
    pub environment: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UpdateProcessParam {
    /// Name of the process to update
    pub name: String,
    /// Command to execute
    pub command: String,
    /// Working directory (optional)
    pub working_dir: Option<String>,
    /// Whether to auto-restart the process
    pub auto_restart: Option<bool>,
    /// Health check URL (optional)
    pub health_check_url: Option<String>,
    /// Health check interval in seconds (optional)
    pub health_check_interval: Option<u32>,
    /// Environment variables (optional)
    pub env_vars: Option<std::collections::HashMap<String, String>>,
    /// Processes this one depends on (optional)
    pub depends_on: Option<Vec<String>>,
    /// Override environment (optional)
    pub environment: Option<String>,
}

impl Default for RunceptTools {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router]
impl RunceptTools {
    /// Create new RunceptTools instance
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
            current_environment: Arc::new(RwLock::new(None)),
        }
    }

    /// Helper method to record activity for the current environment
    async fn record_current_environment_activity(&self) -> Result<(), McpError> {
        let current_env = self.current_environment.read().await;
        if let Some(env_id) = current_env.as_ref() {
            let activity_request = DaemonRequest::RecordEnvironmentActivity {
                environment_id: env_id.clone(),
            };
            let _ = send_daemon_request(activity_request).await; // Ignore activity tracking errors
        }
        Ok(())
    }

    /// Activate a project environment from .runcept.toml configuration
    #[tool(description = "Activate a project environment from .runcept.toml configuration")]
    async fn activate_environment(
        &self,
        Parameters(EnvironmentPathParam { path }): Parameters<EnvironmentPathParam>,
    ) -> Result<CallToolResult, McpError> {
        // Resolve path to absolute path for both daemon request and environment ID
        let absolute_path = if let Some(p) = path {
            // Convert to absolute path
            std::path::Path::new(&p)
                .canonicalize()
                .map(|abs_path| abs_path.to_string_lossy().to_string())
                .unwrap_or_else(|_| p) // Fallback to original path if canonicalize fails
        } else {
            // Use current working directory as absolute path
            std::env::current_dir()
                .map(|cwd| cwd.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string())
        };

        // Send activation request with absolute path
        let request = DaemonRequest::ActivateEnvironment {
            path: Some(absolute_path.clone()),
        };
        let response = send_daemon_request(request).await?;

        // Check if activation failed due to missing configuration
        let final_response = match &response {
            DaemonResponse::Error { error } if error.contains("No .runcept.toml found") => {
                // Automatically create default configuration for MCP usage
                let init_request = DaemonRequest::InitProject {
                    path: Some(absolute_path.clone()),
                    force: false,
                };

                match send_daemon_request(init_request).await? {
                    DaemonResponse::Success { .. } => {
                        // Configuration created successfully, try activation again
                        let retry_request = DaemonRequest::ActivateEnvironment {
                            path: Some(absolute_path.clone()),
                        };
                        send_daemon_request(retry_request).await?
                    }
                    _init_error => {
                        // Failed to create config, return original error
                        response
                    }
                }
            }
            _ => response,
        };

        // If activation was successful, update our current environment state
        if let DaemonResponse::Success { .. } = final_response {
            *self.current_environment.write().await = Some(absolute_path.clone());

            // Record initial activity for the newly activated environment
            let activity_request = DaemonRequest::RecordEnvironmentActivity {
                environment_id: absolute_path,
            };
            let _ = send_daemon_request(activity_request).await; // Ignore activity tracking errors
        }

        let result_text = format_daemon_response(final_response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// Deactivate the current environment
    #[tool(description = "Deactivate the current environment")]
    async fn deactivate_environment(&self) -> Result<CallToolResult, McpError> {
        let request = DaemonRequest::DeactivateEnvironment;
        let response = send_daemon_request(request).await?;

        // If deactivation was successful, clear our current environment state
        if let DaemonResponse::Success { .. } = response {
            *self.current_environment.write().await = None;
        }

        let result_text = format_daemon_response(response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// Get the status of the current environment
    #[tool(description = "Get the status of the current environment")]
    async fn get_environment_status(&self) -> Result<CallToolResult, McpError> {
        // Record activity for the current environment
        let _ = self.record_current_environment_activity().await;

        let request = DaemonRequest::GetEnvironmentStatus;
        let response = send_daemon_request(request).await?;

        let result_text = format_daemon_response(response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// Start a specific process in the current environment
    #[tool(description = "Start a specific process in the current environment")]
    async fn start_process(
        &self,
        Parameters(NameParam { name }): Parameters<NameParam>,
    ) -> Result<CallToolResult, McpError> {
        // Record activity for the current environment
        let _ = self.record_current_environment_activity().await;

        let request = DaemonRequest::StartProcess { name };
        let response = send_daemon_request(request).await?;

        let result_text = format_daemon_response(response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// Stop a running process
    #[tool(description = "Stop a running process")]
    async fn stop_process(
        &self,
        Parameters(NameParam { name }): Parameters<NameParam>,
    ) -> Result<CallToolResult, McpError> {
        // Record activity for the current environment
        let _ = self.record_current_environment_activity().await;

        let request = DaemonRequest::StopProcess { name };
        let response = send_daemon_request(request).await?;

        let result_text = format_daemon_response(response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// Restart a process
    #[tool(description = "Restart a process")]
    async fn restart_process(
        &self,
        Parameters(NameParam { name }): Parameters<NameParam>,
    ) -> Result<CallToolResult, McpError> {
        // Record activity for the current environment
        let _ = self.record_current_environment_activity().await;

        let request = DaemonRequest::RestartProcess { name };
        let response = send_daemon_request(request).await?;

        let result_text = format_daemon_response(response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// List processes in the current environment
    #[tool(description = "List processes in the current environment")]
    async fn list_processes(&self) -> Result<CallToolResult, McpError> {
        // Record activity for the current environment
        let _ = self.record_current_environment_activity().await;

        let request = DaemonRequest::ListProcesses;
        let response = send_daemon_request(request).await?;

        let result_text = format_daemon_response(response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// Get logs for a specific process
    #[tool(description = "Get logs for a specific process")]
    async fn get_process_logs(
        &self,
        Parameters(LogsParam { name, lines }): Parameters<LogsParam>,
    ) -> Result<CallToolResult, McpError> {
        // Record activity for the current environment
        let _ = self.record_current_environment_activity().await;

        let request = DaemonRequest::GetProcessLogs { name, lines };
        let response = send_daemon_request(request).await?;

        let result_text = format_daemon_response(response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// List all processes across all environments
    #[tool(description = "List all processes across all environments")]
    async fn list_all_processes(&self) -> Result<CallToolResult, McpError> {
        let request = DaemonRequest::ListAllProcesses;
        let response = send_daemon_request(request).await?;

        let result_text = format_daemon_response(response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// Emergency stop all processes
    #[tool(description = "Emergency stop all processes")]
    async fn kill_all_processes(&self) -> Result<CallToolResult, McpError> {
        let request = DaemonRequest::KillAllProcesses;
        let response = send_daemon_request(request).await?;

        let result_text = format_daemon_response(response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// Get daemon status and information
    #[tool(description = "Get daemon status and information")]
    async fn get_daemon_status(&self) -> Result<CallToolResult, McpError> {
        let request = DaemonRequest::GetDaemonStatus;
        let response = send_daemon_request(request).await?;

        let result_text = format_daemon_response(response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// Record activity for the current environment (for inactivity tracking)
    #[tool(description = "Record activity for the current environment to prevent auto-shutdown")]
    async fn record_environment_activity(&self) -> Result<CallToolResult, McpError> {
        let _ = self.record_current_environment_activity().await;

        let current_env = self.current_environment.read().await;
        if let Some(env_id) = current_env.as_ref() {
            Ok(CallToolResult::success(vec![Content::text(format!(
                "Recorded activity for environment '{env_id}'"
            ))]))
        } else {
            Ok(CallToolResult::success(vec![Content::text(
                "No active environment to record activity for".to_string(),
            )]))
        }
    }

    /// Add a new process to the project configuration
    #[tool(description = "Add a new process to the project configuration")]
    async fn add_process(
        &self,
        Parameters(AddProcessParam {
            name,
            command,
            working_dir,
            auto_restart,
            health_check_url,
            health_check_interval,
            env_vars,
            depends_on,
            environment,
        }): Parameters<AddProcessParam>,
    ) -> Result<CallToolResult, McpError> {
        // Record activity for the current environment
        let _ = self.record_current_environment_activity().await;

        // Create ProcessDefinition from parameters
        let process_def = crate::config::ProcessDefinition {
            name: name.clone(),
            command,
            working_dir,
            auto_restart,
            health_check_url,
            health_check_interval,
            depends_on: depends_on.unwrap_or_default(),
            env_vars: env_vars.unwrap_or_default(),
        };

        let request = DaemonRequest::AddProcess {
            process: process_def,
            environment,
        };
        let response = send_daemon_request(request).await?;

        let result_text = format_daemon_response(response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// Remove a process from the project configuration
    #[tool(description = "Remove a process from the project configuration")]
    async fn remove_process(
        &self,
        Parameters(RemoveProcessParam { name, environment }): Parameters<RemoveProcessParam>,
    ) -> Result<CallToolResult, McpError> {
        // Record activity for the current environment
        let _ = self.record_current_environment_activity().await;

        let request = DaemonRequest::RemoveProcess { name, environment };
        let response = send_daemon_request(request).await?;

        let result_text = format_daemon_response(response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// Update an existing process in the project configuration
    #[tool(description = "Update an existing process in the project configuration")]
    async fn update_process(
        &self,
        Parameters(UpdateProcessParam {
            name,
            command,
            working_dir,
            auto_restart,
            health_check_url,
            health_check_interval,
            env_vars,
            depends_on,
            environment,
        }): Parameters<UpdateProcessParam>,
    ) -> Result<CallToolResult, McpError> {
        // Record activity for the current environment
        let _ = self.record_current_environment_activity().await;

        // Create ProcessDefinition from parameters
        let process_def = crate::config::ProcessDefinition {
            name: name.clone(),
            command,
            working_dir,
            auto_restart,
            health_check_url,
            health_check_interval,
            depends_on: depends_on.unwrap_or_default(),
            env_vars: env_vars.unwrap_or_default(),
        };

        let request = DaemonRequest::UpdateProcess {
            name,
            process: process_def,
            environment,
        };
        let response = send_daemon_request(request).await?;

        let result_text = format_daemon_response(response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }
}

#[tool_handler]
impl ServerHandler for RunceptTools {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            server_info: Implementation {
                name: "runcept-mcp".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            instructions: Some("This server provides tools for managing Runcept processes and environments. Use the available tools to start, stop, restart processes, manage environments, and get status information.".to_string()),
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        // We don't have resources in this implementation
        Ok(ListResourcesResult {
            resources: vec![],
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        _request: ReadResourceRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        // We don't have resources in this implementation
        Err(McpError::resource_not_found("No resources available", None))
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        // We don't have prompts in this implementation
        Ok(ListPromptsResult {
            prompts: vec![],
            next_cursor: None,
        })
    }

    async fn get_prompt(
        &self,
        _request: GetPromptRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        // We don't have prompts in this implementation
        Err(McpError::invalid_params("No prompts available", None))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        // We don't have resource templates in this implementation
        Ok(ListResourceTemplatesResult {
            resource_templates: vec![],
            next_cursor: None,
        })
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        Ok(self.get_info())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runcept_tools_creation() {
        let tools = RunceptTools::new();
        // Just test that we can create the struct
        assert!(format!("{tools:?}").contains("RunceptTools"));
    }

    #[test]
    fn test_format_daemon_response_success() {
        let response = DaemonResponse::Success {
            message: "Operation completed".to_string(),
        };
        let formatted = format_daemon_response(response);
        assert_eq!(formatted, "Operation completed");
    }

    #[test]
    fn test_format_daemon_response_error() {
        let response = DaemonResponse::Error {
            error: "Something went wrong".to_string(),
        };
        let formatted = format_daemon_response(response);
        assert_eq!(formatted, "Error: Something went wrong");
    }
}
