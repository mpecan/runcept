use tracing::{debug, error, info, warn};

/// Service for managing process groups and termination
pub struct ProcessGroupManager;

impl ProcessGroupManager {
    /// Create a new ProcessGroupManager
    pub fn new() -> Self {
        Self
    }

    /// Kill an entire process group using system calls
    pub async fn kill_process_group(&self, pid: i32, process_name: &str, environment_id: &str) -> bool {
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
            processes_to_kill.len(), process_name, processes_to_kill
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
            if system.process(sysinfo::Pid::from_u32(target_pid as u32)).is_some() {
                remaining_processes.push(target_pid);
            }
        }

        if remaining_processes.is_empty() {
            info!(
                "Successfully killed all {} processes for '{}' in environment '{}'",
                processes_to_kill.len(), process_name, environment_id
            );
            true
        } else {
            error!(
                "Failed to kill {} processes for '{}': {:?}",
                remaining_processes.len(), process_name, remaining_processes
            );
            false
        }
    }

    /// Recursively find all child processes of a given PID
    fn find_all_child_processes(&self, parent_pid: i32, system: &sysinfo::System, result: &mut Vec<i32>) {
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