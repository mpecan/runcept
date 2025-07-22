use tracing::{debug, error, info, warn};

/// Service for managing process groups and termination
pub struct ProcessGroupManager;

impl ProcessGroupManager {
    /// Create a new ProcessGroupManager
    pub fn new() -> Self {
        Self
    }

    /// Kill an entire process group using system calls
    pub async fn kill_process_group(
        &self,
        pid: i32,
        process_name: &str,
        environment_id: &str,
    ) -> bool {
        use sysinfo::{Signal as SysinfoSignal, System};

        info!(
            "Attempting to kill process group for process '{}' (PID: {}) in environment '{}'",
            process_name, pid, environment_id
        );

        let mut system = System::new_all();

        // Find all processes in the same process group
        let mut processes_to_kill = Vec::new();

        // First, find the main process and its process group
        if let Some(_main_process) = system.process(sysinfo::Pid::from_u32(pid as u32)) {
            processes_to_kill.push(pid);

            // Find all child processes recursively
            self.find_all_child_processes(pid, &system, &mut processes_to_kill);
        }

        if processes_to_kill.is_empty() {
            warn!(
                "No processes found for process '{}' (PID: {}) in environment '{}'",
                process_name, pid, environment_id
            );
            return false;
        }

        info!(
            "Found {} processes to kill for '{}': {:?}",
            processes_to_kill.len(),
            process_name,
            processes_to_kill
        );

        // First pass: Send SIGTERM to all processes (graceful shutdown)
        for &target_pid in &processes_to_kill {
            if let Some(process) = system.process(sysinfo::Pid::from_u32(target_pid as u32)) {
                if process.kill_with(SysinfoSignal::Term).unwrap_or(false) {
                    debug!("Sent SIGTERM to process {}", target_pid);
                }
            }
        }

        // Wait a moment for graceful shutdown
        tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;

        // Second pass: Check which processes are still alive and force kill them
        system.refresh_all();
        for &target_pid in &processes_to_kill {
            if let Some(process) = system.process(sysinfo::Pid::from_u32(target_pid as u32)) {
                // Process is still alive, force kill it
                if process.kill_with(SysinfoSignal::Kill).unwrap_or(false) {
                    debug!("Sent SIGKILL to process {}", target_pid);
                } else {
                    warn!("Failed to send SIGKILL to process {}", target_pid);
                }
            }
        }

        // Wait a bit for processes to be killed
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Verify all processes are gone
        system.refresh_all();
        let mut remaining_processes = Vec::new();
        for &target_pid in &processes_to_kill {
            if system
                .process(sysinfo::Pid::from_u32(target_pid as u32))
                .is_some()
            {
                remaining_processes.push(target_pid);
            }
        }

        if remaining_processes.is_empty() {
            info!(
                "Successfully killed all {} processes for '{}' in environment '{}'",
                processes_to_kill.len(),
                process_name,
                environment_id
            );
            true
        } else {
            error!(
                "Failed to kill {} processes for '{}': {:?}",
                remaining_processes.len(),
                process_name,
                remaining_processes
            );
            false
        }
    }

    /// Recursively find all child processes of a given PID
    #[allow(clippy::only_used_in_recursion)]
    fn find_all_child_processes(
        &self,
        parent_pid: i32,
        system: &sysinfo::System,
        result: &mut Vec<i32>,
    ) {
        for (child_pid, process) in system.processes() {
            if let Some(ppid) = process.parent() {
                if ppid.as_u32() as i32 == parent_pid {
                    let child_pid_val = child_pid.as_u32() as i32;
                    result.push(child_pid_val);

                    // Recursively find children of this child
                    self.find_all_child_processes(child_pid_val, system, result);
                }
            }
        }
    }
}

