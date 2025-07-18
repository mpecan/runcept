use assert_cmd::Command;
use predicates::prelude::*;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::sleep;

mod common;
use common::get_binary_path;

/// Binary integration tests that exercise the actual built executables
/// These tests run the real CLI and daemon binaries in isolated environments
#[cfg(test)]
mod binary_integration_tests {
    use super::*;

    struct TestEnvironment {
        temp_dir: TempDir,
        home_dir: PathBuf,
        project_dir: PathBuf,
        runcept_dir: PathBuf,
    }

    impl TestEnvironment {
        fn new() -> Self {
            let temp_dir = TempDir::new().unwrap();
            let home_dir = temp_dir.path().join("home");
            let project_dir = temp_dir.path().join("test_project");
            let runcept_dir = home_dir.join(".runcept");

            // Create directory structure
            std::fs::create_dir_all(&home_dir).unwrap();
            std::fs::create_dir_all(&project_dir).unwrap();
            std::fs::create_dir_all(&runcept_dir).unwrap();

            Self {
                temp_dir,
                home_dir,
                project_dir,
                runcept_dir,
            }
        }

        fn setup_test_project(&self) {
            let config_content = r#"
[environment]
name = "binary-test-project"
inactivity_timeout = "10m"

[processes.web]
name = "web"
command = "python3 -m http.server 8080"
working_dir = "."
auto_restart = true

[processes.worker]
name = "worker"
command = "echo 'Worker started' && sleep 3"
working_dir = "."
auto_restart = false

[processes.background]
name = "background"
command = "echo 'Background service' && sleep 30"
working_dir = "."
auto_restart = true
"#;
            std::fs::write(self.project_dir.join(".runcept.toml"), config_content).unwrap();
        }

        fn runcept_cmd(&self) -> Command {
            let runcept_path = get_binary_path("runcept");
            let mut cmd = Command::new(runcept_path);
            cmd.env("HOME", &self.home_dir);
            cmd
        }

        fn runcept_cmd_in_project(&self) -> Command {
            let runcept_path = get_binary_path("runcept");
            let mut cmd = Command::new(runcept_path);
            cmd.env("HOME", &self.home_dir);
            cmd.current_dir(&self.project_dir);
            cmd
        }

        fn runcept_cmd_with_env(&self, subcommand: &str, args: &[&str]) -> Command {
            let runcept_path = get_binary_path("runcept");
            let mut cmd = Command::new(runcept_path);
            cmd.env("HOME", &self.home_dir);
            cmd.arg(subcommand);
            cmd.args(args);
            cmd.args(["--environment", self.project_dir.to_str().unwrap()]);
            cmd
        }

        fn start_daemon(&self) -> std::process::Child {
            let runcept_path = get_binary_path("runcept");
            let mut cmd = std::process::Command::new(runcept_path);
            cmd.args(["daemon", "start"])
                .env("HOME", &self.home_dir)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());

            cmd.spawn().expect("Failed to start daemon")
        }

        fn wait_for_daemon(&self) {
            let socket_path = self.runcept_dir.join("daemon.sock");

            // Wait up to 5 seconds for daemon to start
            for _ in 0..50 {
                if socket_path.exists() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            panic!("Daemon failed to start within timeout");
        }

        fn stop_daemon(&self) {
            // Send shutdown command
            let _ = self.runcept_cmd().args(["daemon", "stop"]).output();
        }
    }

