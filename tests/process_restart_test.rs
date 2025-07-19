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
name = "restart-test"
inactivity_timeout = "10m"

[processes.short_task]
name = "short_task"
command = "echo 'Hello World' && sleep 1"
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
}

#[test]
fn test_process_restart_after_completion() {
    // This test verifies that we can restart a process after it has completed
    // and been marked as "stopped" in the database. This should NOT cause
    // a UNIQUE constraint violation.
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

    // Start the short-lived process
    test_env
        .runcept_cmd()
        .args(["start", "short_task"])
        .assert()
        .success()
        .stdout(predicate::str::contains("started").or(predicate::str::contains("Starting")));

    // Wait for process to complete (should finish in ~1 second)
    thread::sleep(Duration::from_millis(3000));

    // Verify process is stopped
    test_env
        .runcept_cmd()
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("short_task"))
        .stdout(predicate::str::contains("stopped"));

    // Now try to start the same process again - this should NOT fail with UNIQUE constraint
    test_env
        .runcept_cmd()
        .args(["start", "short_task"])
        .assert()
        .success()
        .stdout(predicate::str::contains("started").or(predicate::str::contains("Starting")))
        .stdout(predicate::str::contains("UNIQUE constraint").not()); // Should NOT contain this error

    // Wait for second run to complete
    thread::sleep(Duration::from_millis(3000));

    // Verify process is stopped again
    test_env
        .runcept_cmd()
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("short_task"))
        .stdout(predicate::str::contains("stopped"));

    // Try starting it a third time to be absolutely sure
    test_env
        .runcept_cmd()
        .args(["start", "short_task"])
        .assert()
        .success()
        .stdout(predicate::str::contains("started").or(predicate::str::contains("Starting")))
        .stdout(predicate::str::contains("UNIQUE constraint").not()); // Should NOT contain this error

    println!(
        "✅ SUCCESS: Process can be restarted multiple times without UNIQUE constraint errors"
    );

    test_env.stop_daemon();
    let _ = daemon_process.wait();
}

#[test]
fn test_process_restart_command() {
    // Test the explicit restart command
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

    // Start the process
    test_env
        .runcept_cmd()
        .args(["start", "short_task"])
        .assert()
        .success();

    // Wait for process to complete
    thread::sleep(Duration::from_millis(3000));

    // Use the restart command - this should also work without UNIQUE constraint errors
    test_env
        .runcept_cmd()
        .args(["restart", "short_task"])
        .assert()
        .success()
        .stdout(predicate::str::contains("UNIQUE constraint").not()); // Should NOT contain this error

    println!("✅ SUCCESS: Restart command works without UNIQUE constraint errors");

    test_env.stop_daemon();
    let _ = daemon_process.wait();
}
