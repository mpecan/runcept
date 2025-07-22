use crate::database::process_repository::ProcessRecord;
use crate::process::{Process, ProcessStatus};
use chrono::Utc;
use std::collections::HashMap;

/// Builder for creating test Process instances
pub struct ProcessBuilder {
    process: Process,
}

impl ProcessBuilder {
    pub fn new(name: &str) -> Self {
        let environment_id = "test-env".to_string();
        let process_id = format!("{environment_id}:{name}");

        Self {
            process: Process {
                id: process_id,
                name: name.to_string(),
                command: "echo hello".to_string(),
                working_dir: "/tmp".to_string(),
                environment_id,
                pid: None,
                status: ProcessStatus::Stopped,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                last_activity: None,
                auto_restart: false,
                health_check_url: None,
                health_check_interval: None,
                depends_on: Vec::new(),
                env_vars: HashMap::new(),
            },
        }
    }

    pub fn with_environment(mut self, environment_id: &str) -> Self {
        self.process.environment_id = environment_id.to_string();
        self.process.id = format!("{environment_id}:{}", self.process.name);
        self
    }

    pub fn with_command(mut self, command: &str) -> Self {
        self.process.command = command.to_string();
        self
    }

    pub fn with_working_dir(mut self, working_dir: &str) -> Self {
        self.process.working_dir = working_dir.to_string();
        self
    }

    pub fn with_status(mut self, status: ProcessStatus) -> Self {
        self.process.status = status;
        self
    }

    pub fn with_pid(mut self, pid: u32) -> Self {
        self.process.pid = Some(pid);
        self
    }

    pub fn with_auto_restart(mut self, auto_restart: bool) -> Self {
        self.process.auto_restart = auto_restart;
        self
    }

    pub fn with_health_check(mut self, url: &str, interval: u32) -> Self {
        self.process.health_check_url = Some(url.to_string());
        self.process.health_check_interval = Some(interval);
        self
    }

    pub fn with_env_var(mut self, key: &str, value: &str) -> Self {
        self.process
            .env_vars
            .insert(key.to_string(), value.to_string());
        self
    }

    pub fn build(self) -> Process {
        self.process
    }
}

/// Builder for creating test ProcessRecord instances
pub struct ProcessRecordBuilder {
    record: ProcessRecord,
}

impl ProcessRecordBuilder {
    pub fn new(name: &str) -> Self {
        let environment_id = "test-env".to_string();
        let process_id = format!("{environment_id}:{name}");

        Self {
            record: ProcessRecord {
                id: process_id,
                name: name.to_string(),
                command: "echo hello".to_string(),
                working_dir: "/tmp".to_string(),
                environment_id,
                status: "stopped".to_string(),
                pid: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        }
    }

    pub fn with_environment(mut self, environment_id: &str) -> Self {
        self.record.environment_id = environment_id.to_string();
        self.record.id = format!("{environment_id}:{}", self.record.name);
        self
    }

    pub fn with_command(mut self, command: &str) -> Self {
        self.record.command = command.to_string();
        self
    }

    pub fn with_working_dir(mut self, working_dir: &str) -> Self {
        self.record.working_dir = working_dir.to_string();
        self
    }

    pub fn with_status(mut self, status: &str) -> Self {
        self.record.status = status.to_string();
        self
    }

    pub fn with_pid(mut self, pid: i64) -> Self {
        self.record.pid = Some(pid);
        self
    }

    pub fn build(self) -> ProcessRecord {
        self.record
    }
}
