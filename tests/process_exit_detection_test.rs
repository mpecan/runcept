use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::time::Duration;
use tempfile::TempDir;

mod common;
use common::get_binary_path;

/// Test environment for process exit detection tests
struct ProcessExitTestEnvironment {
    temp_dir: TempDir,
    project_dir: PathBuf,
    home_dir: PathBuf,
    binary_path: PathBuf,
}

impl ProcessExitTestEnvironment {
    fn new() -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let project_dir = temp_dir.path().join("test_project");
        let home_dir = temp_dir.path().join("home");
        
        // Create project and home directories
        fs::create_dir_all(&project_dir).expect("Failed to create project dir");
        fs::create_dir_all(&home_dir).expect("Failed to create home dir");

        // Get the binary path using the common module
        let binary_path = get_binary_path("runcept");

        Self {
            temp_dir,
            project_dir,
            home_dir,
            binary_path,
        }
    }

    fn setup_test_project(&self) {
        let config_content = r#"
[environment]
name = "process-exit-test"
inactivity_timeout = "10m"

[processes.short_task]
name = "short_task"
command = "echo 'Hello World' && sleep 2"
working_dir = "."
auto_restart = false

[processes.long_task]
name = "long_task"
command = "sleep 30"
working_dir = "."
auto_restart = false
"#;

        let config_path = self.project_dir.join(".runcept.toml");
        fs::write(config_path, config_content).expect("Failed to write config file");
    }

    fn runcept_cmd(&self) -> Command {
        let mut cmd = Command::new(&self.binary_path);
        cmd.env("HOME", &self.home_dir);
        cmd.env("RUNCEPT_HOME", self.home_dir.join(".runcept"));
        cmd.current_dir(&self.project_dir);
        cmd
    }

    fn start_daemon(&self) -> Child {
        let mut cmd = std::process::Command::new(&self.binary_path);
        cmd.env("HOME", &self.home_dir);
        cmd.env("RUNCEPT_HOME", self.home_dir.join(".runcept"));
        cmd.current_dir(&self.project_dir);
        cmd.args(["daemon", "start"]);
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());
        cmd.spawn().expect("Failed to start daemon")
    }

    fn wait_for_daemon(&self) {
        // Wait for daemon to start
        for _ in 0..30 {
            if self.runcept_cmd().arg("status").output().is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("Daemon failed to start within timeout");
    }

    fn stop_daemon(&self) {
        let _ = self.runcept_cmd().args(["daemon", "stop"]).output();
        // Wait for daemon to stop
        std::thread::sleep(Duration::from_millis(500));
    }
}

impl Drop for ProcessExitTestEnvironment {
    fn drop(&mut self) {
        self.stop_daemon();
    }
}

#[cfg(test)]
mod process_exit_detection_tests {
    use super::*;

    #[test]
    fn test_short_lived_process_exit_detection() {
        let test_env = ProcessExitTestEnvironment::new();
        test_env.setup_test_project();

        // Start daemon
        let mut daemon_process = test_env.start_daemon();
        test_env.wait_for_daemon();

        // Activate environment
        test_env
            .runcept_cmd()
            .arg("activate")
            .assert()
            .success();

        // Start short-lived process
        test_env
            .runcept_cmd()
            .args(["start", "short_task"])
            .assert()
            .success()
            .stdout(predicate::str::contains("started").or(predicate::str::contains("Starting")));

        // Wait for process to complete (2 seconds + buffer)
        std::thread::sleep(Duration::from_millis(3000));

        // Check process list - should show process as stopped
        let output = test_env
            .runcept_cmd()
            .arg("list")
            .output()
            .expect("Failed to run list command");

        let list_output = String::from_utf8_lossy(&output.stdout);
        println!("Process list output: {}", list_output);

        // Process should be visible but marked as stopped
        assert!(
            list_output.contains("short_task"),
            "Process 'short_task' should be visible in process list. Output: {}",
            list_output
        );

        assert!(
            list_output.contains("stopped") || list_output.contains("configured"),
            "Process 'short_task' should be marked as stopped or configured. Output: {}",
            list_output
        );

        // Verify it's NOT marked as running
        assert!(
            !list_output.contains("running"),
            "Process should not be marked as running. Output: {}",
            list_output
        );

        // Clean up
        test_env.stop_daemon();
        let _ = daemon_process.wait();
    }

