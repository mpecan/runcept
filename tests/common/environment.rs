#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(unused_mut)]

use assert_cmd::Command;
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Mutex, Once};
use std::time::Duration;
use sysinfo::{Signal as SysinfoSignal, System};
use tempfile::TempDir;
use tokio::io::AsyncWriteExt;

static INIT: Once = Once::new();
static BINARY_PATHS: Mutex<Option<HashMap<String, PathBuf>>> = Mutex::new(None);

/// Build binaries once and cache their paths for all tests
/// This version is compatible with coverage collection tools
pub fn ensure_binaries_built() {
    INIT.call_once(|| {
        println!("Building binaries once for all tests...");

        // Create build command with proper environment inheritance
        let mut build_cmd = std::process::Command::new("cargo");
        build_cmd.args(["build", "--bins"]);

        // Inherit coverage-related environment variables
        inherit_coverage_env(&mut build_cmd);

        // Execute the build
        let build_output = build_cmd.output().expect("Failed to build binaries");

        if !build_output.status.success() {
            panic!(
                "Failed to build binaries: {}",
                String::from_utf8_lossy(&build_output.stderr)
            );
        }

        // Get the target directory
        let target_dir = std::process::Command::new("cargo")
            .args(["metadata", "--format-version=1", "--no-deps"])
            .output()
            .expect("Failed to get cargo metadata")
            .stdout;

        let metadata: serde_json::Value =
            serde_json::from_slice(&target_dir).expect("Failed to parse cargo metadata");

        let target_directory = metadata["target_directory"]
            .as_str()
            .expect("Failed to get target directory");

        let debug_dir = PathBuf::from(target_directory).join("debug");

        // Store binary paths
        let mut paths = HashMap::new();
        let binary_name = if cfg!(windows) {
            "runcept.exe"
        } else {
            "runcept"
        };
        paths.insert("runcept".to_string(), debug_dir.join(binary_name));

        // Verify binaries exist
        for (name, path) in &paths {
            if !path.exists() {
                panic!("Binary {} not found at {}", name, path.display());
            }
        }

        *BINARY_PATHS.lock().unwrap() = Some(paths);

        let coverage_info = if is_coverage_enabled() {
            " (with coverage instrumentation)"
        } else {
            ""
        };
        println!("Binaries built and cached successfully{coverage_info}");
    });
}

/// Inherit coverage-related environment variables to the build process
fn inherit_coverage_env(cmd: &mut std::process::Command) {
    // Coverage-related environment variables that need to be passed through
    let coverage_vars = [
        "RUSTFLAGS",
        "LLVM_PROFILE_FILE",
        "CARGO_INCREMENTAL",
        "RUSTDOCFLAGS",
        // Additional variables that cargo-llvm-cov might set
        "CARGO_LLVM_COV",
        "CARGO_LLVM_COV_TARGET_DIR",
    ];

    for var in &coverage_vars {
        if let Ok(value) = env::var(var) {
            cmd.env(var, value);
        }
    }

    // Ensure we inherit the current environment as well
    // This catches any other coverage-related variables we might miss
    for (key, value) in env::vars() {
        if key.starts_with("LLVM_PROFILE")
            || key.starts_with("GCOV_")
            || key.contains("COV") && key.contains("CARGO")
        {
            cmd.env(key, value);
        }
    }
}

/// Check if coverage collection is enabled
fn is_coverage_enabled() -> bool {
    env::var("LLVM_PROFILE_FILE").is_ok()
        || env::var("RUSTFLAGS")
            .unwrap_or_default()
            .contains("instrument-coverage")
        || env::var("CARGO_LLVM_COV").is_ok()
}

/// Get the path to a binary
pub fn get_binary_path(name: &str) -> PathBuf {
    get_binary_path_checked(name).unwrap()
}

