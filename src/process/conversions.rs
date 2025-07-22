use crate::cli::commands::ProcessInfo;
use crate::config::ProcessDefinition;
use crate::database::process_repository::ProcessRecord;
use crate::process::{Process, ProcessStatus};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Formal conversion traits for Process-related types
///
/// This module consolidates all conversions between Process, ProcessRecord,
/// ProcessInfo, and ProcessDefinition to eliminate code duplication and
/// ensure consistent field mapping.

// ProcessDefinition → Process conversions

impl Process {
    /// Create a Process from a ProcessDefinition with additional context
    pub fn from_definition_with_context(
        definition: ProcessDefinition,
        environment_id: String,
        working_dir: Option<String>,
    ) -> Self {
        let now = Utc::now();
        let process_id = format!("{}:{}", environment_id, definition.name);

        Self {
            id: process_id,
            name: definition.name,
            command: definition.command,
            working_dir: working_dir
                .or(definition.working_dir)
                .unwrap_or_else(|| "/tmp".to_string()),
            environment_id,
            pid: None,
            status: ProcessStatus::Stopped,
            created_at: now,
            updated_at: now,
            last_activity: None,
            auto_restart: definition.auto_restart.unwrap_or(false),
            health_check_url: definition.health_check_url,
            health_check_interval: definition.health_check_interval,
            depends_on: definition.depends_on,
            env_vars: definition.env_vars,
        }
    }
}

// Process → ProcessRecord conversions

impl From<&Process> for ProcessRecord {
    fn from(process: &Process) -> Self {
        Self {
            id: process.id.clone(),
            name: process.name.clone(),
            command: process.command.clone(),
            working_dir: process.working_dir.clone(),
            environment_id: process.environment_id.clone(),
            status: process.status.to_string(),
            pid: process.pid.map(|p| p as i64),
            created_at: process.created_at,
            updated_at: process.updated_at,
        }
    }
}

impl From<Process> for ProcessRecord {
    fn from(process: Process) -> Self {
        (&process).into()
    }
}

// ProcessRecord → Process conversions

impl TryFrom<ProcessRecord> for Process {
    type Error = crate::error::RunceptError;

    fn try_from(record: ProcessRecord) -> Result<Self, Self::Error> {
        let status = ProcessStatus::try_from(record.status.as_str())?;

        Ok(Self {
            id: record.id,
            name: record.name,
            command: record.command,
            working_dir: record.working_dir,
            environment_id: record.environment_id,
            pid: record.pid.map(|p| p as u32),
            status,
            created_at: record.created_at,
            updated_at: record.updated_at,
            last_activity: None, // Not stored in ProcessRecord, would need separate query
            auto_restart: false, // Not stored in ProcessRecord, would need separate query
            health_check_url: None, // Not stored in ProcessRecord, would need separate query
            health_check_interval: None, // Not stored in ProcessRecord, would need separate query
            depends_on: Vec::new(), // Not stored in ProcessRecord, would need separate query
            env_vars: HashMap::new(), // Not stored in ProcessRecord, would need separate query
        })
    }
}

// ProcessStatus string conversions

impl TryFrom<&str> for ProcessStatus {
    type Error = crate::error::RunceptError;

    fn try_from(status: &str) -> Result<Self, Self::Error> {
        match status.to_lowercase().as_str() {
            "stopped" => Ok(ProcessStatus::Stopped),
            "starting" => Ok(ProcessStatus::Starting),
            "running" => Ok(ProcessStatus::Running),
            "stopping" => Ok(ProcessStatus::Stopping),
            "failed" => Ok(ProcessStatus::Failed),
            "crashed" => Ok(ProcessStatus::Crashed),
            _ => Err(crate::error::RunceptError::ProcessError(format!(
                "Invalid process status: {}",
                status
            ))),
        }
    }
}

// Process → ProcessInfo conversions (with computed fields)

/// Helper struct for computing ProcessInfo fields from Process and runtime data
pub struct ProcessInfoBuilder {
    process: Process,
    uptime: Option<String>,
    health_status: Option<String>,
    restart_count: u32,
}

