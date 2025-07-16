#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_args_parsing() {
        // Test environment commands
        let args = CliArgs::try_parse_from(&["runcept", "activate", "/path/to/project"]).unwrap();
        match args.command {
            Commands::Activate { path } => {
                assert_eq!(path, Some("/path/to/project".to_string()));
            }
            _ => panic!("Expected Activate command"),
        }

        // Test process commands
        let args = CliArgs::try_parse_from(&["runcept", "start", "web-server"]).unwrap();
        match args.command {
            Commands::Start { name } => {
                assert_eq!(name, "web-server");
            }
            _ => panic!("Expected Start command"),
        }

        // Test global commands
        let args = CliArgs::try_parse_from(&["runcept", "ps"]).unwrap();
        match args.command {
            Commands::Ps => {}
            _ => panic!("Expected Ps command"),
        }
    }

    #[test]
    fn test_logs_command_parsing() {
        // Test logs with follow
        let args = CliArgs::try_parse_from(&[
            "runcept",
            "logs",
            "web-server",
            "--follow",
            "--lines",
            "50",
        ])
        .unwrap();
        match args.command {
            Commands::Logs {
                name,
                follow,
                lines,
            } => {
                assert_eq!(name, "web-server");
                assert!(follow);
                assert_eq!(lines, Some(50));
            }
            _ => panic!("Expected Logs command"),
        }
    }

    #[test]
    fn test_daemon_commands() {
        // Test daemon start
        let args = CliArgs::try_parse_from(&["runcept", "daemon", "start"]).unwrap();
        match args.command {
            Commands::Daemon { action } => match action {
                DaemonAction::Start { foreground: _ } => {}
                _ => panic!("Expected Start action"),
            },
            _ => panic!("Expected Daemon command"),
        }

        // Test daemon status
        let args = CliArgs::try_parse_from(&["runcept", "daemon", "status"]).unwrap();
        match args.command {
            Commands::Daemon { action } => match action {
                DaemonAction::Status => {}
                _ => panic!("Expected Status action"),
            },
            _ => panic!("Expected Daemon command"),
        }
    }
}

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Runcept - A process manager for development environments
#[derive(Parser, Debug)]
#[command(name = "runcept")]
#[command(about = "A process manager that runs and intercepts sync processes")]
#[command(version = env!("CARGO_PKG_VERSION"))]
pub struct CliArgs {
    /// Enable verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Override default daemon socket path
    #[arg(long, global = true)]
    pub socket: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Environment management commands
    #[group(id = "environment")]

    /// Activate a project environment from .runcept.toml
    Activate {
        /// Path to project directory (defaults to current directory)
        path: Option<String>,
    },

    /// Deactivate the current environment
    Deactivate,

    /// Show environment and process status
    Status,

    /// Initialize a new project with default .runcept.toml configuration
    Init {
        /// Path to project directory (defaults to current directory)
        path: Option<String>,
        /// Force overwrite existing .runcept.toml file
        #[arg(short, long)]
        force: bool,
    },

    /// Process management commands
    #[group(id = "process")]

    /// Start a specific process in the current environment
    Start {
        /// Name of the process to start
        name: String,
    },

    /// Stop a running process
    Stop {
        /// Name of the process to stop
        name: String,
    },

    /// Restart a process
    Restart {
        /// Name of the process to restart
        name: String,
    },

    /// List processes in current environment
    List,

    /// View process logs
    Logs {
        /// Name of the process
        name: String,
        /// Follow log output
        #[arg(short, long)]
        follow: bool,
        /// Number of lines to show from the end
        #[arg(short = 'n', long)]
        lines: Option<usize>,
    },

    /// Global commands
    #[group(id = "global")]

    /// List all processes across all environments
    Ps,

    /// Stop all processes in all environments (emergency stop)
    KillAll,

    /// Daemon management commands
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum DaemonAction {
    /// Start the daemon
    Start {
        /// Run daemon in foreground
        #[arg(short, long)]
        foreground: bool,
    },
    /// Stop the daemon
    Stop,
    /// Restart the daemon
    Restart,
    /// Show daemon status
    Status,
}

/// Request types for daemon communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DaemonRequest {
    // Environment commands
    ActivateEnvironment { path: Option<String> },
    DeactivateEnvironment,
    GetEnvironmentStatus,
    InitProject { path: Option<String>, force: bool },

    // Process commands
    StartProcess { name: String },
    StopProcess { name: String },
    RestartProcess { name: String },
    ListProcesses,
    GetProcessLogs { name: String, lines: Option<usize> },

    // Global commands
    ListAllProcesses,
    KillAllProcesses,

    // Daemon commands
    GetDaemonStatus,
    Shutdown,

    // Activity tracking commands
    RecordEnvironmentActivity { environment_id: String },
}