    #[test]
    fn test_long_lived_process_still_running() {
        let test_env = ProcessExitTestEnvironment::new();
        test_env.setup_test_project();

        // Start daemon
        let mut daemon_process = test_env.start_daemon();
        test_env.wait_for_daemon();

        // Activate environment
        test_env
            .runcept_cmd()
            .arg("activate")
            .assert()
            .success();

        // Start long-lived process
        test_env
            .runcept_cmd()
            .args(["start", "long_task"])
            .assert()
            .success()
            .stdout(predicate::str::contains("started").or(predicate::str::contains("Starting")));

        // Wait a bit but not long enough for process to complete
        std::thread::sleep(Duration::from_millis(1000));

        // Check process list - should show process as running
        let output = test_env
            .runcept_cmd()
            .arg("list")
            .output()
            .expect("Failed to run list command");

        let list_output = String::from_utf8_lossy(&output.stdout);
        println!("Process list output: {}", list_output);

        // Process should be visible and marked as running
        assert!(
            list_output.contains("long_task"),
            "Process 'long_task' should be visible in process list. Output: {}",
            list_output
        );

        assert!(
            list_output.contains("running"),
            "Process 'long_task' should be marked as running. Output: {}",
            list_output
        );

        // Stop the process manually
        test_env
            .runcept_cmd()
            .args(["stop", "long_task"])
            .assert()
            .success();

        // Wait for stop to complete
        std::thread::sleep(Duration::from_millis(1000));

        // Check process list again - should show process as stopped
        let output = test_env
            .runcept_cmd()
            .arg("list")
            .output()
            .expect("Failed to run list command");

        let list_output = String::from_utf8_lossy(&output.stdout);
        println!("Process list output after stop: {}", list_output);

        // Process should be visible but not running
        assert!(
            list_output.contains("long_task"),
            "Process 'long_task' should be visible in process list. Output: {}",
            list_output
        );

        assert!(
            !list_output.contains("running"),
            "Process should not be marked as running after stop. Output: {}",
            list_output
        );

        // Clean up
        test_env.stop_daemon();
        let _ = daemon_process.wait();
    }

    #[test]
    fn test_process_exit_detection_multiple_processes() {
        let test_env = ProcessExitTestEnvironment::new();
        test_env.setup_test_project();

        // Start daemon
        let mut daemon_process = test_env.start_daemon();
        test_env.wait_for_daemon();

        // Activate environment
        test_env
            .runcept_cmd()
            .arg("activate")
            .assert()
            .success();

        // Start both processes
        test_env
            .runcept_cmd()
            .args(["start", "short_task"])
            .assert()
            .success();

        test_env
            .runcept_cmd()
            .args(["start", "long_task"])
            .assert()
            .success();

        // Wait for short process to complete but not long process
        std::thread::sleep(Duration::from_millis(3000));

        // Check process list
        let output = test_env
            .runcept_cmd()
            .arg("list")
            .output()
            .expect("Failed to run list command");

        let list_output = String::from_utf8_lossy(&output.stdout);
        println!("Process list output: {}", list_output);

        // Both processes should be visible
        assert!(
            list_output.contains("short_task"),
            "Process 'short_task' should be visible. Output: {}",
            list_output
        );

        assert!(
            list_output.contains("long_task"),
            "Process 'long_task' should be visible. Output: {}",
            list_output
        );

        // Count running processes - should be 1 (long_task)
        let running_count = list_output.matches("running").count();
        assert_eq!(
            running_count, 1,
            "Should have exactly 1 running process. Output: {}",
            list_output
        );

        // Clean up
        test_env.stop_daemon();
        let _ = daemon_process.wait();
    }

    #[test]
    fn test_process_exit_detection_timing() {
        let test_env = ProcessExitTestEnvironment::new();
        test_env.setup_test_project();

        // Start daemon
        let mut daemon_process = test_env.start_daemon();
        test_env.wait_for_daemon();

        // Activate environment
        test_env
            .runcept_cmd()
            .arg("activate")
            .assert()
            .success();

        // Start short-lived process
        test_env
            .runcept_cmd()
            .args(["start", "short_task"])
            .assert()
            .success();

        // Check immediately - should be running
        let output = test_env
            .runcept_cmd()
            .arg("list")
            .output()
            .expect("Failed to run list command");

        let list_output = String::from_utf8_lossy(&output.stdout);
        println!("Immediate process list output: {}", list_output);

        assert!(
            list_output.contains("short_task"),
            "Process should be visible immediately. Output: {}",
            list_output
        );

        // Wait for process to complete
        std::thread::sleep(Duration::from_millis(3000));

        // Check multiple times to verify exit detection is working
        for attempt in 1..=5 {
            let output = test_env
                .runcept_cmd()
                .arg("list")
                .output()
                .expect("Failed to run list command");

            let list_output = String::from_utf8_lossy(&output.stdout);
            println!("Attempt {}: Process list output: {}", attempt, list_output);

            if !list_output.contains("running") {
                // Process is no longer running - this is what we want
                break;
            }

            if attempt == 5 {
                panic!(
                    "Process still showing as running after 5 attempts. Final output: {}",
                    list_output
                );
            }

            std::thread::sleep(Duration::from_millis(1000));
        }

        // Clean up
        test_env.stop_daemon();
        let _ = daemon_process.wait();
    }
}