/// Enhanced version with better error handling and logging
pub fn get_binary_path_checked(name: &str) -> Result<PathBuf, String> {
    ensure_binaries_built();
    let paths = BINARY_PATHS.lock().unwrap();

    match paths.as_ref().and_then(|p| p.get(name)) {
        Some(path) => {
            if path.exists() {
                Ok(path.clone())
            } else {
                Err(format!(
                    "Binary '{}' was built but no longer exists at {}",
                    name,
                    path.display()
                ))
            }
        }
        None => Err(format!("Binary '{name}' not found in built binaries")),
    }
}

/// Centralized test environment for all integration tests
/// Provides consistent environment variable handling, socket management, and cleanup
pub struct RunceptTestEnvironment {
    temp_dir: TempDir,
    home_dir: PathBuf,
    project_dir: PathBuf,
    runcept_dir: PathBuf,
    daemon_process: Option<Child>,
    daemon_pid: Option<u32>,
    config: TestConfig,
}

#[derive(Debug, Clone)]
pub struct TestConfig {
    pub project_name: String,
    pub use_custom_socket: bool,
    pub enable_logging: bool,
    pub cleanup_timeout: Duration,
    pub daemon_startup_timeout: Duration,
    pub auto_start_daemon: bool,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            project_name: "test-env".to_string(),
            use_custom_socket: true,
            enable_logging: false,
            cleanup_timeout: Duration::from_millis(500),
            daemon_startup_timeout: Duration::from_secs(5),
            auto_start_daemon: true,
        }
    }
}

impl RunceptTestEnvironment {
    /// Create a new test environment with default configuration
    pub async fn new() -> Self {
        Self::with_config(TestConfig::default()).await
    }

    /// Create a new test environment with custom configuration
    pub async fn with_config(config: TestConfig) -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let home_dir = temp_dir.path().join("home");
        let project_dir = temp_dir.path().join("project");
        let runcept_dir = home_dir.join(".runcept");

        // Create required directories
        tokio::fs::create_dir_all(&home_dir)
            .await
            .expect("Failed to create home dir");
        tokio::fs::create_dir_all(&project_dir)
            .await
            .expect("Failed to create project dir");
        tokio::fs::create_dir_all(&runcept_dir)
            .await
            .expect("Failed to create runcept dir");
        tokio::fs::create_dir_all(runcept_dir.join("logs"))
            .await
            .expect("Failed to create logs dir");

