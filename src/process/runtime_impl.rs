use crate::error::{Result, RunceptError};
use crate::process::Process;
use crate::process::logging::ProcessLogger;
use crate::process::traits::{ProcessExitResult, ProcessHandle, ProcessRuntimeTrait};
use async_trait::async_trait;
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
use tokio::sync::oneshot;
use tracing::{debug, error};

/// Default implementation of ProcessRuntimeTrait
///
/// Handles actual system process spawning and management operations.
/// This implementation provides the concrete system-level operations
/// that interact with the operating system.
pub struct DefaultProcessRuntime;

impl DefaultProcessRuntime {
    pub fn new() -> Self {
        Self
    }

    /// Parse command string into command and arguments
    fn parse_command(command_str: &str) -> Result<(String, Vec<String>)> {
        let parts = shlex::split(command_str)
            .ok_or_else(|| RunceptError::ProcessError("Failed to parse command".to_string()))?;

        if parts.is_empty() {
            return Err(RunceptError::ProcessError("Empty command".to_string()));
        }

        let command = parts[0].clone();
        let args = parts[1..].to_vec();

        Ok((command, args))
    }

    /// Setup environment variables for process
    fn setup_environment_vars(
        command: &mut Command,
        process_env_vars: &std::collections::HashMap<String, String>,
    ) {
        for (key, value) in process_env_vars {
            command.env(key, value);
        }
    }
}

#[async_trait]
impl ProcessRuntimeTrait for DefaultProcessRuntime {
    async fn spawn_process(&self, process: &Process) -> Result<ProcessHandle> {
        debug!(
            "Spawning process '{}' with command: {}",
            process.name, process.command
        );

        // Debug: Log the working directory being used
        error!(
            "RUNTIME DEBUG: Process '{}' working directory: '{}'",
            process.name, process.working_dir
        );

        // Parse the command
        let (command_name, args) = Self::parse_command(&process.command)?;

        // Create the command
        let mut command = Command::new(&command_name);
        command.args(&args);

        // Set working directory
        command.current_dir(&process.working_dir);

        // Debug: Log what we're about to execute
        error!(
            "RUNTIME DEBUG: About to execute: {} {:?} in directory: {}",
            command_name, args, process.working_dir
        );

        // Set up stdio - CRITICAL: We need piped stdout/stderr for capturing
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        command.stdin(Stdio::null());

        // Create the process in its own process group for proper cleanup
        #[cfg(unix)]
        {
            #[allow(unused_imports)]
            use std::os::unix::process::CommandExt;
            command.process_group(0); // Create new process group
        }

        // Set environment variables
        Self::setup_environment_vars(&mut command, &process.env_vars);

        // Spawn the process
        let mut child = command.spawn().map_err(|e| {
            RunceptError::ProcessError(format!("Failed to spawn process '{}': {}", process.name, e))
        })?;

        error!(
            "RUNTIME DEBUG: Process '{}' spawned successfully with PID: {:?}",
            process.name,
            child.id()
        );

        // Create logger for capturing output
        let logger = ProcessLogger::new(
            process.name.clone(),
            Path::new(&process.working_dir).to_path_buf(),
        )
        .await?;

        // Take stdout and stderr for capturing
        if let Some(stdout) = child.stdout.take() {
            Self::spawn_output_capture_task(
                stdout,
                process.name.clone(),
                true,
                &process.working_dir,
            );
        }
        if let Some(stderr) = child.stderr.take() {
            Self::spawn_output_capture_task(
                stderr,
                process.name.clone(),
                false,
                &process.working_dir,
            );
        }

        // Create shutdown channel
        let (shutdown_tx, _shutdown_rx) = oneshot::channel();

        // Create process handle
        let handle = ProcessHandle {
            child,
            logger: Some(logger),
            shutdown_tx: Some(shutdown_tx),
        };

        Ok(handle)
    }
}

impl DefaultProcessRuntime {
    /// Terminate a process gracefully
    pub async fn terminate_process(&self, handle: &mut ProcessHandle) -> Result<()> {
        debug!("Terminating process gracefully");

        // Send shutdown signal if available
        if let Some(shutdown_tx) = handle.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }

        // Try graceful termination first
        if let Some(pid) = handle.child.id() {
            use sysinfo::{Signal as SysinfoSignal, System};

            let system = System::new_all();
            let sysinfo_pid = sysinfo::Pid::from_u32(pid);

            // Check if the process exists and send SIGTERM
            if let Some(process) = system.process(sysinfo_pid) {
                if process.kill_with(SysinfoSignal::Term).unwrap_or(false) {
                    debug!("Sent SIGTERM to process {}", pid);

                    // Wait a bit for graceful shutdown
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

                    // Check if process is still running
                    if let Ok(Some(_status)) = handle.child.try_wait() {
                        debug!("Process terminated gracefully");
                        return Ok(());
                    }
                }
            }
        }