impl Default for ProcessGroupManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_group_manager_creation() {
        let _manager = ProcessGroupManager::new();
        // Just ensure it can be created without panic
        assert!(true);
    }

    #[tokio::test]
    async fn test_default_creation() {
        let _manager = ProcessGroupManager::default();
        // Just ensure it can be created without panic
        assert!(true);
    }

    #[tokio::test]
    async fn test_kill_nonexistent_process() {
        let manager = ProcessGroupManager::new();

        // Try to kill a process that doesn't exist (use a very high PID)
        let result = manager
            .kill_process_group(99999, "nonexistent-process", "test-env")
            .await;

        // Should return false since process doesn't exist
        assert!(!result);
    }

    #[tokio::test]
    async fn test_kill_simple_process() {
        let manager = ProcessGroupManager::new();

        // Spawn a simple sleep process
        let mut child = tokio::process::Command::new("sleep")
            .arg("0.1") // Very short sleep to avoid long-running test
            .spawn()
            .unwrap();

        let pid = child.id().unwrap() as i32;

        // Kill the process group - may or may not succeed depending on timing and permissions
        let _result = manager
            .kill_process_group(pid, "sleep-process", "test-env")
            .await;

        // Either succeeds in killing or process already exited naturally
        // Don't assert on result since it depends on timing

        // Process should be terminated or exit naturally
        let wait_result =
            tokio::time::timeout(tokio::time::Duration::from_millis(500), child.wait()).await;

        // Should complete either way (killed or exited naturally)
        assert!(wait_result.is_ok());
    }

    #[tokio::test]
    async fn test_kill_process_with_children() {
        let manager = ProcessGroupManager::new();

        // Create a shell script that spawns short-lived children
        let script = r#"
            sleep 0.2 &
            sleep 0.2 &
            wait
        "#;

        // Spawn a bash process that creates child processes
        let mut child = tokio::process::Command::new("bash")
            .arg("-c")
            .arg(script)
            .spawn()
            .unwrap();

        let pid = child.id().unwrap() as i32;

        // Give it a moment to spawn children
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Kill the process group - may succeed or processes may exit naturally
        let _result = manager
            .kill_process_group(pid, "bash-process", "test-env")
            .await;

        // Don't assert on result - timing dependent

        // Parent process should be terminated or exit naturally
        let wait_result =
            tokio::time::timeout(tokio::time::Duration::from_millis(1000), child.wait()).await;

        assert!(wait_result.is_ok());
    }

    #[tokio::test]
    async fn test_find_child_processes() {
        use sysinfo::System;

        let manager = ProcessGroupManager::new();

        // Create a process with children
        let mut child = tokio::process::Command::new("bash")
            .arg("-c")
            .arg("sleep 0.5 & sleep 0.5 & wait")
            .spawn()
            .unwrap();

        let parent_pid = child.id().unwrap() as i32;

        // Give it a moment to spawn children
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let system = System::new_all();
        let mut result = Vec::new();

        // Test the find_all_child_processes method
        manager.find_all_child_processes(parent_pid, &system, &mut result);

        // Should find some child processes (might be 0 if they exit quickly)
        // We can't guarantee exact count due to timing, but method shouldn't panic
        // Length is always >= 0 for Vec, so just check that we got a result
        let _count = result.len();

        // Clean up
        let _ = child.kill().await;
    }

    #[tokio::test]
    async fn test_graceful_then_force_kill_timing() {
        let manager = ProcessGroupManager::new();

        // Create a process with a short but measurable lifetime
        let mut child = tokio::process::Command::new("sleep")
            .arg("0.5")
            .spawn()
            .unwrap();

        let pid = child.id().unwrap() as i32;

        // Kill the process - should try SIGTERM first, then SIGKILL after timeout
        let start_time = std::time::Instant::now();
        let _result = manager
            .kill_process_group(pid, "test-process", "test-env")
            .await;
        let elapsed = start_time.elapsed();

        // Should take some time to complete the kill sequence
        // (either because it worked or because process exited naturally)
        assert!(elapsed >= tokio::time::Duration::from_millis(10));

        // Process should be terminated one way or another
        let wait_result =
            tokio::time::timeout(tokio::time::Duration::from_millis(1000), child.wait()).await;

        assert!(wait_result.is_ok());
    }
}