        Self {
            temp_dir,
            home_dir,
            project_dir,
            runcept_dir,
            daemon_process: None,
            daemon_pid: None,
            config,
        }
    }

    /// Get the socket path for this test environment
    pub fn get_socket_path(&self) -> PathBuf {
        self.runcept_dir.join("daemon.sock")
    }

    /// Get the home directory for this test environment
    pub fn home_dir(&self) -> &Path {
        &self.home_dir
    }

    /// Get the project directory for this test environment
    pub fn project_dir(&self) -> &Path {
        &self.project_dir
    }

    /// Get the .runcept directory for this test environment
    pub fn runcept_dir(&self) -> &Path {
        &self.runcept_dir
    }

    /// Get the daemon PID if it exists
    pub fn daemon_pid(&self) -> Option<u32> {
        self.daemon_pid
    }

    /// Create a runcept command with proper environment variables set
    pub fn runcept_cmd(&self) -> Command {
        // Use the cached binary path instead of cargo_bin
        let mut cmd = Command::new(get_binary_path("runcept"));

        // Set up environment variables consistently
        self.setup_environment_vars(&mut cmd);

        // Set working directory to project directory
        cmd.current_dir(&self.project_dir);

        cmd
    }

    pub fn binary_path(&self) -> PathBuf {
        get_binary_path("runcept")
    }

    /// Set up environment variables for a command
    fn setup_environment_vars(&self, cmd: &mut Command) {
        // Set HOME to our test home directory
        cmd.env("HOME", &self.home_dir);

        // Remove RUNCEPT_HOME since it's not used in source code
        cmd.env_remove("RUNCEPT_HOME");

        // Clear other environment variables that might interfere
        cmd.env_remove("XDG_CONFIG_HOME");
        cmd.env_remove("XDG_DATA_HOME");

        // If using custom socket, set the socket path
        if self.config.use_custom_socket {
            cmd.arg("--socket").arg(self.get_socket_path());
        }
    }

    /// Create a .runcept.toml configuration file in the project directory
    pub async fn create_config_file(&self, content: &str) -> Result<PathBuf, std::io::Error> {
        let config_path = self.project_dir.join(".runcept.toml");
        let mut file = tokio::fs::File::create(&config_path).await?;
        file.write_all(content.as_bytes()).await?;
        file.flush().await?;
        Ok(config_path)
    }

    /// Start the daemon safely with proper isolation
    pub async fn start_daemon(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.ensure_socket_cleanup();

        // Use the cached binary path instead of cargo_bin
        let mut cmd = std::process::Command::new(get_binary_path("runcept"));

        cmd.args(["daemon", "start", "--foreground"]);
        self.setup_environment_vars_for_process(&mut cmd);

        // Start daemon in background
        cmd.stdout(Stdio::null())
            .stderr(if self.config.enable_logging {
                Stdio::inherit()
            } else {
                Stdio::null()
            });

        let child = cmd.spawn()?;

        // Capture the PID for proper process management
        self.daemon_pid = Some(child.id());
        self.daemon_process = Some(child);

        // Wait for daemon to be ready
        self.wait_for_daemon().await
    }

    /// Wait for daemon to become ready
    async fn wait_for_daemon(&self) -> Result<(), Box<dyn std::error::Error>> {
        let socket_path = self.get_socket_path();
        let timeout = self.config.daemon_startup_timeout;
        let start_time = std::time::Instant::now();

        while start_time.elapsed() < timeout {
            if socket_path.exists() {
                // Verify daemon responds to status check
                if self.check_daemon_health().await.is_ok() {
                    return Ok(());
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Err(format!(
            "Daemon failed to start within {} seconds",
            timeout.as_secs()
        )
        .into())
    }

    /// Check if daemon is healthy by sending a status request
    async fn check_daemon_health(&self) -> Result<(), Box<dyn std::error::Error>> {
        let output = self.runcept_cmd().args(["daemon", "status"]).output()?;

        if output.status.success() {
            Ok(())
        } else {
            Err("Daemon health check failed".into())
        }
    }

    /// Stop the daemon gracefully
    pub async fn stop_daemon(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Try graceful shutdown first via CLI command
        let _ = self.runcept_cmd().args(["daemon", "stop"]).output();

        // If we have a PID, try to kill the process gracefully
        if let Some(pid) = self.daemon_pid.take() {
            let mut system = System::new_all();

            let sysinfo_pid = sysinfo::Pid::from_u32(pid);
            if let Some(process) = system.process(sysinfo_pid) {
                // First try SIGTERM for graceful shutdown
                if process.kill_with(SysinfoSignal::Term).unwrap_or(false) {
                    // Wait a bit for graceful shutdown
                    tokio::time::sleep(Duration::from_millis(1000)).await;

                    // Check if process is still running
                    system.refresh_all();
                    if system.process(sysinfo_pid).is_some() {
                        // Still running, force kill with SIGKILL
                        if let Some(process) = system.process(sysinfo_pid) {
                            let _ = process.kill_with(SysinfoSignal::Kill);
                        }
                    }
                }
            }
        }

        // Also clean up via Child handle if available
        if let Some(mut process) = self.daemon_process.take() {
            let _ = process.kill();
            let _ = process.wait();
        }

        // Clean up socket file
        self.ensure_socket_cleanup();

        Ok(())
    }

    /// Ensure socket file is cleaned up
    fn ensure_socket_cleanup(&self) {
        let socket_path = self.get_socket_path();
        if socket_path.exists() {
            let _ = std::fs::remove_file(&socket_path);
        }
    }

    /// Set up environment variables for std::process::Command (not assert_cmd::Command)
    fn setup_environment_vars_for_process(&self, cmd: &mut std::process::Command) {
        cmd.env("HOME", &self.home_dir);
        cmd.env_remove("RUNCEPT_HOME");
        cmd.env_remove("XDG_CONFIG_HOME");
        cmd.env_remove("XDG_DATA_HOME");

        if self.config.use_custom_socket {
            cmd.arg("--socket").arg(self.get_socket_path());
        }
    }

    /// Check if daemon is running using sysinfo
    pub fn is_daemon_running(&self) -> bool {
        if let Some(pid) = self.daemon_pid {
            let system = System::new_all();
            system.process(sysinfo::Pid::from_u32(pid)).is_some()
        } else {
            false
        }
    }

    /// Execute a command and assert it succeeds with expected output
    pub fn assert_cmd_success(&self, args: &[&str], expected_output: &str) {
        use crate::common::assertions::assert_success_with_output;
        let mut cmd = self.runcept_cmd();
        cmd.args(args);
        assert_success_with_output(cmd, expected_output);
    }

    /// Execute a command and assert it fails with expected error
    pub fn assert_cmd_failure(&self, args: &[&str], expected_error: &str) {
        use crate::common::assertions::assert_failure_with_error;
        let mut cmd = self.runcept_cmd();
        cmd.args(args);
        assert_failure_with_error(cmd, expected_error);
    }

    /// Execute a command and return the output for custom assertions
    pub fn execute_cmd(&self, args: &[&str]) -> std::process::Output {
        self.runcept_cmd()
            .args(args)
            .output()
            .expect("Failed to execute command")
    }

    // Environment management convenience methods
    pub fn activate_environment(&self, path: Option<&str>) -> std::process::Output {
        let path = path.unwrap_or_else(|| self.project_dir().to_str().unwrap());
        self.execute_cmd(&["activate", path])
    }

    pub fn deactivate_environment(&self) -> std::process::Output {
        self.execute_cmd(&["deactivate"])
    }

    pub fn status(&self) -> std::process::Output {
        self.execute_cmd(&["status"])
    }

    // Process management convenience methods
    pub fn start_process(&self, name: &str) -> std::process::Output {
        self.execute_cmd(&["start", name])
    }

    pub fn start_process_with_env(&self, name: &str, env_path: &str) -> std::process::Output {
        self.execute_cmd(&["start", name, "--environment", env_path])
    }

    pub fn stop_process(&self, name: &str) -> std::process::Output {
        self.execute_cmd(&["stop", name])
    }

    pub fn stop_process_with_env(&self, name: &str, env_path: &str) -> std::process::Output {
        self.execute_cmd(&["stop", name, "--environment", env_path])
    }

    pub fn restart_process(&self, name: &str) -> std::process::Output {
        self.execute_cmd(&["restart", name])
    }

    // Process information convenience methods
    pub fn list_processes(&self) -> std::process::Output {
        self.execute_cmd(&["list"])
    }

    pub fn list_processes_with_env(&self, env_path: &str) -> std::process::Output {
        self.execute_cmd(&["list", "--environment", env_path])
    }

    pub fn get_process_logs(&self, name: &str) -> std::process::Output {
        self.execute_cmd(&["logs", name])
    }

    pub fn get_process_logs_limited(&self, name: &str, lines: u32) -> std::process::Output {
        self.execute_cmd(&["logs", name, "--lines", &lines.to_string()])
    }

    // Daemon management convenience methods
    pub fn daemon_status(&self) -> std::process::Output {
        self.execute_cmd(&["daemon", "status"])
    }

    pub fn daemon_start(&self) -> std::process::Output {
        self.execute_cmd(&["daemon", "start"])
    }

    pub fn daemon_stop(&self) -> std::process::Output {
        self.execute_cmd(&["daemon", "stop"])
    }

    // Initialization convenience methods
    pub fn init_project(&self, path: Option<&str>, force: bool) -> std::process::Output {
        let mut args = vec!["init"];
        if let Some(path) = path {
            args.push(path);
        }
        if force {
            args.push("--force");
        }
        self.execute_cmd(&args)
    }

    // MCP server convenience methods
    /// Spawn an MCP server process with proper stdio configuration
    pub fn spawn_mcp_server(&self) -> Result<std::process::Child, std::io::Error> {
        use std::process::{Command, Stdio};

        let mut mcp_cmd = Command::new(self.binary_path());
        mcp_cmd
            .args(["mcp"])
            .env("HOME", self.home_dir())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        mcp_cmd.spawn()
    }

    /// Spawn an MCP server process with a specific working directory
    pub fn spawn_mcp_server_with_cwd(
        &self,
        working_dir: &std::path::Path,
    ) -> Result<std::process::Child, std::io::Error> {
        use std::process::{Command, Stdio};

        let mut mcp_cmd = Command::new(self.binary_path());
        mcp_cmd
            .args(["mcp"])
            .env("HOME", self.home_dir())
            .current_dir(working_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        mcp_cmd.spawn()
    }

    /// Get the daemon logs for debugging purposes
    pub fn get_daemon_logs(&self) -> Result<String, std::io::Error> {
        let log_path = self.runcept_dir.join("logs").join("daemon.log");
        if log_path.exists() {
            std::fs::read_to_string(log_path)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Daemon log file not found",
            ))
        }
    }

    /// Get the MCP server logs for debugging purposes
    pub fn get_mcp_server_logs(&self) -> Result<String, std::io::Error> {
        let log_path = self.runcept_dir.join("logs").join("mcp-server.log");
        if log_path.exists() {
            std::fs::read_to_string(log_path)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "MCP server log file not found",
            ))
        }
    }

    /// Get the cli logs for debugging purposes
    pub fn get_cli_logs(&self) -> Result<String, std::io::Error> {
        let log_path = self.runcept_dir.join("logs").join("cli.log");
        if log_path.exists() {
            std::fs::read_to_string(log_path)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "CLI log file not found",
            ))
        }
    }
}

