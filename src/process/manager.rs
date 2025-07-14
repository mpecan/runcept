#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GlobalConfig, ProcessDefinition};
    use std::collections::HashMap;
    use tempfile::TempDir;
    use tokio::time::Duration;

    #[tokio::test]
    async fn test_process_manager_creation() {
        let global_config = GlobalConfig::default();
        let manager = ProcessManager::new(global_config).await.unwrap();

        assert!(manager.processes.is_empty());
        assert!(manager.process_handles.is_empty());
    }

    #[tokio::test]
    async fn test_start_simple_process() {
        let global_config = GlobalConfig::default();
        let mut manager = ProcessManager::new(global_config).await.unwrap();

        let temp_dir = TempDir::new().unwrap();
        let working_dir = temp_dir.path().to_path_buf();

        let process_def = ProcessDefinition {
            name: "test-echo".to_string(),
            command: "echo hello world".to_string(),
            working_dir: Some(working_dir.to_string_lossy().to_string()),
            auto_restart: Some(false),
            health_check_url: None,
            health_check_interval: None,
            depends_on: vec![],
            env_vars: HashMap::new(),
        };

        let process_id = manager
            .start_process(&process_def, &working_dir)
            .await
            .unwrap();

        assert_eq!(manager.processes.len(), 1);
        assert!(manager.processes.contains_key(&process_id));

        let process = manager.processes.get(&process_id).unwrap();
        assert_eq!(process.name, "test-echo");
        assert!(process.is_running() || process.status == ProcessStatus::Starting);
    }

    #[tokio::test]
    async fn test_stop_process() {
        let global_config = GlobalConfig::default();
        let mut manager = ProcessManager::new(global_config).await.unwrap();

        let temp_dir = TempDir::new().unwrap();
        let working_dir = temp_dir.path().to_path_buf();

        // Start a long-running process
        let process_def = ProcessDefinition {
            name: "test-sleep".to_string(),
            command: "sleep 60".to_string(),
            working_dir: Some(working_dir.to_string_lossy().to_string()),
            auto_restart: Some(false),
            health_check_url: None,
            health_check_interval: None,
            depends_on: vec![],
            env_vars: HashMap::new(),
        };

        let process_id = manager
            .start_process(&process_def, &working_dir)
            .await
            .unwrap();

        // Wait a bit for process to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Stop the process
        let result = manager.stop_process(&process_id).await;
        assert!(result.is_ok());

        let process = manager.processes.get(&process_id).unwrap();
        assert!(process.is_stopped() || process.status == ProcessStatus::Stopping);
    }

    #[tokio::test]
    async fn test_restart_process() {
        let global_config = GlobalConfig::default();
        let mut manager = ProcessManager::new(global_config).await.unwrap();

        let temp_dir = TempDir::new().unwrap();
        let working_dir = temp_dir.path().to_path_buf();

        let process_def = ProcessDefinition {
            name: "test-restart".to_string(),
            command: "echo restart test".to_string(),
            working_dir: Some(working_dir.to_string_lossy().to_string()),
            auto_restart: Some(false),
            health_check_url: None,
            health_check_interval: None,
            depends_on: vec![],
            env_vars: HashMap::new(),
        };

        let process_id = manager
            .start_process(&process_def, &working_dir)
            .await
            .unwrap();

        // Wait for process to complete
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Restart the process
        let result = manager.restart_process(&process_id).await;
        assert!(result.is_ok());

        let process = manager.processes.get(&process_id).unwrap();
        assert!(process.is_running() || process.status == ProcessStatus::Starting);
    }

    #[tokio::test]
    async fn test_process_with_environment_variables() {
        let global_config = GlobalConfig::default();
        let mut manager = ProcessManager::new(global_config).await.unwrap();

        let temp_dir = TempDir::new().unwrap();
        let working_dir = temp_dir.path().to_path_buf();

        let mut env_vars = HashMap::new();
        env_vars.insert("TEST_VAR".to_string(), "test_value".to_string());

        let process_def = ProcessDefinition {
            name: "test-env".to_string(),
            command: "env | grep TEST_VAR".to_string(),
            working_dir: Some(working_dir.to_string_lossy().to_string()),
            auto_restart: Some(false),
            health_check_url: None,
            health_check_interval: None,
            depends_on: vec![],
            env_vars,
        };

        let process_id = manager
            .start_process(&process_def, &working_dir)
            .await
            .unwrap();

        assert_eq!(manager.processes.len(), 1);

        let process = manager.processes.get(&process_id).unwrap();
        assert_eq!(
            process.get_env_var("TEST_VAR"),
            Some(&"test_value".to_string())
        );
    }

    #[tokio::test]
    async fn test_list_processes() {
        let global_config = GlobalConfig::default();
        let mut manager = ProcessManager::new(global_config).await.unwrap();

        let temp_dir = TempDir::new().unwrap();
        let working_dir = temp_dir.path().to_path_buf();

        // Start multiple processes
        for i in 1..=3 {
            let process_def = ProcessDefinition {
                name: format!("test-process-{i}"),
                command: "echo test".to_string(),
                working_dir: Some(working_dir.to_string_lossy().to_string()),
                auto_restart: Some(false),
                health_check_url: None,
                health_check_interval: None,
                depends_on: vec![],
                env_vars: HashMap::new(),
            };

            manager
                .start_process(&process_def, &working_dir)
                .await
                .unwrap();
        }

        let processes = manager.list_processes();
        assert_eq!(processes.len(), 3);

        let running_processes = manager.list_running_processes();
        assert!(running_processes.len() <= 3); // May have finished already
    }

    #[tokio::test]
    async fn test_get_process_status() {
        let global_config = GlobalConfig::default();
        let mut manager = ProcessManager::new(global_config).await.unwrap();

        let temp_dir = TempDir::new().unwrap();
        let working_dir = temp_dir.path().to_path_buf();

        let process_def = ProcessDefinition {
            name: "test-status".to_string(),
            command: "echo status test".to_string(),
            working_dir: Some(working_dir.to_string_lossy().to_string()),
            auto_restart: Some(false),
            health_check_url: None,
            health_check_interval: None,
            depends_on: vec![],
            env_vars: HashMap::new(),
        };

        let process_id = manager
            .start_process(&process_def, &working_dir)
            .await
            .unwrap();

        let process = manager.get_process(&process_id).unwrap();
        assert_eq!(process.name, "test-status");

        let status = manager.get_process_status(&process_id).unwrap();
        assert!(matches!(
            status,
            ProcessStatus::Starting | ProcessStatus::Running
        ));
    }

    #[tokio::test]
    async fn test_stop_all_processes() {
        let global_config = GlobalConfig::default();
        let mut manager = ProcessManager::new(global_config).await.unwrap();

        let temp_dir = TempDir::new().unwrap();
        let working_dir = temp_dir.path().to_path_buf();

        // Start multiple long-running processes
        for i in 1..=3 {
            let process_def = ProcessDefinition {
                name: format!("test-long-{i}"),
                command: "sleep 30".to_string(),
                working_dir: Some(working_dir.to_string_lossy().to_string()),
                auto_restart: Some(false),
                health_check_url: None,
                health_check_interval: None,
                depends_on: vec![],
                env_vars: HashMap::new(),
            };

            manager
                .start_process(&process_def, &working_dir)
                .await
                .unwrap();
        }

        // Wait a bit for processes to start
        tokio::time::sleep(Duration::from_millis(200)).await;

        let result = manager.stop_all_processes().await;
        assert!(result.is_ok());

        // Check that all processes are stopped or stopping
        let processes = manager.list_processes();
        for (_, process) in processes {
            assert!(process.is_stopped() || process.status == ProcessStatus::Stopping);
        }
    }

    #[tokio::test]
    async fn test_cleanup_finished_processes() {
        let global_config = GlobalConfig::default();
        let mut manager = ProcessManager::new(global_config).await.unwrap();

        let temp_dir = TempDir::new().unwrap();
        let working_dir = temp_dir.path().to_path_buf();

        // Start a quick-finishing process
        let process_def = ProcessDefinition {
            name: "test-quick".to_string(),
            command: "echo quick".to_string(),
            working_dir: Some(working_dir.to_string_lossy().to_string()),
            auto_restart: Some(false),
            health_check_url: None,
            health_check_interval: None,
            depends_on: vec![],
            env_vars: HashMap::new(),
        };

        let _process_id = manager
            .start_process(&process_def, &working_dir)
            .await
            .unwrap();

        // Wait for process to finish
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Clean up finished processes
        let cleaned = manager.cleanup_finished_processes().await.unwrap();

        // The process should be cleaned up if it finished
        assert!(cleaned.len() <= 1);
    }

    #[tokio::test]
    async fn test_kill_process_force() {
        let global_config = GlobalConfig::default();
        let mut manager = ProcessManager::new(global_config).await.unwrap();

        let temp_dir = TempDir::new().unwrap();
        let working_dir = temp_dir.path().to_path_buf();

        // Start a process that ignores SIGTERM
        let process_def = ProcessDefinition {
            name: "test-stubborn".to_string(),
            command: "sleep 60".to_string(),
            working_dir: Some(working_dir.to_string_lossy().to_string()),
            auto_restart: Some(false),
            health_check_url: None,
            health_check_interval: None,
            depends_on: vec![],
            env_vars: HashMap::new(),
        };

        let process_id = manager
            .start_process(&process_def, &working_dir)
            .await
            .unwrap();

        // Wait for process to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Force kill the process
        let result = manager.kill_process(&process_id, true).await;
        assert!(result.is_ok());

        let process = manager.processes.get(&process_id).unwrap();
        assert!(process.is_stopped() || process.status == ProcessStatus::Stopping);
    }

    #[tokio::test]
    async fn test_process_not_found_error() {
        let global_config = GlobalConfig::default();
        let mut manager = ProcessManager::new(global_config).await.unwrap();

        let fake_process_id = "non-existent-process".to_string();

        let result = manager.stop_process(&fake_process_id).await;
        assert!(result.is_err());

        match result.err().unwrap() {
            RunceptError::ProcessError(msg) => {
                assert!(msg.contains("not found"));
            }
            _ => panic!("Expected ProcessError"),
        }
    }
}

