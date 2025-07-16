use assert_cmd::Command;
use predicates::prelude::*;
use std::path::PathBuf;
use tempfile::TempDir;

mod common;
use common::get_binary_path;

/// Tests for the init command functionality
#[cfg(test)]
mod init_command_tests {
    use super::*;

    struct TestEnvironment {
        temp_dir: TempDir,
        project_dir: PathBuf,
    }

    impl TestEnvironment {
        fn new() -> Self {
            let temp_dir = TempDir::new().unwrap();
            let project_dir = temp_dir.path().join("test_project");
            std::fs::create_dir_all(&project_dir).unwrap();
            
            Self {
                temp_dir,
                project_dir,
            }
        }

        fn runcept_cmd(&self) -> Command {
            let runcept_path = get_binary_path("runcept");
            let mut cmd = Command::new(runcept_path);
            cmd.env("HOME", self.temp_dir.path());
            cmd
        }

        fn start_daemon(&self) -> std::process::Child {
            let runcept_path = get_binary_path("runcept");
            let mut cmd = std::process::Command::new(runcept_path);
            cmd.args(&["daemon", "start"])
                .env("HOME", self.temp_dir.path())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            
            cmd.spawn().expect("Failed to start daemon")
        }

        fn wait_for_daemon(&self) {
            let socket_path = self.temp_dir.path().join(".runcept").join("daemon.sock");
            
            // Wait up to 5 seconds for daemon to start
            for _ in 0..50 {
                if socket_path.exists() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            panic!("Daemon failed to start within timeout");
        }

        fn stop_daemon(&self) {
            let _ = self.runcept_cmd()
                .args(&["daemon", "stop"])
                .output();
        }
    }

    impl Drop for TestEnvironment {
        fn drop(&mut self) {
            self.stop_daemon();
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    #[test]
    fn test_init_command_creates_default_config() {
        let test_env = TestEnvironment::new();
        
        // Start daemon
        let mut daemon_process = test_env.start_daemon();
        test_env.wait_for_daemon();
        
        // Test init command
        test_env.runcept_cmd()
            .args(&["init", test_env.project_dir.to_str().unwrap()])
            .assert()
            .success()
            .stdout(predicate::str::contains("Initialized new runcept project"));
        
        // Verify config file was created
        let config_path = test_env.project_dir.join(".runcept.toml");
        assert!(config_path.exists(), "Config file should be created");
        
        // Verify config content
        let config_content = std::fs::read_to_string(&config_path).unwrap();
        assert!(config_content.contains("name = \"test_project\""));
        assert!(config_content.contains("worker"));
        assert!(config_content.contains("echo 'Hello from worker'"));
        
        test_env.stop_daemon();
        let _ = daemon_process.wait();
    }

    #[test]
    fn test_init_command_with_force_flag() {
        let test_env = TestEnvironment::new();
        
        // Start daemon
        let mut daemon_process = test_env.start_daemon();
        test_env.wait_for_daemon();
        
        // Create initial config
        test_env.runcept_cmd()
            .args(&["init", test_env.project_dir.to_str().unwrap()])
            .assert()
            .success();
        
        // Test init without force should fail
        test_env.runcept_cmd()
            .args(&["init", test_env.project_dir.to_str().unwrap()])
            .assert()
            .failure()
            .stderr(predicate::str::contains("Configuration file already exists"));
        
        // Test init with force should succeed
        test_env.runcept_cmd()
            .args(&["init", test_env.project_dir.to_str().unwrap(), "--force"])
            .assert()
            .success()
            .stdout(predicate::str::contains("Initialized new runcept project"));
        
        test_env.stop_daemon();
        let _ = daemon_process.wait();
    }

    #[test]
    fn test_init_command_current_directory() {
        let test_env = TestEnvironment::new();
        
        // Start daemon
        let mut daemon_process = test_env.start_daemon();
        test_env.wait_for_daemon();
        
        // Test init in current directory
        test_env.runcept_cmd()
            .current_dir(&test_env.project_dir)
            .arg("init")
            .assert()
            .success()
            .stdout(predicate::str::contains("Initialized new runcept project"));
        
        // Verify config file was created
        let config_path = test_env.project_dir.join(".runcept.toml");
        assert!(config_path.exists(), "Config file should be created");
        
        test_env.stop_daemon();
        let _ = daemon_process.wait();
    }

    #[test]
    fn test_init_followed_by_activation() {
        let test_env = TestEnvironment::new();
        
        // Start daemon
        let mut daemon_process = test_env.start_daemon();
        test_env.wait_for_daemon();
        
        // Initialize project
        test_env.runcept_cmd()
            .args(&["init", test_env.project_dir.to_str().unwrap()])
            .assert()
            .success();
        
        // Now activation should work
        test_env.runcept_cmd()
            .args(&["activate", test_env.project_dir.to_str().unwrap()])
            .assert()
            .success()
            .stdout(predicate::str::contains("activated"));
        
        // Verify status shows active environment
        test_env.runcept_cmd()
            .arg("status")
            .assert()
            .success()
            .stdout(predicate::str::contains("test_project"));
        
        test_env.stop_daemon();
        let _ = daemon_process.wait();
    }
}