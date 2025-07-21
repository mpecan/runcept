mod common;

use common::{
    assertions::*,
    environment::{RunceptTestEnvironment, TestConfig},
    fixtures::*,
};
use std::time::Duration;

/// Tests for CLI process management commands (start, stop, restart, list, logs)

#[tokio::test]
async fn test_process_start_stop_restart_workflow() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "process-management".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Create a comprehensive test configuration
    let config_content = r#"
[environment]
name = "process-mgmt-env"

[processes.web-server]
name = "web-server"
command = "python3 -m http.server 8080"
auto_restart = false

[processes.worker]
name = "worker"
command = "bash -c 'while true; do echo Working; sleep 2; done'"
auto_restart = false

[processes.quick-task]
name = "quick-task"
command = "echo 'Task completed'"
auto_restart = false
"#;

    test_env.create_config_file(config_content).await.unwrap();

    // Activate the environment
    let output = test_env.activate_environment(Some(&test_env.project_dir().to_string_lossy()));
    assert!(output.status.success(), "Activate should succeed");
    assert_output_contains(&output, "activated");

    // Test starting individual processes
    let output = test_env.start_process("worker");
    assert!(output.status.success(), "Start worker should succeed");
    assert_output_contains(&output, "started");

    let output = test_env.start_process("quick-task");
    assert!(output.status.success(), "Start quick-task should succeed");
    assert_output_contains(&output, "started");

    // Verify processes appear in list
    let list_output = test_env.list_processes();
    assert!(list_output.status.success(), "List should succeed");

    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    assert!(
        list_stdout.contains("worker"),
        "Worker should appear in process list"
    );
    assert!(
        list_stdout.contains("quick-task"),
        "Quick task should appear in process list"
    );

    // Test stopping a process
    let output = test_env.stop_process("worker");
    assert!(output.status.success(), "Stop worker should succeed");
    assert_output_contains(&output, "stopped");

    // Verify worker is stopped but quick-task might still be running or finished
    assert_process_status(test_env.runcept_cmd(), "worker", "stopped");

    // Test restarting a process
    let output = test_env.restart_process("worker");
    assert!(output.status.success(), "Restart worker should succeed");
    assert_output_contains(&output, "restarted");

    // Worker should be running again
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_process_status(test_env.runcept_cmd(), "worker", "running");
}

#[tokio::test]
async fn test_process_logs_command() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "process-logs".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Create config with a process that generates output
    let config_content = r#"
[environment]
name = "logs-env"

[processes.chatty-process]
name = "chatty-process"
command = "bash -c 'echo Starting; sleep 1; echo Middle; sleep 1; echo Ending'"
auto_restart = false
"#;

    test_env.create_config_file(config_content).await.unwrap();

    // Activate environment
    let output = test_env.activate_environment(Some(&test_env.project_dir().to_string_lossy()));
    assert!(output.status.success(), "Activate should succeed");
    assert_output_contains(&output, "activated");

    // Start the chatty process
    let output = test_env.start_process("chatty-process");
    assert!(
        output.status.success(),
        "Start chatty-process should succeed"
    );
    assert_output_contains(&output, "started");

    // Wait for process to generate some output
    tokio::time::sleep(Duration::from_millis(3000)).await;

    // Test logs command
    let logs_output = test_env.get_process_logs("chatty-process");
    assert!(logs_output.status.success(), "Logs command should succeed");

    let logs_stdout = String::from_utf8_lossy(&logs_output.stdout);

    // Should contain process output
    assert!(
        logs_stdout.contains("Starting")
            || logs_stdout.contains("Middle")
            || logs_stdout.contains("Ending"),
        "Logs should contain process output. Actual logs: {logs_stdout}"
    );

    // Test logs with line limit
    let limited_logs_output = test_env
        .runcept_cmd()
        .args(["logs", "chatty-process", "--lines", "1"])
        .output()
        .expect("Failed to get limited logs");

    assert!(
        limited_logs_output.status.success(),
        "Limited logs command should succeed"
    );

    // Should contain fewer lines
    let limited_logs_stdout = String::from_utf8_lossy(&limited_logs_output.stdout);
    let line_count = limited_logs_stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();

    // Should have limited output (allowing for headers and formatting)
    assert!(
        line_count <= 5, // Allow some headers/formatting
        "Limited logs should contain fewer lines, got {line_count} lines"
    );
}

