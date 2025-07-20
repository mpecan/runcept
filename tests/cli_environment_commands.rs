mod common;

use common::{
    assertions::*,
    environment::{RunceptTestEnvironment, TestConfig},
    fixtures::*,
};
use std::time::Duration;

/// Tests for CLI environment management commands (activate, deactivate, status)
/// Verifies that environment activation/deactivation works correctly across daemon restarts

#[tokio::test]
async fn test_environment_activation_and_deactivation() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "environment-activation".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    test_env
        .create_config_file(basic_test_config())
        .await
        .unwrap();

    // Test activation
    let mut cmd = test_env.runcept_cmd();
    cmd.args(["activate", &test_env.project_dir().to_string_lossy()]);
    assert_success_with_output(cmd, "activated");

    // Test status shows active environment
    let status_output = test_env
        .runcept_cmd()
        .args(["status"])
        .output()
        .expect("Failed to get status");

    assert!(
        status_output.status.success(),
        "Status command should succeed"
    );

    let status_stdout = String::from_utf8_lossy(&status_output.stdout);
    assert!(
        status_stdout.contains("test-env") || status_stdout.contains("environment"),
        "Status should show active environment. Actual output: {}",
        status_stdout
    );

    // Test deactivation
    let mut cmd = test_env.runcept_cmd();
    cmd.args(["deactivate"]);
    assert_success_with_output(cmd, "deactivated");

    // Test status shows no active environment after deactivation
    let status_after_deactivate = test_env
        .runcept_cmd()
        .args(["status"])
        .output()
        .expect("Failed to get status after deactivate");

    let status_after_stdout = String::from_utf8_lossy(&status_after_deactivate.stdout);
    assert!(
        !status_after_stdout.contains("test-env")
            || status_after_stdout.contains("no active")
            || status_after_stdout.contains("not activated"),
        "Status should show no active environment after deactivation"
    );
}

#[tokio::test]
async fn test_environment_activation_with_invalid_path() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "invalid-environment".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Test activating non-existent environment
    let mut cmd = test_env.runcept_cmd();
    cmd.args(["activate", "/non/existent/path"]);
    assert_failure_with_error(
        cmd,
        "Failed to activate environment: Configuration error: No .runcept.toml found in or above",
    );

    // Test activating directory without .runcept.toml
    let empty_dir = test_env.project_dir().join("empty_project");
    tokio::fs::create_dir_all(&empty_dir).await.unwrap();

    let mut cmd = test_env.runcept_cmd();
    cmd.args(["activate", &empty_dir.to_string_lossy()]);
    assert_failure_with_error(cmd, "No .runcept.toml");

    // Test invalid environment path with helpful error
    let mut cmd = test_env.runcept_cmd();
    cmd.args(["start", "worker", "--environment", "/nonexistent/path"]);
    assert_failure_with_error(
        cmd,
        "Failed to execute command: Environment error: Invalid environment path",
    );
}

#[tokio::test]
async fn test_daemon_persistence_across_restarts() {
    let mut test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "daemon-persistence".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    test_env
        .create_config_file(basic_test_config())
        .await
        .unwrap();

    // First daemon instance - activate environment
    let mut cmd = test_env.runcept_cmd();
    cmd.args(["activate", &test_env.project_dir().to_string_lossy()]);
    assert_success_with_output(cmd, "activated");

    // Manually stop the daemon and start a new one to test persistence
    test_env.stop_daemon();
    test_env.start_daemon().await;

    // Check that we can reactivate the same environment after daemon restart
    let mut cmd = test_env.runcept_cmd();
    cmd.args(["activate", &test_env.project_dir().to_string_lossy()]);
    assert_success_with_output(cmd, "activated");

    // Verify the environment is active
    let mut cmd = test_env.runcept_cmd();
    cmd.args(["status"]);
    assert_success_with_output(cmd, "environment");
}

#[tokio::test]
async fn test_environment_enforcement_for_process_commands() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "environment-enforcement".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Test that process commands fail when no environment is available
    let mut cmd = test_env.runcept_cmd();
    cmd.args(["start", "worker"]);
    assert_failure_with_error(cmd, "No .runcept.toml configuration found");

    // Test that list command fails when no environment is available
    let mut cmd = test_env.runcept_cmd();
    cmd.args(["list"]);
    assert_failure_with_error(cmd, "No .runcept.toml configuration found");

    // Now create the environment and test that commands work with --environment flag
    test_env
        .create_config_file(basic_test_config())
        .await
        .unwrap();

    // Activate the environment first
    let mut cmd = test_env.runcept_cmd();
    cmd.args(["activate", &test_env.project_dir().to_string_lossy()]);
    assert_success_with_output(cmd, "activated");

    // Test that --environment flag works even when environment is already active
    let mut cmd = test_env.runcept_cmd();
    cmd.args([
        "start",
        "test-process",
        "--environment",
        &test_env.project_dir().to_string_lossy(),
    ]);
    assert_success_with_output(cmd, "started");

    // Test list with environment override
    let list_output = test_env
        .runcept_cmd()
        .args([
            "list",
            "--environment",
            &test_env.project_dir().to_string_lossy(),
        ])
        .output()
        .expect("Failed to list processes with environment override");

    assert!(
        list_output.status.success(),
        "List with environment override should succeed"
    );

    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    assert!(
        list_stdout.contains("test-process"),
        "Should show test-process"
    );

    // Stop process with environment override
    let mut cmd = test_env.runcept_cmd();
    cmd.args([
        "stop",
        "test-process",
        "--environment",
        &test_env.project_dir().to_string_lossy(),
    ]);
    assert_success_with_output(cmd, "stopped");
}

