#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_creation() {
        let process = Process::new(
            "test-process".to_string(),
            "echo hello".to_string(),
            "/tmp".to_string(),
            "env-123".to_string(),
        );

        assert_eq!(process.name, "test-process");
        assert_eq!(process.command, "echo hello");
        assert_eq!(process.working_dir, "/tmp");
        assert_eq!(process.environment_id, "env-123");
        assert_eq!(process.status, ProcessStatus::Stopped);
        assert!(process.pid.is_none());
        assert!(!process.auto_restart);
        assert!(process.health_check_url.is_none());
        assert!(process.depends_on.is_empty());
        assert!(process.env_vars.is_empty());
    }

    #[test]
    fn test_process_status_transitions() {
        let mut process = Process::new(
            "test".to_string(),
            "echo test".to_string(),
            "/tmp".to_string(),
            "env-1".to_string(),
        );

        // Test starting
        assert!(process.can_transition_to(&ProcessStatus::Starting));
        process.transition_to(ProcessStatus::Starting);
        assert_eq!(process.status, ProcessStatus::Starting);

        // Test running
        assert!(process.can_transition_to(&ProcessStatus::Running));
        process.transition_to(ProcessStatus::Running);
        assert_eq!(process.status, ProcessStatus::Running);

        // Test stopping
        assert!(process.can_transition_to(&ProcessStatus::Stopping));
        process.transition_to(ProcessStatus::Stopping);
        assert_eq!(process.status, ProcessStatus::Stopping);

        // Test stopped
        assert!(process.can_transition_to(&ProcessStatus::Stopped));
        process.transition_to(ProcessStatus::Stopped);
        assert_eq!(process.status, ProcessStatus::Stopped);
    }

    #[test]
    fn test_invalid_status_transitions() {
        let process = Process::new(
            "test".to_string(),
            "echo test".to_string(),
            "/tmp".to_string(),
            "env-1".to_string(),
        );

        // Can't go from Stopped to Running directly
        assert!(!process.can_transition_to(&ProcessStatus::Running));
        
        // Can't go from Stopped to Stopping
        assert!(!process.can_transition_to(&ProcessStatus::Stopping));
    }

    #[test]
    fn test_process_with_dependencies() {
        let mut process = Process::new(
            "web-server".to_string(),
            "python server.py".to_string(),
            "/app".to_string(),
            "env-1".to_string(),
        );

        process.add_dependency("database".to_string());
        process.add_dependency("redis".to_string());

        assert_eq!(process.depends_on.len(), 2);
        assert!(process.depends_on.contains(&"database".to_string()));
        assert!(process.depends_on.contains(&"redis".to_string()));

        assert!(process.has_dependency("database"));
        assert!(process.has_dependency("redis"));
        assert!(!process.has_dependency("cache"));
    }

    #[test]
    fn test_process_environment_variables() {
        let mut process = Process::new(
            "app".to_string(),
            "node server.js".to_string(),
            "/app".to_string(),
            "env-1".to_string(),
        );

        process.set_env_var("NODE_ENV".to_string(), "production".to_string());
        process.set_env_var("PORT".to_string(), "3000".to_string());

        assert_eq!(process.env_vars.len(), 2);
        assert_eq!(process.get_env_var("NODE_ENV"), Some(&"production".to_string()));
        assert_eq!(process.get_env_var("PORT"), Some(&"3000".to_string()));
        assert_eq!(process.get_env_var("DEBUG"), None);
    }

    #[test]
    fn test_process_health_check() {
        let mut process = Process::new(
            "web".to_string(),
            "python app.py".to_string(),
            "/app".to_string(),
            "env-1".to_string(),
        );

        process.set_health_check("http://localhost:8000/health".to_string(), 30);
        
        assert_eq!(process.health_check_url, Some("http://localhost:8000/health".to_string()));
        assert_eq!(process.health_check_interval, Some(30));
    }

    #[test]
    fn test_process_serialization() {
        let mut process = Process::new(
            "test-app".to_string(),
            "cargo run".to_string(),
            "/project".to_string(),
            "dev-env".to_string(),
        );

        process.set_env_var("RUST_LOG".to_string(), "debug".to_string());
        process.add_dependency("database".to_string());
        process.auto_restart = true;

        // Test serialization
        let json = serde_json::to_string(&process).unwrap();
        assert!(json.contains("test-app"));
        assert!(json.contains("cargo run"));

        // Test deserialization
        let deserialized: Process = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, process.name);
        assert_eq!(deserialized.command, process.command);
        assert_eq!(deserialized.auto_restart, process.auto_restart);
        assert_eq!(deserialized.env_vars, process.env_vars);
        assert_eq!(deserialized.depends_on, process.depends_on);
    }

    #[test]
    fn test_process_status_display() {
        assert_eq!(ProcessStatus::Stopped.to_string(), "stopped");
        assert_eq!(ProcessStatus::Starting.to_string(), "starting");
        assert_eq!(ProcessStatus::Running.to_string(), "running");
        assert_eq!(ProcessStatus::Stopping.to_string(), "stopping");
        assert_eq!(ProcessStatus::Failed.to_string(), "failed");
        assert_eq!(ProcessStatus::Crashed.to_string(), "crashed");
    }

    #[test]
    fn test_process_update_activity() {
        let mut process = Process::new(
            "test".to_string(),
            "echo test".to_string(),
            "/tmp".to_string(),
            "env-1".to_string(),
        );

        let before = process.last_activity;
        std::thread::sleep(std::time::Duration::from_millis(10));
        
        process.update_activity();
        
        assert!(process.last_activity.is_some());
        assert!(process.last_activity > before);
    }

    #[test]
    fn test_process_is_healthy_status() {
        let mut process = Process::new(
            "test".to_string(),
            "echo test".to_string(),
            "/tmp".to_string(),
            "env-1".to_string(),
        );

        // Stopped process is considered healthy
        assert!(process.is_healthy_status());

        process.transition_to(ProcessStatus::Running);
        assert!(process.is_healthy_status());

        process.transition_to(ProcessStatus::Failed);
        assert!(!process.is_healthy_status());

        process.transition_to(ProcessStatus::Crashed);
        assert!(!process.is_healthy_status());
    }
}

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProcessStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
    Crashed,
}