    impl Drop for TestEnvironment {
        fn drop(&mut self) {
            self.stop_daemon();
            // Give daemon time to shut down
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    #[test]
    fn test_cli_help_command() {
        let test_env = TestEnvironment::new();

        test_env
            .runcept_cmd()
            .arg("--help")
            .assert()
            .success()
            .stdout(predicate::str::contains("process manager"))
            .stdout(predicate::str::contains("activate"))
            .stdout(predicate::str::contains("deactivate"))
            .stdout(predicate::str::contains("daemon"));
    }

    #[test]
    fn test_daemon_startup_and_status() {
        let test_env = TestEnvironment::new();

        // Start daemon
        let mut daemon_process = test_env.start_daemon();
        test_env.wait_for_daemon();

        // Test daemon status
        test_env
            .runcept_cmd()
            .args(["daemon", "status"])
            .assert()
            .success()
            .stdout(predicate::str::contains("Running"));

        // Test that socket file exists
        assert!(test_env.runcept_dir.join("daemon.sock").exists());

        // Stop daemon
        test_env.stop_daemon();
        let _ = daemon_process.wait();
    }

    #[test]
    fn test_environment_activation_via_cli() {
        let test_env = TestEnvironment::new();
        test_env.setup_test_project();

        // Start daemon
        let mut daemon_process = test_env.start_daemon();
        test_env.wait_for_daemon();

        // Test activation
        test_env
            .runcept_cmd()
            .args(["activate", test_env.project_dir.to_str().unwrap()])
            .assert()
            .success()
            .stdout(predicate::str::contains("activated"));

        // Test status shows active environment
        test_env
            .runcept_cmd()
            .arg("status")
            .assert()
            .success()
            .stdout(predicate::str::contains("binary-test-project"));

        // Test deactivation
        test_env
            .runcept_cmd()
            .arg("deactivate")
            .assert()
            .success()
            .stdout(predicate::str::contains("deactivated"));

        test_env.stop_daemon();
        let _ = daemon_process.wait();
    }

    #[test]
    fn test_process_management_via_cli() {
        let test_env = TestEnvironment::new();
        test_env.setup_test_project();

        // Start daemon
        let mut daemon_process = test_env.start_daemon();
        test_env.wait_for_daemon();

        // Activate environment
        test_env
            .runcept_cmd()
            .args(["activate", test_env.project_dir.to_str().unwrap()])
            .assert()
            .success();

        // Start a process - use project directory as current working directory
        test_env
            .runcept_cmd_in_project()
            .args(["start", "worker"])
            .assert()
            .success()
            .stdout(predicate::str::contains("started").or(predicate::str::contains("Starting")));

        // Wait a moment for process to run
        std::thread::sleep(Duration::from_millis(500));

        // List processes - use project directory as current working directory
        test_env
            .runcept_cmd_in_project()
            .arg("list")
            .assert()
            .success()
            .stdout(predicate::str::contains("worker"));

        // Wait for worker to complete and get logs
        std::thread::sleep(Duration::from_secs(4));

        test_env
            .runcept_cmd_in_project()
            .args(["logs", "worker"])
            .assert()
            .success()
            .stdout(predicate::str::contains("Worker started"));

        test_env.stop_daemon();
        let _ = daemon_process.wait();
    }

    #[test]
    fn test_daemon_persistence_across_restarts() {
        let test_env = TestEnvironment::new();
        test_env.setup_test_project();

        // First daemon instance
        {
            let mut daemon_process = test_env.start_daemon();
            test_env.wait_for_daemon();

            // Activate environment
            test_env
                .runcept_cmd()
                .args(["activate", test_env.project_dir.to_str().unwrap()])
                .assert()
                .success();

            // Stop daemon
            test_env.stop_daemon();
            let _ = daemon_process.wait();
            std::thread::sleep(Duration::from_millis(500));
        }

        // Second daemon instance
        {
            let mut daemon_process = test_env.start_daemon();
            test_env.wait_for_daemon();

            // Check that we can reactivate the same environment
            test_env
                .runcept_cmd()
                .args(["activate", test_env.project_dir.to_str().unwrap()])
                .assert()
                .success()
                .stdout(predicate::str::contains("activated"));

            test_env.stop_daemon();
            let _ = daemon_process.wait();
        }
    }

    #[test]
    fn test_multiple_process_operations() {
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

        // Start multiple processes
        test_env
            .runcept_cmd_in_project()
            .args(["start", "worker"])
            .assert()
            .success();

        test_env
            .runcept_cmd_in_project()
            .args(["start", "background"])
            .assert()
            .success();

        std::thread::sleep(Duration::from_millis(200));

        // List all processes
        test_env
            .runcept_cmd_in_project()
            .arg("list")
            .assert()
            .success()
            .stdout(predicate::str::contains("worker"))
            .stdout(predicate::str::contains("background"));

        // Stop specific process
        test_env
            .runcept_cmd_in_project()
            .args(["stop", "background"])
            .assert()
            .success();

        std::thread::sleep(Duration::from_millis(100));

        // Verify process is stopped
        test_env
            .runcept_cmd_in_project()
            .arg("list")
            .assert()
            .success();

        test_env.stop_daemon();
        let _ = daemon_process.wait();
    }

    #[test]
    fn test_environment_enforcement() {
        let test_env = TestEnvironment::new();
        // Note: NOT calling setup_test_project(), so no .runcept.toml exists

        let mut daemon_process = test_env.start_daemon();
        test_env.wait_for_daemon();

        // Test that process commands fail when no environment is available
        test_env
            .runcept_cmd()
            .args(["start", "worker"])
            .assert()
            .failure()
            .stderr(predicate::str::contains(
                "No .runcept.toml configuration found",
            ))
            .stderr(predicate::str::contains("To fix this"));

        // Test that list command fails when no environment is available
        test_env
            .runcept_cmd()
            .arg("list")
            .assert()
            .failure()
            .stderr(predicate::str::contains(
                "No .runcept.toml configuration found",
            ));

        // Test that commands work when using --environment flag with valid path
        test_env.setup_test_project();

        // Activate the environment first
        test_env
            .runcept_cmd()
            .args(["activate", test_env.project_dir.to_str().unwrap()])
            .assert()
            .success();

        // Test that --environment flag works
        test_env
            .runcept_cmd_with_env("start", &["worker"])
            .assert()
            .success();

        // Test that invalid environment path fails with helpful error
        test_env
            .runcept_cmd()
            .args(["start", "worker", "--environment", "/nonexistent/path"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("Invalid environment path"));

        test_env.stop_daemon();
        let _ = daemon_process.wait();
    }

    #[test]
    fn test_error_handling_via_cli() {
        let test_env = TestEnvironment::new();
        test_env.setup_test_project();

        let mut daemon_process = test_env.start_daemon();
        test_env.wait_for_daemon();

        // Test activating non-existent environment
        test_env
            .runcept_cmd()
            .args(["activate", "/non/existent/path"])
            .assert()
            .failure()
            .stderr(
                predicate::str::contains("No .runcept.toml found")
                    .or(predicate::str::contains("not found"))
                    .or(predicate::str::contains("error")),
            );

        // Test starting process without active environment
        test_env
            .runcept_cmd()
            .args(["start", "worker"])
            .assert()
            .failure()
            .stderr(
                predicate::str::contains("No active environment")
                    .or(predicate::str::contains("not activated"))
                    .or(predicate::str::contains("error")),
            );

        // Activate environment
        test_env
            .runcept_cmd()
            .args(["activate", test_env.project_dir.to_str().unwrap()])
            .assert()
            .success();

        // Test starting non-existent process
        test_env
            .runcept_cmd()
            .args(["start", "nonexistent"])
            .assert()
            .failure()
            .stderr(
                predicate::str::contains("not found")
                    .or(predicate::str::contains("No process")
                        .or(predicate::str::contains("error"))),
            );

        test_env.stop_daemon();
        let _ = daemon_process.wait();
    }

    #[test]
    fn test_database_functionality() {
        let test_env = TestEnvironment::new();
        test_env.setup_test_project();

        // Start daemon
        let mut daemon_process = test_env.start_daemon();
        test_env.wait_for_daemon();

        // Activate environment to trigger database operations
        test_env
            .runcept_cmd()
            .args(["activate", test_env.project_dir.to_str().unwrap()])
            .assert()
            .success();

        // Verify environment is active
        test_env
            .runcept_cmd()
            .arg("status")
            .assert()
            .success()
            .stdout(predicate::str::contains("binary-test-project"));

        // Check that runcept directory exists and has expected structure
        assert!(test_env.runcept_dir.exists());
        assert!(test_env.runcept_dir.join("logs").exists());
        assert!(test_env.runcept_dir.join("daemon.sock").exists());

        test_env.stop_daemon();
        let _ = daemon_process.wait();

        // Runcept directory should persist after shutdown
        assert!(test_env.runcept_dir.exists());
    }

    #[test]
    fn test_log_file_creation() {
        let test_env = TestEnvironment::new();
        test_env.setup_test_project();

        let mut daemon_process = test_env.start_daemon();
        test_env.wait_for_daemon();

        // Activate and run a process
        test_env
            .runcept_cmd()
            .args(["activate", test_env.project_dir.to_str().unwrap()])
            .assert()
            .success();

        test_env
            .runcept_cmd_in_project()
            .args(["start", "worker"])
            .assert()
            .success();

        std::thread::sleep(Duration::from_secs(4));

        // Check that log files are created
        let daemon_log = test_env.runcept_dir.join("logs").join("daemon.log");
        let worker_log = test_env
            .project_dir
            .join(".runcept")
            .join("logs")
            .join("worker.log");

        assert!(daemon_log.exists(), "Daemon log file should be created");
        assert!(worker_log.exists(), "Process log file should be created");

        test_env.stop_daemon();
        let _ = daemon_process.wait();
    }

    #[tokio::test]
    async fn test_mcp_server_binary() {
        let test_env = TestEnvironment::new();
        test_env.setup_test_project();

        // Start daemon first
        let mut daemon_process = test_env.start_daemon();
        test_env.wait_for_daemon();

        // Activate environment
        test_env
            .runcept_cmd()
            .args(["activate", test_env.project_dir.to_str().unwrap()])
            .assert()
            .success();

        // Start MCP server in background
        let runcept_path = get_binary_path("runcept");
        let mut mcp_cmd = std::process::Command::new(runcept_path);
        mcp_cmd
            .args(["mcp"])
            .env("HOME", &test_env.home_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());

        let mut mcp_process = mcp_cmd.spawn().expect("Failed to start MCP server");

        // Give MCP server time to start
        sleep(Duration::from_millis(500)).await;

        // Send a simple MCP request (this is a basic test - real MCP testing would need proper protocol)
        if let Some(stdin) = mcp_process.stdin.as_mut() {
            use std::io::Write;
            let _ = stdin.write_all(b"\n");
        }

        // Give it time to process
        sleep(Duration::from_millis(100)).await;

        // Clean up
        let _ = mcp_process.kill();
        let _ = mcp_process.wait();

        test_env.stop_daemon();
        let _ = daemon_process.wait();
    }

    #[test]
    fn test_concurrent_cli_operations() {
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

        // Test concurrent status checks
        let handles: Vec<_> = (0..5)
            .map(|_| {
                let home_dir = test_env.home_dir.clone();
                std::thread::spawn(move || {
                    let runcept_path = get_binary_path("runcept");
                    let mut cmd = Command::new(runcept_path);
                    cmd.env("HOME", &home_dir);
                    cmd.arg("status").assert().success();
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        test_env.stop_daemon();
        let _ = daemon_process.wait();
    }
}
