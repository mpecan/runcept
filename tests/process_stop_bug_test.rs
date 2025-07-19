use assert_cmd::Command;
use predicates::prelude::*;
use std::process::Stdio;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

struct TestEnvironment {
    temp_dir: TempDir,
    project_dir: std::path::PathBuf,
    home_dir: std::path::PathBuf,
}

impl TestEnvironment {
    fn new() -> Self {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("test_project");
        let home_dir = temp_dir.path().join("home");

        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::create_dir_all(&home_dir).unwrap();

        Self {
            temp_dir,
            project_dir,
            home_dir,
        }
    }

    fn setup_test_project(&self) {
        let config_content = r#"
[environment]
name = "process-stop-test"
inactivity_timeout = "10m"

[processes.long_task]
name = "long_task"
command = "sleep 30"
working_dir = "."
auto_restart = false
"#;
        std::fs::write(self.project_dir.join(".runcept.toml"), config_content).unwrap();
    }

    fn runcept_cmd(&self) -> Command {
        let mut cmd = Command::cargo_bin("runcept").unwrap();
        cmd.env("HOME", &self.home_dir);
        cmd.env("RUNCEPT_HOME", self.home_dir.join(".runcept"));
        cmd.current_dir(&self.project_dir);
        cmd
    }

    fn start_daemon(&self) -> std::process::Child {
        std::process::Command::new("cargo")
            .args(["run", "--bin", "runcept", "--", "daemon"])
            .env("HOME", &self.home_dir)
            .env("RUNCEPT_HOME", self.home_dir.join(".runcept"))
            .current_dir(&self.project_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }

    fn wait_for_daemon(&self) {
        for _ in 0..30 {
            if self.runcept_cmd().arg("status").output().is_ok() {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
        panic!("Daemon failed to start");
    }

    fn stop_daemon(&self) {
        let _ = self.runcept_cmd().arg("shutdown").output();
        thread::sleep(Duration::from_millis(500));
    }

    fn extract_pid_from_list(&self, process_name: &str) -> Option<u32> {
        let output = self.runcept_cmd().arg("list").output().unwrap();
        let list_output = String::from_utf8_lossy(&output.stdout);

        for line in list_output.lines() {
            if line.contains(process_name) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    let pid_str = parts[2]; // PID column
                    if pid_str != "-" {
                        return pid_str.parse().ok();
                    }
                }
            }
        }
        None
    }
}

fn is_process_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        use nix::sys::signal::kill;
        use nix::unistd::Pid;

        // Handle potential invalid PIDs gracefully
        if pid == 0 {
            return false;
        }

        let nix_pid = match Pid::from_raw(pid as i32) {
            pid if pid.as_raw() > 0 => pid,
            _ => return false,
        };

        // Use kill with signal 0 to check if process exists
        match kill(nix_pid, None) {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    #[cfg(not(unix))]
    {
        // On non-Unix systems, assume process is running
        true
    }
}

#[test]
fn test_process_actually_killed_on_stop() {
    let test_env = TestEnvironment::new();
    test_env.setup_test_project();

    let mut daemon_process = test_env.start_daemon();
    test_env.wait_for_daemon();

    // Activate environment
    test_env
        .runcept_cmd()
        .args(["activate", test_env.project_dir.to_str().unwrap()])
        .assert()
        .success();

    // Start the long-running process
    test_env
        .runcept_cmd()
        .args(["start", "long_task"])
        .assert()
        .success()
        .stdout(predicate::str::contains("started").or(predicate::str::contains("Starting")));

    // Wait for process to start
    thread::sleep(Duration::from_millis(2000));

    // Verify process is running and get its PID
    test_env
        .runcept_cmd()
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("long_task"))
        .stdout(predicate::str::contains("running"));

    let pid = test_env
        .extract_pid_from_list("long_task")
        .expect("Could not extract PID from process list");

    println!("Process running with PID: {}", pid);

    // Verify the process is actually running using system call
    assert!(
        is_process_running(pid),
        "Process should be running before stop"
    );

    // Stop the process
    test_env
        .runcept_cmd()
        .args(["stop", "long_task"])
        .assert()
        .success()
        .stdout(predicate::str::contains("stopped").or(predicate::str::contains("Stopping")));

    // Wait for process to be killed
    thread::sleep(Duration::from_millis(3000));

    // Check if process is actually killed (this is the critical test)
    let is_still_running = is_process_running(pid);
    assert!(
        !is_still_running,
        "CRITICAL BUG: Process {} is still running after stop command! This means the process was marked as stopped in the database but not actually killed.",
        pid
    );

    // Also verify the database shows it as stopped
    let output = test_env.runcept_cmd().arg("list").output().unwrap();
    let list_output = String::from_utf8_lossy(&output.stdout);

    // Process should either be marked as stopped or not appear in running processes
    let process_line = list_output.lines().find(|line| line.contains("long_task"));
    if let Some(line) = process_line {
        assert!(
            line.contains("stopped"),
            "Process should be marked as stopped in database, but line was: {}",
            line
        );
    }

    println!("✅ SUCCESS: Process was properly killed and marked as stopped");

    test_env.stop_daemon();
    let _ = daemon_process.wait();
}

#[test]
fn test_process_stop_without_handle() {
    // This test simulates the case where a process exists in the database
    // but doesn't have an in-memory handle (e.g., after daemon restart)
    let test_env = TestEnvironment::new();
    test_env.setup_test_project();

    let mut daemon_process = test_env.start_daemon();
    test_env.wait_for_daemon();

    // Activate environment
    test_env
        .runcept_cmd()
        .args(["activate", test_env.project_dir.to_str().unwrap()])
        .assert()
        .success();

    // Start the long-running process
    test_env
        .runcept_cmd()
        .args(["start", "long_task"])
        .assert()
        .success();

    // Wait for process to start
    thread::sleep(Duration::from_millis(2000));

    // Get the PID
    let pid = test_env
        .extract_pid_from_list("long_task")
        .expect("Could not extract PID from process list");

    println!("Process running with PID: {}", pid);

    // Simulate daemon restart by stopping and starting daemon
    // This should cause the process to lose its in-memory handle
    test_env.stop_daemon();
    let _ = daemon_process.wait();

    // Start daemon again
    daemon_process = test_env.start_daemon();
    test_env.wait_for_daemon();

    // Activate environment again
    test_env
        .runcept_cmd()
        .args(["activate", test_env.project_dir.to_str().unwrap()])
        .assert()
        .success();

    // Process should still be running (the OS process)
    assert!(
        is_process_running(pid),
        "Process should still be running after daemon restart"
    );

    // Try to stop the process - this should work even without in-memory handle
    test_env
        .runcept_cmd()
        .args(["stop", "long_task"])
        .assert()
        .success();

    // Wait for process to be killed
    thread::sleep(Duration::from_millis(3000));

    // Critical test: process should be actually killed
    let is_still_running = is_process_running(pid);
    assert!(
        !is_still_running,
        "CRITICAL BUG: Process {} is still running after stop command (no handle case)! This means our PID-based killing fallback is not working.",
        pid
    );

    println!("✅ SUCCESS: Process was properly killed even without in-memory handle");

    test_env.stop_daemon();
    let _ = daemon_process.wait();
}