#[tokio::test]
async fn test_environment_activation_without_daemon() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "activation-no-daemon".to_string(),
        enable_logging: true,
        auto_start_daemon: false, // Don't auto-start daemon for this test
        ..TestConfig::default()
    })
    .await;

    test_env
        .create_config_file(basic_test_config())
        .await
        .unwrap();

    // Test activation when daemon is not running
    let activate_output = test_env
        .runcept_cmd()
        .args(["activate", &test_env.project_dir().to_string_lossy()])
        .output()
        .expect("Failed to execute activate command");

    // Should either start daemon automatically or fail gracefully
    if activate_output.status.success() {
        // If activation succeeds, daemon should have auto-started
        let mut cmd = test_env.runcept_cmd();
        cmd.args(["daemon", "status"]);
        assert_success_with_output(cmd, "running");
    } else {
        // If activation fails, should provide helpful error message
        let stderr = String::from_utf8_lossy(&activate_output.stderr);
        assert!(
            stderr.contains("daemon") || stderr.contains("connection"),
            "Should provide meaningful error about daemon connection"
        );
    }
}

#[tokio::test]
async fn test_status_command_without_active_environment() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "status-no-env".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Test status when no environment is active
    let status_output = test_env
        .runcept_cmd()
        .args(["status"])
        .output()
        .expect("Failed to get status");

    assert!(
        status_output.status.success(),
        "Status command should succeed even without active environment"
    );

    let status_stdout = String::from_utf8_lossy(&status_output.stdout);
    assert!(
        status_stdout.contains("No active environment"),
        "Status should indicate no active environment. Actual output: {}",
        status_stdout
    );
}

#[tokio::test]
async fn test_environment_switching() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "environment-switching".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Create first environment
    let env1_config = r#"
[environment]
name = "first-env"

[processes.env1-process]
name = "env1-process"
command = "echo env1"
auto_restart = false
"#;
    test_env.create_config_file(env1_config).await.unwrap();

    // Create second environment
    let env2_dir = test_env.project_dir().join("env2");
    tokio::fs::create_dir_all(&env2_dir).await.unwrap();

    let env2_config = r#"
[environment]
name = "second-env"

[processes.env2-process]
name = "env2-process"
command = "echo env2"
auto_restart = false
"#;
    tokio::fs::write(env2_dir.join(".runcept.toml"), env2_config)
        .await
        .unwrap();

    // Activate first environment
    let mut cmd = test_env.runcept_cmd();
    cmd.args(["activate", &test_env.project_dir().to_string_lossy()]);
    assert_success_with_output(cmd, "activated");

    // Verify first environment is active
    let mut cmd = test_env.runcept_cmd();
    cmd.args(["status"]);
    assert_success_with_output(cmd, "first-env");

    // Switch to second environment
    let mut cmd = test_env.runcept_cmd();
    cmd.args(["activate", &env2_dir.to_string_lossy()]);
    assert_success_with_output(cmd, "activated");

    // Verify second environment is now active
    let mut cmd = test_env.runcept_cmd();
    cmd.args(["status"]);
    assert_success_with_output(cmd, "second-env");

    // Test that process commands work with the new environment
    let list_output = test_env
        .runcept_cmd()
        .args(["list", "--environment", &env2_dir.to_string_lossy()])
        .output()
        .expect("Failed to list processes");

    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    assert!(
        list_stdout.contains("env2-process"),
        "Should show processes from second environment, actual output: {}",
        list_stdout
    );
}

#[tokio::test]
async fn test_database_functionality_with_environment() {
    let mut test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "database-functionality".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    test_env
        .create_config_file(basic_test_config())
        .await
        .unwrap();

    // Activate environment to trigger database operations
    let mut cmd = test_env.runcept_cmd();
    cmd.args(["activate", &test_env.project_dir().to_string_lossy()]);
    assert_success_with_output(cmd, "activated");

    // Verify environment is active
    let mut cmd = test_env.runcept_cmd();
    cmd.args(["status"]);
    assert_success_with_output(cmd, "test-env");

    // Check that runcept directory exists and has expected structure
    let runcept_dir = test_env.home_dir().join(".runcept");
    assert!(runcept_dir.exists(), "Runcept directory should exist");
    assert!(
        runcept_dir.join("logs").exists(),
        "Logs directory should exist"
    );
    assert!(
        runcept_dir.join("daemon.sock").exists(),
        "Daemon socket should exist"
    );

    // Stop daemon and verify directory persists
    test_env.stop_daemon();

    // Runcept directory should persist after shutdown
    assert!(
        runcept_dir.exists(),
        "Runcept directory should persist after daemon shutdown"
    );
}
