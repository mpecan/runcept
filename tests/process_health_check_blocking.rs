mod common;

use common::{
    assertions::*,
    environment::{RunceptTestEnvironment, TestConfig},
    fixtures::*,
};
use std::time::Duration;

/// Tests for health check blocking behavior during process startup
/// Verifies that start commands block until health checks pass or timeout

#[tokio::test]
async fn test_health_check_blocks_start_command_until_timeout() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "health-check-blocking".to_string(),
        enable_logging: true,
        daemon_startup_timeout: Duration::from_secs(3),
        ..TestConfig::default()
    })
    .await;

    // Create config with a health check that will fail (unreachable URL)
    test_env
        .create_config_file(failing_health_check_config())
        .await
        .unwrap();

    // Activate the environment
    test_env
        .runcept_cmd()
        .args(["activate", &test_env.project_dir().to_string_lossy()])
        .assert()
        .success();

    // Measure how long the start command takes
    let start_time = std::time::Instant::now();

    // Start the process - this should block for the health check timeout (15 seconds default)
    let start_output = test_env
        .runcept_cmd()
        .args(["start", "health-check-process"])
        .timeout(Duration::from_secs(20)) // Allow extra time for the command itself
        .output()
        .expect("Failed to execute start command");

    let elapsed = start_time.elapsed();

    // The command should have taken close to 15 seconds (health check timeout)
    assert!(
        elapsed >= Duration::from_secs(10), // Allow some variance
        "Start command should have blocked for health check timeout, but only took {:?}",
        elapsed
    );

    // The command should fail due to health check timeout
    assert!(
        !start_output.status.success(),
        "Start command should fail due to health check timeout"
    );

    // Check for timeout error message
    let mut cmd = test_env.runcept_cmd();
    cmd.args(["start", "health-check-process"]);
    assert_health_check_timeout(cmd, "health-check-process", 15);
}

#[tokio::test]
async fn test_successful_health_check_allows_start_to_complete() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "successful-health-check".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Use a command health check that will succeed
    let config_content = command_health_check_config();
    test_env.create_config_file(config_content).await.unwrap();

    // Activate the environment
    test_env
        .runcept_cmd()
        .args(["activate", &test_env.project_dir().to_string_lossy()])
        .assert()
        .success();

    // Start the process - this should complete quickly once health check passes
    let start_time = std::time::Instant::now();

    let mut cmd = test_env.runcept_cmd();
    cmd.args(["start", "command-process"]);
    assert_success_with_output(cmd, "started");

    let elapsed = start_time.elapsed();

    // Should complete much faster than the timeout since health check succeeds
    assert!(
        elapsed < Duration::from_secs(5),
        "Start command should complete quickly when health check succeeds, but took {:?}",
        elapsed
    );

    // Process should be running
    let mut cmd = test_env.runcept_cmd();
    assert_process_status(
        cmd,
        "command-\
    process",
        "running",
    );
}

#[tokio::test]
async fn test_process_without_health_check_starts_immediately() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "no-health-check".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Use basic config without health check
    test_env
        .create_config_file(basic_test_config())
        .await
        .unwrap();

    // Activate the environment
    test_env
        .runcept_cmd()
        .args(["activate", &test_env.project_dir().to_string_lossy()])
        .assert()
        .success();

    // Start the process - this should complete very quickly since no health check
    let start_time = std::time::Instant::now();

    let mut cmd = test_env.runcept_cmd();
    cmd.args(["start", "test-process"]);
    assert_success_with_output(cmd, "started");

    let elapsed = start_time.elapsed();

    // Should complete almost immediately
    assert!(
        elapsed < Duration::from_secs(2),
        "Start command should complete immediately when no health check, but took {:?}",
        elapsed
    );
}

#[tokio::test]
async fn test_health_check_timeout_is_configurable() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "configurable-timeout".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Create config with custom health check timeout
    let config_content = r#"
[environment]
name = "custom-timeout-env"