impl ProcessInfoBuilder {
    pub fn from_process(process: Process) -> Self {
        Self {
            process,
            uptime: None,
            health_status: None,
            restart_count: 0,
        }
    }

    pub fn with_uptime(mut self, uptime: Option<String>) -> Self {
        self.uptime = uptime;
        self
    }

    pub fn with_health_status(mut self, health_status: Option<String>) -> Self {
        self.health_status = health_status;
        self
    }

    pub fn with_restart_count(mut self, restart_count: u32) -> Self {
        self.restart_count = restart_count;
        self
    }

    pub fn build(self) -> ProcessInfo {
        ProcessInfo {
            id: self.process.id,
            name: self.process.name,
            status: self.process.status.to_string(),
            pid: self.process.pid,
            uptime: self.uptime,
            environment: self.process.environment_id,
            health_status: self.health_status,
            restart_count: self.restart_count,
            last_activity: self.process.last_activity.map(|dt| dt.to_rfc3339()),
        }
    }
}

// Basic conversion for ProcessInfo when we don't have runtime data
impl From<Process> for ProcessInfo {
    fn from(process: Process) -> Self {
        ProcessInfoBuilder::from_process(process).build()
    }
}

impl From<&Process> for ProcessInfo {
    fn from(process: &Process) -> Self {
        ProcessInfoBuilder::from_process(process.clone()).build()
    }
}

// Utility functions for common conversions

/// Calculate uptime string from process start time
pub fn calculate_uptime(created_at: DateTime<Utc>, status: &ProcessStatus) -> Option<String> {
    if matches!(status, ProcessStatus::Running | ProcessStatus::Starting) {
        let duration = Utc::now() - created_at;
        let total_seconds = duration.num_seconds();

        if total_seconds < 60 {
            Some(format!("{}s", total_seconds))
        } else if total_seconds < 3600 {
            let minutes = total_seconds / 60;
            let seconds = total_seconds % 60;
            Some(format!("{}m {}s", minutes, seconds))
        } else {
            let hours = total_seconds / 3600;
            let minutes = (total_seconds % 3600) / 60;
            Some(format!("{}h {}m", hours, minutes))
        }
    } else {
        None
    }
}

/// Convert health check URL to a basic health status
pub fn derive_health_status(
    health_check_url: &Option<String>,
    status: &ProcessStatus,
) -> Option<String> {
    match (health_check_url, status) {
        (Some(_), ProcessStatus::Running) => Some("checking".to_string()),
        (Some(_), ProcessStatus::Failed) => Some("unhealthy".to_string()),
        (Some(_), _) => Some("unknown".to_string()),
        (None, _) => None,
    }
}

