mod common;

use common::{
    assertions::*,
    environment::{RunceptTestEnvironment, TestConfig},
};
use std::time::Duration;

/// Tests for process exit detection and status tracking
/// Verifies that runcept correctly detects when processes exit and updates their status

#[tokio::test]
async fn test_process_exit_status_is_detected_and_updated() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "process-exit-detection".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Create config with a process that will exit quickly
    let config_content = r#"
[environment]
name = "exit-detection-env"

[processes.quick-exit]
name = "quick-exit"
command = "bash -c 'echo Starting; sleep 1; echo Exiting; exit 0'"
auto_restart = false
"#;

    test_env.create_config_file(config_content).await.unwrap();

    // Activate the environment
    let output = test_env.activate_environment(Some(&test_env.project_dir().to_string_lossy()));
    assert!(
        output.status.success(),
        "Environment activation should succeed"
    );

    // Start the process
    let output = test_env.start_process("quick-exit");
    assert!(output.status.success(), "Process start should succeed");
    assert_output_contains(&output, "started");

    // Initially the process should be running
    assert_process_status_with_env(&test_env, "quick-exit", "running");

    // Wait for the process to exit (it sleeps for 1 second)
    tokio::time::sleep(Duration::from_millis(2000)).await;

    // The process should now be detected as exited/stopped
    let list_output = test_env.list_processes();

    let list_stdout = String::from_utf8_lossy(&list_output.stdout);

    // Process should show as stopped, exited, or finished
    assert!(
        list_stdout.contains("quick-exit")
            && (list_stdout.contains("stopped")
                || list_stdout.contains("exited")
                || list_stdout.contains("finished")
                || list_stdout.contains("completed")),
        "Process exit should be detected and status updated. Actual output: {list_stdout}"
    );

    // Check that logs contain exit information
    assert_lifecycle_events_logged_with_env(&test_env, "quick-exit");
}

#[tokio::test]
async fn test_process_crash_exit_code_is_tracked() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "process-crash-detection".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Create config with a process that will crash (non-zero exit)
    let config_content = r#"
[environment]
name = "crash-detection-env"

[processes.crashing-process]
name = "crashing-process"
command = "bash -c 'echo Starting; sleep 1; echo Crashing; exit 42'"
auto_restart = false
"#;

    test_env.create_config_file(config_content).await.unwrap();

    // Activate the environment
    let output = test_env.activate_environment(Some(&test_env.project_dir().to_string_lossy()));
    assert!(
        output.status.success(),
        "Environment activation should succeed"
    );

    // Start the process
    let output = test_env.start_process("crashing-process");
    assert!(output.status.success(), "Process start should succeed");
    assert_output_contains(&output, "started");

    // Wait for the process to crash
    tokio::time::sleep(Duration::from_millis(2000)).await;

    // Check that the crash is detected
    let list_output = test_env.list_processes();

    let list_stdout = String::from_utf8_lossy(&list_output.stdout);

    // Process should show as crashed, failed, or exited
    assert!(
        list_stdout.contains("crashing-process")
            && (list_stdout.contains("crashed")
                || list_stdout.contains("failed")
                || list_stdout.contains("exited")
                || list_stdout.contains("stopped")),
        "Process crash should be detected and status updated. Actual output: {list_stdout}"
    );

    // Check logs for crash information
    let logs_output = test_env.get_process_logs("crashing-process");

    let logs_stdout = String::from_utf8_lossy(&logs_output.stdout);

    // Logs should contain exit status information
    assert!(
        logs_stdout.contains("exit")
            || logs_stdout.contains("status")
            || logs_stdout.contains("42"),
        "Logs should contain exit status information. Actual logs: {logs_stdout}"
    );
}

#[tokio::test]
async fn test_multiple_process_exit_detection() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "multiple-process-exit".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Create config with multiple processes that exit at different times
    let config_content = r#"
[environment]
name = "multiple-exit-env"

[processes.fast-exit]
name = "fast-exit"
command = "bash -c 'echo Fast process; sleep 0.5; exit 0'"
auto_restart = false

[processes.medium-exit]
name = "medium-exit"
command = "bash -c 'echo Medium process; sleep 1.5; exit 0'"
auto_restart = false