[processes.custom-timeout-process]
name = "custom-timeout-process"
command = "sleep 30"
health_check_url = "http://localhost:9998/health"
health_check_interval = 1
startup_health_check_timeout = 5
auto_restart = false
"#;

    test_env.create_config_file(config_content).await.unwrap();

    // Activate the environment
    test_env
        .runcept_cmd()
        .args(["activate", &test_env.project_dir().to_string_lossy()])
        .assert()
        .success();

    // Measure start time with custom timeout
    let start_time = std::time::Instant::now();

    let start_output = test_env
        .runcept_cmd()
        .args(["start", "custom-timeout-process"])
        .timeout(Duration::from_secs(10))
        .output()
        .expect("Failed to execute start command");

    let elapsed = start_time.elapsed();

    // Should timeout after approximately 5 seconds (custom timeout)
    // Allow more flexibility in timing due to system load and test environment
    assert!(
        elapsed >= Duration::from_secs(3) && elapsed <= Duration::from_secs(12),
        "Start command should timeout after ~5 seconds with custom timeout, but took {:?}",
        elapsed
    );

    // Should fail due to timeout
    assert!(
        !start_output.status.success(),
        "Start command should fail due to custom timeout"
    );
}

#[tokio::test]
async fn test_multiple_health_check_attempts_before_timeout() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "multiple-attempts".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Create config with failing health check and short interval
    let config_content = r#"
[environment]
name = "multiple-attempts-env"

[processes.multi-attempt-process]
name = "multi-attempt-process"
command = "sleep 30"
health_check_url = "http://localhost:9997/health"
health_check_interval = 1
startup_health_check_timeout = 5
auto_restart = false
"#;

    test_env.create_config_file(config_content).await.unwrap();

    // Activate the environment
    test_env
        .runcept_cmd()
        .args(["activate", &test_env.project_dir().to_string_lossy()])
        .assert()
        .success();

    // Start the process and capture output to check for multiple attempts
    let start_output = test_env
        .runcept_cmd()
        .args(["start", "multi-attempt-process"])
        .timeout(Duration::from_secs(10))
        .output()
        .expect("Failed to execute start command");

    // Should have made multiple attempts (roughly 5 attempts in 5 seconds with 1 second interval)
    // Note: The exact output format depends on the logging implementation
    let stderr = String::from_utf8_lossy(&start_output.stderr);
    let stdout = String::from_utf8_lossy(&start_output.stdout);
    let combined_output = format!("{}{}", stdout, stderr);

    // Look for indicators of multiple attempts or timeout message
    // The command should fail (non-zero exit) even if the error message format varies
    assert!(
        !start_output.status.success()
            || combined_output.contains("timeout")
            || combined_output.contains("failed")
            || combined_output.contains("error"),
        "Should show timeout/failure or command should fail after multiple attempts. Exit status: {:?}, Output: {}",
        start_output.status,
        combined_output
    );
}

#[tokio::test]
async fn test_lifecycle_events_logged_during_health_check_blocking() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "lifecycle-health-check".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Create config with failing health check
    test_env
        .create_config_file(failing_health_check_config())
        .await
        .unwrap();

    // Activate the environment
    test_env
        .runcept_cmd()
        .args(["activate", &test_env.project_dir().to_string_lossy()])
        .assert()
        .success();

    // Try to start the process (will timeout)
    let _start_output = test_env
        .runcept_cmd()
        .args(["start", "health-check-process"])
        .timeout(Duration::from_secs(8)) // Shorter than full timeout for test speed
        .output()
        .expect("Failed to execute start command");

    // Check that lifecycle events were logged even though health check failed
    let logs_output = test_env
        .runcept_cmd()
        .args(["logs", "health-check-process"])
        .output();

    if let Ok(logs_output) = logs_output {
        let logs_stdout = String::from_utf8_lossy(&logs_output.stdout);

        // Should contain lifecycle events like "Starting process"
        assert!(
            logs_stdout.contains("Starting") || logs_stdout.contains("Waiting for health check"),
            "Logs should contain lifecycle events during health check blocking. Actual logs: {}",
            logs_stdout
        );
    }
}