/// Response types for daemon communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DaemonResponse {
    Success { message: String },
    Error { error: String },
    EnvironmentStatus(EnvironmentStatusResponse),
    ProcessList(Vec<ProcessInfo>),
    ProcessLogs(LogsResponse),
    DaemonStatus(DaemonStatusResponse),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentStatusResponse {
    pub active_environment: Option<String>,
    pub project_path: Option<String>,
    pub processes: Vec<ProcessInfo>,
    pub total_processes: usize,
    pub running_processes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub id: String,
    pub name: String,
    pub status: String,
    pub pid: Option<u32>,
    pub uptime: Option<String>,
    pub environment: String,
    pub health_status: Option<String>,
    pub restart_count: u32,
    pub last_activity: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogsResponse {
    pub process_name: String,
    pub lines: Vec<LogLine>,
    pub total_lines: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogLine {
    pub timestamp: String,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatusResponse {
    pub running: bool,
    pub uptime: String,
    pub version: String,
    pub socket_path: String,
    pub total_processes: usize,
    pub active_environments: usize,
    pub memory_usage: Option<String>,
}

/// CLI command execution result
#[derive(Debug)]
pub enum CliResult {
    Success(String),
    Error(String),
}

impl std::fmt::Display for CliResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliResult::Success(msg) => write!(f, "{msg}"),
            CliResult::Error(msg) => write!(f, "Error: {msg}"),
        }
    }
}

impl From<DaemonResponse> for CliResult {
    fn from(response: DaemonResponse) -> Self {
        match response {
            DaemonResponse::Success { message } => CliResult::Success(message),
            DaemonResponse::Error { error } => CliResult::Error(error),
            DaemonResponse::EnvironmentStatus(status) => {
                CliResult::Success(format_environment_status(status))
            }
            DaemonResponse::ProcessList(processes) => {
                CliResult::Success(format_process_list(processes))
            }
            DaemonResponse::ProcessLogs(logs) => CliResult::Success(format_logs(logs)),
            DaemonResponse::DaemonStatus(status) => {
                CliResult::Success(format_daemon_status(status))
            }
        }
    }
}

fn format_environment_status(status: EnvironmentStatusResponse) -> String {
    let mut output = String::new();

    if let Some(env) = &status.active_environment {
        output.push_str(&format!("Active Environment: {env}\n"));
        if let Some(path) = &status.project_path {
            output.push_str(&format!("Project Path: {path}\n"));
        }
    } else {
        output.push_str("No active environment\n");
    }

    output.push_str(&format!(
        "Processes: {}/{} running\n",
        status.running_processes, status.total_processes
    ));

    if !status.processes.is_empty() {
        output.push('\n');
        output.push_str(&format_process_list(status.processes));
    }

    output
}

fn format_process_list(processes: Vec<ProcessInfo>) -> String {
    if processes.is_empty() {
        return "No processes running".to_string();
    }

    let mut output = String::new();
    output.push_str("PROCESS         STATUS    PID      UPTIME      ENVIRONMENT\n");
    output.push_str("-------         ------    ---      ------      -----------\n");

    for process in processes {
        output.push_str(&format!(
            "{:<15} {:<9} {:<8} {:<11} {}\n",
            process.name,
            process.status,
            process.pid.map_or("-".to_string(), |p| p.to_string()),
            process.uptime.as_deref().unwrap_or("-"),
            process.environment
        ));
    }

    output
}

fn format_logs(logs: LogsResponse) -> String {
    if logs.lines.is_empty() {
        return format!("No logs available for process '{}'", logs.process_name);
    }

    let mut output = String::new();
    output.push_str(&format!("==> Logs for {} <==\n", logs.process_name));

    let lines_len = logs.lines.len();
    for line in logs.lines {
        output.push_str(&format!(
            "[{}] {}: {}\n",
            line.timestamp, line.level, line.message
        ));
    }

    if logs.total_lines > lines_len {
        output.push_str(&format!(
            "\n... {} more lines available (use --lines to see more)\n",
            logs.total_lines - lines_len
        ));
    }

    output
}

fn format_daemon_status(status: DaemonStatusResponse) -> String {
    let mut output = String::new();

    output.push_str(&format!(
        "Daemon Status: {}\n",
        if status.running { "Running" } else { "Stopped" }
    ));
    output.push_str(&format!("Version: {}\n", status.version));
    output.push_str(&format!("Uptime: {}\n", status.uptime));
    output.push_str(&format!("Socket: {}\n", status.socket_path));
    output.push_str(&format!(
        "Active Environments: {}\n",
        status.active_environments
    ));
    output.push_str(&format!("Total Processes: {}\n", status.total_processes));

    if let Some(memory) = status.memory_usage {
        output.push_str(&format!("Memory Usage: {memory}\n"));
    }

    output
}
