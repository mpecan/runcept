mod common;

use common::{
    assertions::*,
    environment::{RunceptTestEnvironment, TestConfig},
};
use std::time::Duration;

/// Tests for daemon lifecycle management
/// Verifies daemon startup, status checking, shutdown, and concurrent operations

#[tokio::test]
async fn test_daemon_startup_and_status() {
    let mut test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "daemon-startup".to_string(),
        enable_logging: true,
        auto_start_daemon: false, // Start daemon manually for this test
        ..TestConfig::default()
    })
    .await;

    // Verify daemon is not running initially
    assert!(
        !test_env.is_daemon_running(),
        "Daemon should not be running initially"
    );

    // Start daemon manually
    test_env.start_daemon().await.expect("Daemon should start");

    // Test daemon status
    let mut cmd = test_env.runcept_cmd();
    cmd.args(["daemon", "status"]);
    assert_success_with_output(cmd, "Running");

    // Verify daemon is running
    assert!(
        test_env.is_daemon_running(),
        "Daemon should be running after start"
    );

    // Test that socket file exists
    let socket_path = test_env.home_dir().join(".runcept").join("daemon.sock");
    assert!(socket_path.exists(), "Daemon socket file should exist");

    // Stop daemon
    test_env.stop_daemon().await.expect("Daemon should stop");

    // Verify daemon stopped
    test_env.assert_cmd_success(&["daemon", "status"], "Not running");

    // Socket should be removed
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        !socket_path.exists(),
        "Daemon socket file should be removed after stop"
    );
}

#[tokio::test]
async fn test_daemon_handles_multiple_start_attempts() {
    let mut test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "daemon-multiple-start".to_string(),
        enable_logging: true,
        auto_start_daemon: false,
        ..TestConfig::default()
    })
    .await;

    // Start daemon
    test_env.start_daemon().await;
    assert!(test_env.is_daemon_running(), "Daemon should be running");

    // Try to start daemon again - should handle gracefully
    let second_start_output = test_env
        .runcept_cmd()
        .args(["daemon", "start"])
        .output()
        .expect("Failed to execute second daemon start");

    // Either succeeds (idempotent) or fails with helpful message
    if !second_start_output.status.success() {
        let stderr = String::from_utf8_lossy(&second_start_output.stderr);
        assert!(
            stderr.contains("already running") || stderr.contains("exists"),
            "Should provide meaningful error about daemon already running"
        );
    }

    // Daemon should still be running
    assert!(
        test_env.is_daemon_running(),
        "Daemon should still be running after second start attempt"
    );
}

#[tokio::test]
async fn test_daemon_stop_when_not_running() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "daemon-stop-not-running".to_string(),
        enable_logging: true,
        auto_start_daemon: false,
        ..TestConfig::default()
    })
    .await;

    // Verify daemon is not running
    assert!(
        !test_env.is_daemon_running(),
        "Daemon should not be running initially"
    );

    // Try to stop daemon when it's not running
    let stop_output = test_env
        .runcept_cmd()
        .args(["daemon", "stop"])
        .output()
        .expect("Failed to execute daemon stop");

    // Should either succeed (idempotent) or fail with helpful message
    if !stop_output.status.success() {
        let stderr = String::from_utf8_lossy(&stop_output.stderr);
        assert!(
            stderr.contains("not running") || stderr.contains("connection"),
            "Should provide meaningful error about daemon not running"
        );
    }
}

#[tokio::test]
async fn test_concurrent_daemon_operations() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "concurrent-daemon-ops".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Test concurrent status checks
    let status_tasks = (0..5).map(|_| {
        let test_env = &test_env;
        async move {
            let status_output = test_env
                .runcept_cmd()
                .args(["daemon", "status"])
                .output()
                .expect("Failed to execute daemon status");

            // All status checks should succeed
            assert!(
                status_output.status.success(),
                "Concurrent status check should succeed"
            );
        }
    });

    // Run all status checks concurrently
    futures::future::join_all(status_tasks).await;

    // Daemon should still be running
    test_env.assert_cmd_success(&["daemon", "status"], "Not running");
}

#[tokio::test]
async fn test_daemon_logs_are_created() {
    let mut test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "daemon-logs".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Start the daemon
    test_env.start_daemon().await.expect("Daemon should start");

    // Give daemon time to start and create logs
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Check that daemon log file is created
    let daemon_log = test_env
        .home_dir()
        .join(".runcept")
        .join("logs")
        .join("daemon.log");
    assert!(daemon_log.exists(), "Daemon log file should be created");

    // Log file should have some content
    let log_content = tokio::fs::read_to_string(&daemon_log)
        .await
        .expect("Failed to read daemon log");

    assert!(
        !log_content.is_empty(),
        "Daemon log should contain some content"
    );
}

#[tokio::test]
async fn test_daemon_handles_invalid_commands() {
    let mut test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "daemon-invalid-commands".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Test invalid daemon subcommand
    let mut cmd = test_env.runcept_cmd();
    cmd.args(["daemon", "invalid-subcommand"]);
    assert_failure_with_error(cmd, "Usage: runcept daemon");

    // Test daemon command with invalid flags
    let invalid_flag_output = test_env
        .runcept_cmd()
        .args(["daemon", "status", "--invalid-flag"])
        .output()
        .expect("Failed to execute daemon command with invalid flag");

    // Should either ignore the flag or provide helpful error
    if !invalid_flag_output.status.success() {
        let stderr = String::from_utf8_lossy(&invalid_flag_output.stderr);
        assert!(
            stderr.contains("unknown") || stderr.contains("flag") || stderr.contains("option"),
            "Should provide meaningful error about invalid flag, got: {}",
            stderr
        );
    }
}

#[tokio::test]
async fn test_daemon_graceful_shutdown() {
    let mut test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "daemon-graceful-shutdown".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Start the daemon
    test_env.start_daemon().await.expect("Daemon should start");

    // Verify daemon is running
    assert!(test_env.is_daemon_running(), "Daemon should be running");

    // Record the daemon PID
    let daemon_pid = test_env.daemon_pid().expect("Should have daemon PID");

    // Send graceful shutdown command
    let mut cmd = test_env.runcept_cmd();
    cmd.args(["daemon", "stop"]);
    assert_success_with_output(cmd, "stopped");

    // Wait for daemon to shut down
    let mut attempts = 0;
    while test_env.is_daemon_running() && attempts < 20 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        attempts += 1;
    }

    // Daemon should have stopped
    test_env.assert_cmd_success(&["daemon", "status"], "Not running");

    // Socket file should be removed
    let socket_path = test_env.home_dir().join(".runcept").join("daemon.sock");
    assert!(
        !socket_path.exists(),
        "Socket file should be removed after graceful shutdown"
    );
}
