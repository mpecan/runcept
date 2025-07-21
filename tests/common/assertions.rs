#![allow(dead_code)]
#![allow(unused_imports)]

use assert_cmd::Command;
use std::process::Output;
use std::time::Duration;

/// Custom assertion helpers for runcept integration tests

/// Assert that a command output contains text (case-insensitive)
pub fn assert_output_contains(output: &Output, expected: &str) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    assert!(
        combined.to_lowercase().contains(&expected.to_lowercase()),
        "Expected output to contain '{}'\nActual stdout: {}\nActual stderr: {}",
        expected,
        stdout,
        stderr
    );
}

/// Assert that a command succeeded and contains expected text
pub fn assert_success_with_output(mut cmd: Command, expected: &str) {
    let output = cmd.output().expect("Failed to execute command");
    assert!(
        output.status.success(),
        "Command failed with non-zero exit code, output: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_output_contains(&output, expected);
}

/// Assert that a command failed with expected error message
pub fn assert_failure_with_error(mut cmd: Command, expected_error: &str) {
    let output = cmd.output().expect("Failed to execute command");
    assert!(
        !output.status.success(),
        "Expected command to fail but it succeeded"
    );
    assert_output_contains(&output, expected_error);
}

/// Assert that a command succeeds within a timeout
pub fn assert_success_within_timeout(mut cmd: Command, timeout: Duration) {
    let assertion = cmd.timeout(timeout).assert();
    assertion.success();
}

/// Assert that a daemon is running by checking status command
pub fn assert_daemon_running(mut cmd: Command) {
    let output = cmd
        .args(["daemon", "status"])
        .output()
        .expect("Failed to execute daemon status command");

    assert!(
        output.status.success(),
        "Daemon status command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Running"),
        "Expected daemon to be running, got: {}",
        stdout
    );
}

/// Assert that a daemon is not running
pub fn assert_daemon_not_running(mut cmd: Command) {
    let output = cmd
        .args(["daemon", "status"])
        .output()
        .expect("Failed to execute daemon status command");

    // Either the command fails (daemon not running) or status shows not running
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Not running") || stdout.contains("Stopped"),
            "Expected daemon to not be running, got: {}",
            stdout
        );
    }
    // If status command fails, that's also acceptable (daemon not running)
}

/// Assert that a process is in the expected state
pub fn assert_process_status(mut cmd: Command, process_name: &str, expected_status: &str) {
    let output = cmd
        .args(["list"])
        .output()
        .expect("Failed to execute list command");

    assert!(
        output.status.success(),
        "List command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(process_name) && stdout.contains(expected_status),
        "Expected process '{}' to have status '{}'\nActual output: {}",
        process_name,
        expected_status,
        stdout
    );
}

/// Assert that log output contains lifecycle events
pub fn assert_lifecycle_events_logged(mut cmd: Command, process_name: &str) {
    let output = cmd
        .args(["logs", process_name])
        .output()
        .expect("Failed to execute logs command");

    assert!(
        output.status.success(),
        "Logs command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Check for lifecycle events
    let has_lifecycle = stdout.contains("Starting process")
        || stdout.contains("Stopping process")
        || stdout.contains("Process started")
        || stdout.contains("Process stopped");

    assert!(
        has_lifecycle,
        "Expected to find lifecycle events in logs for process '{}'\nActual output: {}",
        process_name, stdout
    );
}

/// Assert that health check timeout occurred
pub fn assert_health_check_timeout(mut cmd: Command, process_name: &str, timeout_seconds: u32) {
    let output = cmd
        .timeout(Duration::from_secs((timeout_seconds + 5) as u64))
        .output()
        .expect("Failed to execute command");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected_message = format!(
        "Health check timeout for process '{}' after {} seconds",
        process_name, timeout_seconds
    );

    assert!(
        stderr.contains(&expected_message) || stderr.contains("timeout"),
        "Expected health check timeout message, got: {}",
        stderr
    );
}

/// Assert that a socket file exists
pub fn assert_socket_exists(socket_path: &std::path::Path) {
    assert!(
        socket_path.exists(),
        "Expected socket file to exist at: {}",
        socket_path.display()
    );
}

/// Assert that a socket file does not exist
pub fn assert_socket_not_exists(socket_path: &std::path::Path) {
    assert!(
        !socket_path.exists(),
        "Expected socket file to not exist at: {}",
        socket_path.display()
    );
}

// Convenience assertion methods that work with test environment directly
use crate::common::environment::RunceptTestEnvironment;

/// Assert that a daemon is running using test environment
pub fn assert_daemon_running_with_env(test_env: &RunceptTestEnvironment) {
    let output = test_env.daemon_status();
    assert!(
        output.status.success(),
        "Daemon status command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Running"),
        "Expected daemon to be running, got: {stdout}"
    );
}

/// Assert that a daemon is not running using test environment
pub fn assert_daemon_not_running_with_env(test_env: &RunceptTestEnvironment) {
    let output = test_env.daemon_status();

    // Either the command fails (daemon not running) or status shows not running
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Not running") || stdout.contains("Stopped"),
            "Expected daemon to not be running, got: {stdout}"
        );
    }
    // If status command fails, that's also acceptable (daemon not running)
}

/// Assert that a process is in the expected state using test environment
pub fn assert_process_status_with_env(
    test_env: &RunceptTestEnvironment,
    process_name: &str,
    expected_status: &str,
) {
    let output = test_env.list_processes();
    assert!(
        output.status.success(),
        "List command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(process_name) && stdout.contains(expected_status),
        "Expected process '{process_name}' to have status '{expected_status}'\nActual output: {stdout}"
    );
}

/// Assert that log output contains lifecycle events using test environment
pub fn assert_lifecycle_events_logged_with_env(
    test_env: &RunceptTestEnvironment,
    process_name: &str,
) {
    let output = test_env.get_process_logs(process_name);
    assert!(
        output.status.success(),
        "Logs command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Check for lifecycle events
    let has_lifecycle = stdout.contains("Starting process")
        || stdout.contains("Stopping process")
        || stdout.contains("Process started")
        || stdout.contains("Process stopped");

    assert!(
        has_lifecycle,
        "Expected to find lifecycle events in logs for process '{process_name}'\nActual output: {stdout}"
    );
}