impl fmt::Display for ProcessStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status_str = match self {
            ProcessStatus::Stopped => "stopped",
            ProcessStatus::Starting => "starting", 
            ProcessStatus::Running => "running",
            ProcessStatus::Stopping => "stopping",
            ProcessStatus::Failed => "failed",
            ProcessStatus::Crashed => "crashed",
        };
        write!(f, "{}", status_str)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Process {
    pub id: String,
    pub name: String,
    pub command: String,
    pub working_dir: String,
    pub environment_id: String,
    pub pid: Option<u32>,
    pub status: ProcessStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_activity: Option<DateTime<Utc>>,
    pub auto_restart: bool,
    pub health_check_url: Option<String>,
    pub health_check_interval: Option<u32>, // seconds
    pub depends_on: Vec<String>,
    pub env_vars: HashMap<String, String>,
}

impl Process {
    pub fn new(
        name: String,
        command: String,
        working_dir: String,
        environment_id: String,
    ) -> Self {
        let now = Utc::now();
        
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            command,
            working_dir,
            environment_id,
            pid: None,
            status: ProcessStatus::Stopped,
            created_at: now,
            updated_at: now,
            last_activity: None,
            auto_restart: false,
            health_check_url: None,
            health_check_interval: None,
            depends_on: Vec::new(),
            env_vars: HashMap::new(),
        }
    }

    pub fn transition_to(&mut self, new_status: ProcessStatus) {
        if self.can_transition_to(&new_status) {
            self.status = new_status;
            self.updated_at = Utc::now();
        }
    }

    pub fn can_transition_to(&self, new_status: &ProcessStatus) -> bool {
        use ProcessStatus::*;
        
        match (&self.status, new_status) {
            // From Stopped
            (Stopped, Starting) => true,
            (Stopped, Failed) => true,
            
            // From Starting  
            (Starting, Running) => true,
            (Starting, Failed) => true,
            (Starting, Crashed) => true,
            
            // From Running
            (Running, Stopping) => true,
            (Running, Crashed) => true,
            (Running, Failed) => true,
            
            // From Stopping
            (Stopping, Stopped) => true,
            (Stopping, Failed) => true,
            
            // From Failed
            (Failed, Starting) => true,
            (Failed, Stopped) => true,
            
            // From Crashed
            (Crashed, Starting) => true,
            (Crashed, Stopped) => true,
            
            // Same status
            (status, new_status) if status == new_status => true,
            
            // All other transitions are invalid
            _ => false,
        }
    }

    pub fn add_dependency(&mut self, dependency: String) {
        if !self.depends_on.contains(&dependency) {
            self.depends_on.push(dependency);
            self.updated_at = Utc::now();
        }
    }

    pub fn has_dependency(&self, dependency: &str) -> bool {
        self.depends_on.iter().any(|dep| dep == dependency)
    }

    pub fn set_env_var(&mut self, key: String, value: String) {
        self.env_vars.insert(key, value);
        self.updated_at = Utc::now();
    }

    pub fn get_env_var(&self, key: &str) -> Option<&String> {
        self.env_vars.get(key)
    }

    pub fn set_health_check(&mut self, url: String, interval_seconds: u32) {
        self.health_check_url = Some(url);
        self.health_check_interval = Some(interval_seconds);
        self.updated_at = Utc::now();
    }

    pub fn update_activity(&mut self) {
        self.last_activity = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    pub fn is_healthy_status(&self) -> bool {
        !matches!(self.status, ProcessStatus::Failed | ProcessStatus::Crashed)
    }

    pub fn is_running(&self) -> bool {
        matches!(self.status, ProcessStatus::Running)
    }

    pub fn is_stopped(&self) -> bool {
        matches!(self.status, ProcessStatus::Stopped)
    }

    pub fn is_transitioning(&self) -> bool {
        matches!(self.status, ProcessStatus::Starting | ProcessStatus::Stopping)
    }

    pub fn set_pid(&mut self, pid: u32) {
        self.pid = Some(pid);
        self.updated_at = Utc::now();
    }

    pub fn clear_pid(&mut self) {
        self.pid = None;
        self.updated_at = Utc::now();
    }
}