impl Drop for RunceptTestEnvironment {
    fn drop(&mut self) {
        // Try graceful shutdown first via CLI command
        let _ = std::process::Command::new(get_binary_path("runcept"))
            .args(["daemon", "stop"])
            .env("HOME", &self.home_dir)
            .arg("--socket")
            .arg(self.get_socket_path())
            .output();

        // If we have a PID, try sysinfo for cleanup
        if let Some(pid) = self.daemon_pid.take() {
            let mut system = System::new_all();

            let sysinfo_pid = sysinfo::Pid::from_u32(pid);
            if let Some(process) = system.process(sysinfo_pid) {
                // First try SIGTERM for graceful shutdown
                if process.kill_with(SysinfoSignal::Term).unwrap_or(false) {
                    // Wait a bit for graceful shutdown
                    std::thread::sleep(Duration::from_millis(500));

                    // Check if process is still running
                    system.refresh_all();
                    if system.process(sysinfo_pid).is_some() {
                        // Still running, force kill with SIGKILL
                        if let Some(process) = system.process(sysinfo_pid) {
                            let _ = process.kill_with(SysinfoSignal::Kill);
                        }
                    }
                }
            }
        }

        // Also stop any running daemon via Child handle
        if let Some(mut process) = self.daemon_process.take() {
            let _ = process.kill();
            let _ = process.wait();
        }

        // Clean up socket file
        self.ensure_socket_cleanup();

        // Wait for cleanup to complete
        std::thread::sleep(self.config.cleanup_timeout);
    }
}