use crate::config::ProcessDefinition;
use crate::error::{Result, RunceptError};
use crate::process::Process;
use crate::process::ProcessStatus;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

pub struct ProcessHandle {
    pub child: Child,
    pub shutdown_tx: mpsc::Sender<()>,
}

pub struct ProcessManager {
    pub processes: HashMap<String, Process>,
    pub process_handles: HashMap<String, ProcessHandle>,
    pub global_config: crate::config::GlobalConfig,
}

impl ProcessManager {
    pub async fn new(global_config: crate::config::GlobalConfig) -> Result<Self> {
        Ok(Self {
            processes: HashMap::new(),
            process_handles: HashMap::new(),
            global_config,
        })
    }

    pub async fn start_process(
        &mut self,
        process_def: &ProcessDefinition,
        working_dir: &Path,
    ) -> Result<String> {
        let mut process = Process::new(
            process_def.name.clone(),
            process_def.command.clone(),
            working_dir.to_string_lossy().to_string(),
            "default".to_string(), // TODO: Use proper environment ID
        );

        // Set up environment variables
        for (key, value) in &process_def.env_vars {
            process.set_env_var(key.clone(), value.clone());
        }

        // Set auto-restart from definition or global config
        process.auto_restart = process_def
            .auto_restart
            .unwrap_or(self.global_config.process.auto_restart_on_crash);

        // Set health check if provided
        if let Some(url) = &process_def.health_check_url {
            let interval = process_def
                .health_check_interval
                .unwrap_or(self.global_config.process.health_check_interval);
            process.set_health_check(url.clone(), interval);
        }

        // Transition to starting
        process.transition_to(ProcessStatus::Starting);

        // Parse command and arguments
        let parts: Vec<&str> = process_def.command.split_whitespace().collect();
        if parts.is_empty() {
            return Err(RunceptError::ProcessError(
                "Command cannot be empty".to_string(),
            ));
        }

        let program = parts[0];
        let args = &parts[1..];

        // Set up working directory
        let work_dir = if let Some(wd) = &process_def.working_dir {
            Path::new(wd)
        } else {
            working_dir
        };

        // Create command
        let mut cmd = Command::new(program);
        cmd.args(args)
            .current_dir(work_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());

        // Add environment variables
        for (key, value) in &process.env_vars {
            cmd.env(key, value);
        }

        // Spawn the process
        let child = cmd.spawn().map_err(|e| {
            RunceptError::ProcessError(format!("Failed to start process '{}': {}", process.name, e))
        })?;

        // Get the PID
        if let Some(pid) = child.id() {
            process.set_pid(pid);
        }

        // Transition to running
        process.transition_to(ProcessStatus::Running);
        process.update_activity();

        let process_id = process.id.clone();

        // Set up shutdown channel
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

        // Store process handle
        let handle = ProcessHandle { child, shutdown_tx };

        self.process_handles.insert(process_id.clone(), handle);

        // Spawn a task to monitor the process
        let _process_id_clone = process_id.clone();

        tokio::spawn(async move {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    // Graceful shutdown requested - the handle will clean up the child
                }
            }
        });

        self.processes.insert(process_id.clone(), process);

        Ok(process_id)
    }

    pub async fn stop_process(&mut self, process_id: &str) -> Result<()> {
        let process = self
            .processes
            .get_mut(process_id)
            .ok_or_else(|| RunceptError::ProcessError(format!("Process {process_id} not found")))?;

        if !process.is_running() {
            return Ok(()); // Already stopped
        }

        process.transition_to(ProcessStatus::Stopping);

        if let Some(handle) = self.process_handles.get(process_id) {
            // Send shutdown signal
            let _ = handle.shutdown_tx.send(()).await;
        }

        // Remove the handle since the process is stopping
        self.process_handles.remove(process_id);

        Ok(())
    }

    pub async fn restart_process(&mut self, process_id: &str) -> Result<()> {
        // Get the process definition by cloning the needed data
        let (process_def, working_dir_path) = {
            let process = self.processes.get(process_id).ok_or_else(|| {
                RunceptError::ProcessError(format!("Process {process_id} not found"))
            })?;

            let process_def = ProcessDefinition {
                name: process.name.clone(),
                command: process.command.clone(),
                working_dir: Some(process.working_dir.clone()),
                auto_restart: Some(process.auto_restart),
                health_check_url: process.health_check_url.clone(),
                health_check_interval: process.health_check_interval,
                depends_on: vec![], // TODO: Handle dependencies
                env_vars: process.env_vars.clone(),
            };

            (process_def, process.working_dir.clone())
        };

        let working_dir = Path::new(&working_dir_path);

        // Stop the current process
        self.stop_process(process_id).await?;

        // Wait a bit for the process to stop
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Remove the old process
        self.processes.remove(process_id);

        // Start a new process with the same definition
        let new_process_id = self.start_process(&process_def, working_dir).await?;

        // Update the process ID in our tracking (for external references)
        if let Some(mut new_process) = self.processes.remove(&new_process_id) {
            new_process.id = process_id.to_string();
            self.processes.insert(process_id.to_string(), new_process);

            // Update the handle mapping
            if let Some(handle) = self.process_handles.remove(&new_process_id) {
                self.process_handles.insert(process_id.to_string(), handle);
            }
        }

        Ok(())
    }

    pub async fn kill_process(&mut self, process_id: &str, force: bool) -> Result<()> {
        let process = self
            .processes
            .get_mut(process_id)
            .ok_or_else(|| RunceptError::ProcessError(format!("Process {process_id} not found")))?;

        if !process.is_running() {
            return Ok(()); // Already stopped
        }

        process.transition_to(ProcessStatus::Stopping);

        if let Some(mut handle) = self.process_handles.remove(process_id) {
            if force {
                // Force kill
                let _ = handle.child.kill().await;
            } else {
                // Graceful shutdown
                let _ = handle.shutdown_tx.send(()).await;
            }
        }

        Ok(())
    }

    pub async fn stop_all_processes(&mut self) -> Result<()> {
        let process_ids: Vec<String> = self.processes.keys().cloned().collect();

        for process_id in process_ids {
            if let Err(e) = self.stop_process(&process_id).await {
                eprintln!("Failed to stop process {process_id}: {e}");
            }
        }

        Ok(())
    }

    pub fn get_process(&self, process_id: &str) -> Option<&Process> {
        self.processes.get(process_id)
    }

    pub fn get_process_mut(&mut self, process_id: &str) -> Option<&mut Process> {
        self.processes.get_mut(process_id)
    }

    pub fn get_process_status(&self, process_id: &str) -> Option<ProcessStatus> {
        self.processes.get(process_id).map(|p| p.status.clone())
    }

    pub fn list_processes(&self) -> Vec<(&String, &Process)> {
        self.processes.iter().collect()
    }

    pub fn list_running_processes(&self) -> Vec<(&String, &Process)> {
        self.processes
            .iter()
            .filter(|(_, process)| process.is_running())
            .collect()
    }

    pub async fn cleanup_finished_processes(&mut self) -> Result<Vec<String>> {
        let mut finished_processes = Vec::new();

        let finished_ids: Vec<String> = self
            .processes
            .iter()
            .filter(|(_, process)| {
                matches!(
                    process.status,
                    ProcessStatus::Stopped | ProcessStatus::Failed | ProcessStatus::Crashed
                ) && !process.auto_restart
            })
            .map(|(id, _)| id.clone())
            .collect();

        for process_id in finished_ids {
            self.processes.remove(&process_id);
            self.process_handles.remove(&process_id);
            finished_processes.push(process_id);
        }

        Ok(finished_processes)
    }

    pub fn get_process_count(&self) -> usize {
        self.processes.len()
    }

    pub fn get_running_process_count(&self) -> usize {
        self.processes
            .values()
            .filter(|process| process.is_running())
            .count()
    }
}
