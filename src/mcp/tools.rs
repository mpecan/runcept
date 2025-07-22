use crate::cli::commands::{DaemonRequest, DaemonResponse};
use crate::ipc::{IpcPath, connect};
use rmcp::handler::server::tool::Parameters;
use rmcp::{
    Error as McpError, RoleServer, ServerHandler, handler::server::router::tool::ToolRouter,
    model::*, schemars, service::RequestContext, tool, tool_handler, tool_router,
};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::RwLock;

/// Helper function to send request to daemon and get response
async fn send_daemon_request(
    request: DaemonRequest,
) -> std::result::Result<DaemonResponse, McpError> {
    use crate::logging::{log_debug, log_error, log_info};

    log_debug("mcp", &format!("Sending daemon request: {request:?}"), None);

    let socket_path = IpcPath::default_path().map_err(|e| {
        let err = McpError::internal_error(format!("Failed to get socket path: {e}"), None);
        log_error("mcp", &format!("Socket path error: {err}"), None);
        err
    })?;

    log_debug(
        "mcp",
        &format!("Connecting to daemon socket: {}", socket_path.as_str()),
        None,
    );

    let stream = connect(&socket_path).await.map_err(|e| {
        let err = McpError::internal_error(format!("Failed to connect to daemon: {e}"), None);
        log_error(
            "mcp",
            &format!("Connection error to {}: {}", socket_path.as_str(), err),
            None,
        );
        err
    })?;

    log_debug("mcp", "Connected to daemon successfully", None);

    let mut reader = BufReader::new(stream);
    let request_json = serde_json::to_string(&request).map_err(|e| {
        let err = McpError::internal_error(format!("Failed to serialize request: {e}"), None);
        log_error("mcp", &format!("Request serialization error: {err}"), None);
        err
    })?;

    log_debug(
        "mcp",
        &format!("Sending request JSON: {request_json}"),
        None,
    );

    // Send request
    {
        let writer = reader.get_mut();
        writer
            .write_all(format!("{request_json}\n").as_bytes())
            .await
            .map_err(|e| {
                let err = McpError::internal_error(format!("Failed to send request: {e}"), None);
                log_error("mcp", &format!("Write error: {err}"), None);
                err
            })?;
    }

    log_debug("mcp", "Request sent, waiting for response", None);

    // Read response
    let mut response_line = String::new();
    let bytes_read = reader.read_line(&mut response_line).await.map_err(|e| {
        let err = McpError::internal_error(format!("Failed to read response: {e}"), None);
        log_error("mcp", &format!("Read error: {err}"), None);
        err
    })?;

    log_debug(
        "mcp",
        &format!(
            "Received response ({} bytes): {}",
            bytes_read,
            response_line.trim()
        ),
        None,
    );

    if bytes_read == 0 {
        let err = McpError::internal_error(
            "Received empty response from daemon (EOF)".to_string(),
            None,
        );
        log_error("mcp", &format!("Empty response error: {err}"), None);
        return Err(err);
    }

    let response: DaemonResponse = serde_json::from_str(&response_line).map_err(|e| {
        let err = McpError::internal_error(format!("Failed to parse response: {e}"), None);
        log_error(
            "mcp",
            &format!(
                "Response parsing error for '{}': {}",
                response_line.trim(),
                err
            ),
            None,
        );
        err
    })?;

    log_info(
        "mcp",
        &format!("Successfully received daemon response: {response:?}"),
        None,
    );
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
    /// Name of the process to get logs for. Must be a process defined in the current environment.
    pub name: String,
    /// Number of recent log lines to retrieve. If not specified, returns all available logs. Maximum recommended: 1000 lines.
    pub lines: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EnvironmentPathParam {
    /// Path to the .runcept.toml file or directory containing it. Can be absolute or relative. If not provided, uses current working directory. Example: "/path/to/project" or "./my-project"
    pub path: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NameParam {
    /// Name of the process to manage. Must match a process defined in the current environment configuration.
    pub name: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct AddProcessParam {
    /// Unique name identifier for the process. Must not conflict with existing processes in the environment.
    pub name: String,
    /// Shell command to execute. Can include arguments. Example: "npm start", "python app.py --port 8000"
    pub command: String,
    /// Working directory for the process. Relative to project root if not absolute. Default: project root directory.
    pub working_dir: Option<String>,
    /// Whether to automatically restart the process if it exits. Default: false
    pub auto_restart: Option<bool>,
    /// HTTP URL for health checks. Process is considered healthy if this URL returns 2xx status. Example: "http://localhost:8000/health"
    pub health_check_url: Option<String>,
    /// Interval in seconds between health checks. Only used if health_check_url is provided. Default: 30 seconds
    pub health_check_interval: Option<u32>,
    /// Environment variables to set for the process. Key-value pairs that will be available in the process environment.
    pub env_vars: Option<std::collections::HashMap<String, String>>,
    /// List of process names this process depends on. This process will not start until all dependencies are running.
    pub depends_on: Option<Vec<String>>,
    /// Override target environment. If not provided, uses the currently active environment.
    pub environment: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RemoveProcessParam {
    /// Name of the process to remove from the configuration. The process will be stopped if currently running.
    pub name: String,
    /// Override target environment. If not provided, uses the currently active environment.
    pub environment: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UpdateProcessParam {
    /// Name of the existing process to update. Must match a process already defined in the environment.
    pub name: String,
    /// New shell command to execute. Can include arguments. Example: "npm run dev", "python app.py --debug"
    pub command: String,
    /// New working directory for the process. Relative to project root if not absolute.
    pub working_dir: Option<String>,
    /// Whether to automatically restart the process if it exits.
    pub auto_restart: Option<bool>,
    /// New HTTP URL for health checks. Process is considered healthy if this URL returns 2xx status.
    pub health_check_url: Option<String>,
    /// New interval in seconds between health checks. Only used if health_check_url is provided.
    pub health_check_interval: Option<u32>,
    /// New environment variables to set for the process. Replaces existing environment variables.
    pub env_vars: Option<std::collections::HashMap<String, String>>,
    /// New list of process names this process depends on. Replaces existing dependencies.
    pub depends_on: Option<Vec<String>>,
    /// Override target environment. If not provided, uses the currently active environment.
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
    #[tool(
        description = "Activate a project environment from .runcept.toml configuration. Auto-creates default config if none exists. Sets environment as active context for all process operations."
    )]
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
    #[tool(
        description = "Deactivate the current environment. Stops all processes and clears active environment context. Process operations will require explicit environment after this."
    )]
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
    #[tool(
        description = "Get comprehensive status of the current environment including process states, PIDs, uptime, and environment details. Shows all processes defined in the environment."
    )]
    async fn get_environment_status(&self) -> Result<CallToolResult, McpError> {
        // Record activity for the current environment
        let _ = self.record_current_environment_activity().await;

        let request = DaemonRequest::GetEnvironmentStatus;
        let response = send_daemon_request(request).await?;

        let result_text = format_daemon_response(response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// Start a specific process in the current environment
    #[tool(
        description = "Start a named process from the current environment configuration. Process must be defined in .runcept.toml. Launches with configured command, working directory, and environment variables."
    )]
    async fn start_process(
        &self,
        Parameters(NameParam { name }): Parameters<NameParam>,
    ) -> Result<CallToolResult, McpError> {
        // Record activity for the current environment
        let _ = self.record_current_environment_activity().await;

        // Use the current session environment
        let current_env = self.current_environment.read().await.clone();
        let request = DaemonRequest::StartProcess {
            name,
            environment: current_env,
        };
        let response = send_daemon_request(request).await?;

        let result_text = format_daemon_response(response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// Stop a running process
    #[tool(
        description = "Gracefully stop a running process by name. Sends SIGTERM first, then SIGKILL if needed. Process won't auto-restart even if configured to do so."
    )]
    async fn stop_process(
        &self,
        Parameters(NameParam { name }): Parameters<NameParam>,
    ) -> Result<CallToolResult, McpError> {
        // Record activity for the current environment
        let _ = self.record_current_environment_activity().await;

        // Use the current session environment
        let current_env = self.current_environment.read().await.clone();
        let request = DaemonRequest::StopProcess {
            name,
            environment: current_env,
        };
        let response = send_daemon_request(request).await?;

        let result_text = format_daemon_response(response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// Restart a process
    #[tool(
        description = "Stop and restart a process by name. Useful for applying configuration changes or recovering from errors. If process is not running, simply starts it."
    )]
    async fn restart_process(
        &self,
        Parameters(NameParam { name }): Parameters<NameParam>,
    ) -> Result<CallToolResult, McpError> {
        // Record activity for the current environment
        let _ = self.record_current_environment_activity().await;

        // Use the current session environment
        let current_env = self.current_environment.read().await.clone();
        let request = DaemonRequest::RestartProcess {
            name,
            environment: current_env,
        };
        let response = send_daemon_request(request).await?;

        let result_text = format_daemon_response(response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// List processes in the current environment
    #[tool(
        description = "List all processes in the current environment with their status (Running/Stopped/Failed), PID if running, uptime, and environment name."
    )]
    async fn list_processes(&self) -> Result<CallToolResult, McpError> {
        // Record activity for the current environment
        let _ = self.record_current_environment_activity().await;

        // Use the current session environment
        let current_env = self.current_environment.read().await.clone();
        let request = DaemonRequest::ListProcesses {
            environment: current_env,
        };
        let response = send_daemon_request(request).await?;

        let result_text = format_daemon_response(response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// Get logs for a specific process
    #[tool(
        description = "Retrieve log output from a specific process including stdout/stderr with timestamps and log levels. Specify number of lines or get all logs."
    )]
    async fn get_process_logs(
        &self,
        Parameters(LogsParam { name, lines }): Parameters<LogsParam>,
    ) -> Result<CallToolResult, McpError> {
        // Record activity for the current environment
        let _ = self.record_current_environment_activity().await;

        // Use the current session environment
        let current_env = self.current_environment.read().await.clone();
        let request = DaemonRequest::GetProcessLogs {
            name,
            lines,
            environment: current_env,
        };
        let response = send_daemon_request(request).await?;

        let result_text = format_daemon_response(response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// List all processes across all environments
    #[tool(
        description = "Get system-wide view of all processes across every environment with status, runtime info, and environment names. Useful for monitoring and debugging."
    )]
    async fn list_all_processes(&self) -> Result<CallToolResult, McpError> {
        let request = DaemonRequest::ListAllProcesses;
        let response = send_daemon_request(request).await?;

        let result_text = format_daemon_response(response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// Emergency stop all processes
    #[tool(
        description = "EMERGENCY: Immediately stop all processes across all environments. Use with caution - this will disrupt all running services and applications!"
    )]
    async fn kill_all_processes(&self) -> Result<CallToolResult, McpError> {
        let request = DaemonRequest::KillAllProcesses;
        let response = send_daemon_request(request).await?;

        let result_text = format_daemon_response(response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// Get daemon status and information
    #[tool(
        description = "Get detailed daemon status including running state, uptime, version, total processes managed, and active environment count."
    )]
    async fn get_daemon_status(&self) -> Result<CallToolResult, McpError> {
        let request = DaemonRequest::GetDaemonStatus;
        let response = send_daemon_request(request).await?;

        let result_text = format_daemon_response(response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// Record activity for the current environment (for inactivity tracking)
    #[tool(
        description = "Record activity for the current environment to reset inactivity timer and prevent auto-shutdown. Useful for keeping development environments active during extended work sessions."
    )]
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
    #[tool(
        description = "Dynamically add a new process to the project's .runcept.toml configuration. Process becomes immediately available for management operations. Supports full configuration options."
    )]
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
        use crate::logging::{log_debug, log_error, log_info};

        log_info(
            "mcp",
            &format!(
                "MCP add_process called: name='{name}', command='{command}', working_dir={working_dir:?}, environment={environment:?}"
            ),
            None,
        );

        // Record activity for the current environment
        let _ = self.record_current_environment_activity().await;

        // If no environment is explicitly provided, use the current MCP environment context
        let target_environment = if environment.is_some() {
            environment
        } else {
            let current_env = self.current_environment.read().await;
            current_env.clone()
        };

        log_debug(
            "mcp",
            &format!("Using target environment: {target_environment:?}"),
            None,
        );

        // Create ProcessDefinition from parameters
        let process_def = crate::config::ProcessDefinition {
            name: name.clone(),
            command: command.clone(),
            working_dir: working_dir.clone(),
            auto_restart,
            health_check_url: health_check_url.clone(),
            health_check_interval,
            depends_on: depends_on.unwrap_or_default(),
            env_vars: env_vars.unwrap_or_default(),
        };

        log_debug(
            "mcp",
            &format!("Created ProcessDefinition: {process_def:?}"),
            None,
        );

        let request = DaemonRequest::AddProcess {
            process: process_def,
            environment: target_environment,
        };

        log_debug("mcp", "Sending AddProcess request to daemon", None);

        let response = match send_daemon_request(request).await {
            Ok(resp) => {
                log_debug("mcp", &format!("Received daemon response: {resp:?}"), None);
                resp
            }
            Err(e) => {
                log_error("mcp", &format!("Failed to send daemon request: {e}"), None);
                return Err(e);
            }
        };

        let result_text = format_daemon_response(response);
        log_info(
            "mcp",
            &format!("MCP add_process completed with result: {result_text}"),
            None,
        );
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// Remove a process from the project configuration
    #[tool(
        description = "Remove a process from the project configuration and .runcept.toml file. Stops the process first if running. WARNING: This permanently removes the process definition."
    )]
    async fn remove_process(
        &self,
        Parameters(RemoveProcessParam { name, environment }): Parameters<RemoveProcessParam>,
    ) -> Result<CallToolResult, McpError> {
        // Record activity for the current environment
        let _ = self.record_current_environment_activity().await;

        // If no environment is explicitly provided, use the current MCP environment context
        let target_environment = if environment.is_some() {
            environment
        } else {
            let current_env = self.current_environment.read().await;
            current_env.clone()
        };

        let request = DaemonRequest::RemoveProcess {
            name,
            environment: target_environment,
        };
        let response = send_daemon_request(request).await?;

        let result_text = format_daemon_response(response);
        Ok(CallToolResult::success(vec![Content::text(result_text)]))
    }

    /// Update an existing process in the project configuration
    #[tool(
        description = "Update an existing process configuration in .runcept.toml. Only specified parameters are changed, others remain unchanged. Restart process to apply changes."
    )]
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

        // If no environment is explicitly provided, use the current MCP environment context
        let target_environment = if environment.is_some() {
            environment
        } else {
            let current_env = self.current_environment.read().await;
            current_env.clone()
        };

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
            environment: target_environment,
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
            instructions: Some("Runcept MCP Server - Advanced Process Management

This server provides comprehensive tools for managing long-running processes and development environments using Runcept. 

WORKFLOW:
1. First, activate an environment using 'activate_environment' (auto-creates config if needed)
2. Use process management tools: start_process, stop_process, restart_process, list_processes
3. Monitor with get_environment_status, get_process_logs, get_daemon_status
4. Dynamically modify configuration with add_process, remove_process, update_process

KEY FEATURES:
- Environment-based process management with .runcept.toml configuration
- Auto-restart, health checks, and dependency management
- Comprehensive logging and monitoring
- Inactivity timeout with auto-shutdown prevention
- Emergency controls for system-wide management

COMMON PATTERNS:
- Development setup: activate_environment → start_process for each service
- Monitoring: get_environment_status → get_process_logs for debugging
- Configuration: add_process → restart_process to apply changes
- Emergency: kill_all_processes when needed

Each tool provides detailed parameter documentation and usage examples.".to_string()),
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