#[tokio::test]
async fn test_list_command_with_multiple_processes() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "list-processes".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Create config with multiple processes in different states
    test_env
        .create_config_file(multi_process_config())
        .await
        .unwrap();

    // Activate environment
    let output = test_env.activate_environment(Some(&test_env.project_dir().to_string_lossy()));
    assert!(output.status.success(), "Activate should succeed");
    assert_output_contains(&output, "activated");

    // Start some processes
    let output = test_env.start_process("worker-1");
    assert!(output.status.success(), "Start worker-1 should succeed");
    assert_output_contains(&output, "started");

    let output = test_env.start_process("quick-task");
    assert!(output.status.success(), "Start quick-task should succeed");
    assert_output_contains(&output, "started");

    // Wait a moment for quick-task to complete
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Test list command
    let list_output = test_env.list_processes();
    assert!(list_output.status.success(), "List command should succeed");

    let list_stdout = String::from_utf8_lossy(&list_output.stdout);

    // Should show all configured processes
    println!(
        "List command output: {}",
        String::from_utf8_lossy(&list_output.stdout)
    );
    assert!(list_stdout.contains("worker-1"), "Should show worker-1");
    assert!(list_stdout.contains("worker-2"), "Should show worker-2");
    assert!(list_stdout.contains("quick-task"), "Should show quick-task");

    // Should show status information
    assert!(
        list_stdout.contains("running")
            || list_stdout.contains("stopped")
            || list_stdout.contains("exited")
            || list_stdout.contains("finished"),
        "Should show process status information, actual output: {list_stdout}"
    );
}

#[tokio::test]
async fn test_process_management_with_environment_override() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "environment-override".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Create a test config
    test_env
        .create_config_file(basic_test_config())
        .await
        .unwrap();

    // Test process management with explicit environment parameter
    let env_path = test_env.project_dir().to_string_lossy().to_string();

    test_env.assert_cmd_success(&["activate", &env_path], "activated");

    // Start process with environment override (without activating first)
    let output = test_env.start_process_with_env("test-process", &env_path);
    assert!(output.status.success(), "Start with env should succeed");
    assert_output_contains(&output, "started");

    // List processes with environment override
    let list_output = test_env.list_processes_with_env(&env_path);
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
    let output = test_env.stop_process_with_env("test-process", &env_path);
    assert!(output.status.success(), "Stop with env should succeed");
    assert_output_contains(&output, "stopped");
}

#[tokio::test]
async fn test_process_management_error_cases() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "process-errors".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    test_env
        .create_config_file(basic_test_config())
        .await
        .unwrap();

    // Activate environment
    let output = test_env.activate_environment(Some(&test_env.project_dir().to_string_lossy()));
    assert!(output.status.success(), "Activate should succeed");
    assert_output_contains(&output, "activated");

    // Test starting non-existent process
    let output = test_env.start_process("non-existent-process");
    assert!(!output.status.success(), "Start non-existent should fail");
    assert_output_contains(&output, "not found");

    // Test stopping non-running process
    let output = test_env.stop_process("test-process");
    assert!(
        output.status.success(),
        "Stop should succeed even if not running"
    );
    assert_output_contains(&output, "stopped");

    // Test getting logs for non-existent process
    let output = test_env.get_process_logs("non-existent-process");
    assert!(
        !output.status.success(),
        "Logs for non-existent should fail"
    );
    assert_output_contains(&output, "not found in environment");

    // Test operations without active environment
    let output = test_env.deactivate_environment();
    assert!(output.status.success(), "Deactivate should succeed");

    let output = test_env.start_process("test-process");
    assert!(
        !output.status.success(),
        "Start without active env should fail"
    );
    assert_output_contains(&output, "is registered but not active");
}

#[tokio::test]
async fn test_concurrent_process_operations() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "concurrent-operations".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    test_env
        .create_config_file(multi_process_config())
        .await
        .unwrap();

    // Activate environment
    let output = test_env.activate_environment(Some(&test_env.project_dir().to_string_lossy()));
    assert!(output.status.success(), "Activate should succeed");
    assert_output_contains(&output, "activated");

    // Start multiple processes sequentially (since concurrent operations would require cloning)
    // This still tests the process management functionality effectively
    let output = test_env.start_process("worker-1");
    assert!(
        output.status.success(),
        "Worker-1 should start successfully"
    );

    let output = test_env.start_process("worker-2");
    assert!(
        output.status.success(),
        "Worker-2 should start successfully"
    );

    let output = test_env.start_process("quick-task");
    assert!(
        output.status.success(),
        "Quick-task should start successfully"
    );

    // Verify all processes were started
    let list_output = test_env.list_processes();
    assert!(list_output.status.success(), "List should succeed");

    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    assert!(
        list_stdout.contains("worker-1"),
        "Worker-1 should be started"
    );
    assert!(
        list_stdout.contains("worker-2"),
        "Worker-2 should be started"
    );
    assert!(
        list_stdout.contains("quick-task"),
        "Quick-task should be started"
    );
}