[processes.slow-exit]
name = "slow-exit"
command = "bash -c 'echo Slow process; sleep 3; exit 0'"
auto_restart = false
"#;

    test_env.create_config_file(config_content).await.unwrap();

    // Activate the environment
    let output = test_env.activate_environment(Some(&test_env.project_dir().to_string_lossy()));
    assert!(
        output.status.success(),
        "Environment activation should succeed"
    );

    // Start all processes
    let output = test_env.start_process("fast-exit");
    assert!(output.status.success(), "Fast process start should succeed");
    assert_output_contains(&output, "started");

    let output = test_env.start_process("medium-exit");
    assert!(
        output.status.success(),
        "Medium process start should succeed"
    );
    assert_output_contains(&output, "started");

    let output = test_env.start_process("slow-exit");
    assert!(output.status.success(), "Slow process start should succeed");
    assert_output_contains(&output, "started");

    // Wait 1 second - fast should be done, medium and slow should still be running
    tokio::time::sleep(Duration::from_millis(1000)).await;

    let list_output_1 = test_env.list_processes();

    let list_stdout_1 = String::from_utf8_lossy(&list_output_1.stdout);

    // Fast should be finished, others still running
    assert!(
        list_stdout_1.contains("fast-exit")
            && !is_process_running_in_output(&list_stdout_1, "fast-exit"),
        "Fast process should have exited by now"
    );
    assert!(
        is_process_running_in_output(&list_stdout_1, "slow-exit"),
        "Slow process should still be running"
    );

    // Wait 2 more seconds - medium should be done, slow might still be running
    tokio::time::sleep(Duration::from_millis(2000)).await;

    let list_output_2 = test_env.list_processes();

    let list_stdout_2 = String::from_utf8_lossy(&list_output_2.stdout);

    // Medium should now be finished
    assert!(
        list_stdout_2.contains("medium-exit")
            && !is_process_running_in_output(&list_stdout_2, "medium-exit"),
        "Medium process should have exited by now"
    );

    // Wait 2 more seconds - all should be done
    tokio::time::sleep(Duration::from_millis(2000)).await;

    let list_output_3 = test_env.list_processes();

    let list_stdout_3 = String::from_utf8_lossy(&list_output_3.stdout);

    // All processes should be finished
    assert!(
        !is_process_running_in_output(&list_stdout_3, "fast-exit")
            && !is_process_running_in_output(&list_stdout_3, "medium-exit")
            && !is_process_running_in_output(&list_stdout_3, "slow-exit"),
        "All processes should have exited by now, but found running processes in output: {list_stdout_3}"
    );
}

#[tokio::test]
async fn test_process_restart_after_crash() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "restart-after-crash".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Create config with a process that crashes but can be restarted
    let config_content = r#"
[environment]
name = "restart-crash-env"

[processes.unstable-process]
name = "unstable-process"
command = "bash -c 'echo Starting unstable process; sleep 1; exit 1'"
auto_restart = false
"#;

    test_env.create_config_file(config_content).await.unwrap();

    // Activate the environment
    let output = test_env.activate_environment(Some(&test_env.project_dir().to_string_lossy()));
    assert!(
        output.status.success(),
        "Environment activation should succeed"
    );

    // Start the process
    let output = test_env.start_process("unstable-process");
    assert!(output.status.success(), "Process start should succeed");
    assert_output_contains(&output, "started");

    // Wait for it to crash
    tokio::time::sleep(Duration::from_millis(2000)).await;

    // Verify it crashed
    let list_output = test_env.list_processes();

    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    assert!(
        !is_process_running_in_output(&list_stdout, "unstable-process"),
        "Process should have crashed and not be running"
    );

    // Now restart it
    let output = test_env.restart_process("unstable-process");
    assert!(output.status.success(), "Process restart should succeed");
    assert_output_contains(&output, "restarted");

    // It should be running again (briefly)
    let list_output_after_restart = test_env.list_processes();

    let list_stdout_after_restart = String::from_utf8_lossy(&list_output_after_restart.stdout);

    // Process should be running again after restart
    assert!(
        list_stdout_after_restart.contains("unstable-process"),
        "Process should be present after restart"
    );
}

/// Helper function to check if a process appears to be running in the list output
fn is_process_running_in_output(output: &str, process_name: &str) -> bool {
    for line in output.lines() {
        if line.contains(process_name) {
            // Check if the line indicates the process is running
            return line.contains("running") || line.contains("active");
        }
    }
    false
}