/// Enhanced ProcessInfo conversion with computed fields
pub fn process_to_info_with_runtime_data(
    process: Process,
    restart_count: u32,
    health_status: Option<String>,
) -> ProcessInfo {
    let uptime = calculate_uptime(process.created_at, &process.status);
    let computed_health_status =
        health_status.or_else(|| derive_health_status(&process.health_check_url, &process.status));

    ProcessInfoBuilder::from_process(process)
        .with_uptime(uptime)
        .with_health_status(computed_health_status)
        .with_restart_count(restart_count)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::collections::HashMap;

    fn create_test_process() -> Process {
        Process {
            id: "env1:test-process".to_string(),
            name: "test-process".to_string(),
            command: "echo hello".to_string(),
            working_dir: "/tmp".to_string(),
            environment_id: "env1".to_string(),
            pid: Some(1234),
            status: ProcessStatus::Running,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_activity: None,
            auto_restart: true,
            health_check_url: Some("http://localhost:8080/health".to_string()),
            health_check_interval: Some(30),
            depends_on: vec!["other-process".to_string()],
            env_vars: {
                let mut map = HashMap::new();
                map.insert("ENV_VAR".to_string(), "value".to_string());
                map
            },
        }
    }

    fn create_test_definition() -> ProcessDefinition {
        ProcessDefinition {
            name: "test-process".to_string(),
            command: "echo hello".to_string(),
            working_dir: Some("/tmp".to_string()),
            auto_restart: Some(true),
            health_check_url: Some("http://localhost:8080/health".to_string()),
            health_check_interval: Some(30),
            depends_on: vec!["other-process".to_string()],
            env_vars: {
                let mut map = HashMap::new();
                map.insert("ENV_VAR".to_string(), "value".to_string());
                map
            },
        }
    }

    #[test]
    fn test_process_from_definition_with_context() {
        let definition = create_test_definition();
        let process =
            Process::from_definition_with_context(definition.clone(), "env1".to_string(), None);

        assert_eq!(process.name, definition.name);
        assert_eq!(process.command, definition.command);
        assert_eq!(process.environment_id, "env1");
        assert_eq!(process.id, "env1:test-process");
        assert_eq!(process.status, ProcessStatus::Stopped);
        assert_eq!(process.auto_restart, true);
        assert_eq!(process.health_check_url, definition.health_check_url);
        assert_eq!(process.depends_on, definition.depends_on);
        assert_eq!(process.env_vars, definition.env_vars);
    }

    #[test]
    fn test_process_to_record_conversion() {
        let process = create_test_process();
        let record: ProcessRecord = (&process).into();

        assert_eq!(record.id, process.id);
        assert_eq!(record.name, process.name);
        assert_eq!(record.command, process.command);
        assert_eq!(record.environment_id, process.environment_id);
        assert_eq!(record.status, process.status.to_string());
        assert_eq!(record.pid, Some(1234i64));
    }

    #[test]
    fn test_record_to_process_conversion() {
        let record = ProcessRecord {
            id: "env1:test-process".to_string(),
            name: "test-process".to_string(),
            command: "echo hello".to_string(),
            working_dir: "/tmp".to_string(),
            environment_id: "env1".to_string(),
            status: "running".to_string(),
            pid: Some(1234i64),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let process: Process = record.try_into().unwrap();
        assert_eq!(process.status, ProcessStatus::Running);
        assert_eq!(process.pid, Some(1234u32));
    }

    #[test]
    fn test_process_status_string_conversion() {
        assert_eq!(
            ProcessStatus::try_from("running").unwrap(),
            ProcessStatus::Running
        );
        assert_eq!(
            ProcessStatus::try_from("STOPPED").unwrap(),
            ProcessStatus::Stopped
        );
        assert_eq!(
            ProcessStatus::try_from("Failed").unwrap(),
            ProcessStatus::Failed
        );

        assert!(ProcessStatus::try_from("invalid").is_err());
    }

    #[test]
    fn test_process_to_info_conversion() {
        let process = create_test_process();
        let info: ProcessInfo = process.clone().into();

        assert_eq!(info.id, process.id);
        assert_eq!(info.name, process.name);
        assert_eq!(info.status, process.status.to_string());
        assert_eq!(info.pid, process.pid);
        assert_eq!(info.environment, process.environment_id);
    }

    #[test]
    fn test_calculate_uptime() {
        let start_time = Utc::now() - chrono::Duration::seconds(125); // 2m 5s ago
        let uptime = calculate_uptime(start_time, &ProcessStatus::Running);
        assert!(uptime.unwrap().contains("2m"));

        let stopped_uptime = calculate_uptime(start_time, &ProcessStatus::Stopped);
        assert!(stopped_uptime.is_none());
    }

    #[test]
    fn test_process_info_builder() {
        let process = create_test_process();
        let info = ProcessInfoBuilder::from_process(process.clone())
            .with_uptime(Some("5m 30s".to_string()))
            .with_health_status(Some("healthy".to_string()))
            .with_restart_count(3)
            .build();

        assert_eq!(info.uptime, Some("5m 30s".to_string()));
        assert_eq!(info.health_status, Some("healthy".to_string()));
        assert_eq!(info.restart_count, 3);
    }

    #[test]
    fn test_derive_health_status() {
        let health_url = Some("http://localhost:8080/health".to_string());

        assert_eq!(
            derive_health_status(&health_url, &ProcessStatus::Running),
            Some("checking".to_string())
        );
        assert_eq!(
            derive_health_status(&health_url, &ProcessStatus::Failed),
            Some("unhealthy".to_string())
        );
        assert_eq!(derive_health_status(&None, &ProcessStatus::Running), None);
    }
}