        // If graceful shutdown failed, force kill
        self.kill_process(handle).await
    }

    /// Force kill a process
    pub async fn kill_process(&self, handle: &mut ProcessHandle) -> Result<()> {
        debug!("Force killing process");

        match handle.child.kill().await {
            Ok(()) => {
                debug!("Process killed successfully");
                Ok(())
            }
            Err(e) => {
                error!("Failed to kill process: {}", e);
                Err(RunceptError::ProcessError(format!(
                    "Failed to kill process: {e}"
                )))
            }
        }
    }

    /// Wait for process exit
    pub async fn wait_for_exit(&self, handle: &mut ProcessHandle) -> Result<ProcessExitResult> {
        debug!("Waiting for process exit");

        let exit_status = handle.child.wait().await.map_err(|e| {
            RunceptError::ProcessError(format!("Failed to wait for process exit: {e}"))
        })?;

        let exit_code = exit_status.code();
        let signal = None; // TODO: Extract signal information on Unix

        let result = ProcessExitResult {
            exit_code,
            signal,
            timestamp: chrono::Utc::now(),
        };

        debug!("Process exited with code: {:?}", exit_code);
        Ok(result)
    }

    /// Spawn a task to capture and log output from a process stream
    fn spawn_output_capture_task<T>(
        stream: T,
        process_name: String,
        is_stdout: bool,
        working_dir: &str,
    ) where
        T: tokio::io::AsyncRead + Send + Unpin + 'static,
    {
        let working_dir = working_dir.to_string();
        tokio::spawn(async move {
            use tokio::io::{AsyncBufReadExt, BufReader};

            // Create a logger for this capture task
            let logger_result =
                ProcessLogger::new(process_name.clone(), Path::new(&working_dir).to_path_buf())
                    .await;

            let mut logger = match logger_result {
                Ok(logger) => logger,
                Err(e) => {
                    eprintln!("Failed to create logger for process '{process_name}': {e}");
                    return;
                }
            };

            let mut reader = BufReader::new(stream);
            let mut line = String::new();

            while let Ok(n) = reader.read_line(&mut line).await {
                if n == 0 {
                    break; // EOF
                }

                let trimmed = line.trim_end();
                if !trimmed.is_empty() {
                    let result = if is_stdout {
                        logger.log_stdout(trimmed).await
                    } else {
                        logger.log_stderr(trimmed).await
                    };
                    if let Err(e) = result {
                        eprintln!("Failed to log output for '{process_name}': {e}");
                    }
                }
                line.clear();
            }
        });
    }
}

impl Default for DefaultProcessRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{Process, ProcessStatus};
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn create_test_process(name: &str, command: &str, working_dir: &str) -> Process {
        Process {
            id: format!("test:{name}"),
            name: name.to_string(),
            command: command.to_string(),
            working_dir: working_dir.to_string(),
            environment_id: "test".to_string(),
            pid: None,
            status: ProcessStatus::Stopped,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            last_activity: None,
            auto_restart: false,
            health_check_url: None,
            health_check_interval: None,
            depends_on: Vec::new(),
            env_vars: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn test_parse_command() {
        let (cmd, args) = DefaultProcessRuntime::parse_command("echo hello world").unwrap();
        assert_eq!(cmd, "echo");
        assert_eq!(args, vec!["hello", "world"]);

        let (cmd, args) = DefaultProcessRuntime::parse_command("ls -la /tmp").unwrap();
        assert_eq!(cmd, "ls");
        assert_eq!(args, vec!["-la", "/tmp"]);
    }

    #[tokio::test]
    async fn test_spawn_simple_process() {
        let temp_dir = TempDir::new().unwrap();
        let process =
            create_test_process("test_echo", "echo hello", temp_dir.path().to_str().unwrap());

        let runtime = DefaultProcessRuntime::new();
        let mut handle = runtime.spawn_process(&process).await.unwrap();

        assert!(handle.child.id().is_some());

        // Wait for process to complete
        let result = runtime.wait_for_exit(&mut handle).await.unwrap();
        assert_eq!(result.exit_code, Some(0));
    }

    #[tokio::test]
    async fn test_process_lifecycle() {
        let temp_dir = TempDir::new().unwrap();
        let process =
            create_test_process("test_sleep", "sleep 10", temp_dir.path().to_str().unwrap());

        let runtime = DefaultProcessRuntime::new();
        let mut handle = runtime.spawn_process(&process).await.unwrap();

        // Check that process has a PID (indicating it's running)
        assert!(handle.child.id().is_some());

        // Terminate the process
        runtime.terminate_process(&mut handle).await.unwrap();

        // Process should no longer be running - wait should return immediately
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        // After termination, the handle should not have a valid child process anymore
        // This is tested by checking if wait() fails or succeeds with an exit status
        let wait_result = runtime.wait_for_exit(&mut handle).await;
        assert!(
            wait_result.is_ok(),
            "Process should have exited after termination"
        );
    }
}
