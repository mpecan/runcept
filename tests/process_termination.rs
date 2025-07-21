mod common;

use common::{
    assertions::*,
    environment::{RunceptTestEnvironment, TestConfig},
};
use std::time::Duration;
use sysinfo::System;

/// Tests that verify processes are actually terminated at the system level
/// These tests use Unix signals to verify that stop/restart commands
/// actually kill processes in the operating system, not just update database state.

#[tokio::test]
async fn test_stop_command_actually_kills_system_process() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "process-termination".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Create config with a process that will run indefinitely
    let config_content = r#"
[environment]
name = "termination-test-env"

[processes.long-running]
name = "long-running"
command = "sleep 30"
auto_restart = false
"#;

    test_env.create_config_file(config_content).await.unwrap();

    // Activate the environment
    let activate_output =
        test_env.activate_environment(Some(&test_env.project_dir().to_string_lossy()));
    assert!(
        activate_output.status.success(),
        "Failed to activate environment"
    );

    // Start the process
    let start_output = test_env.start_process("long-running");
    assert!(start_output.status.success(), "Failed to start process");

    // Get the PID of the started process by checking the list
    let list_output = test_env.list_processes();
    assert!(list_output.status.success(), "Failed to list processes");

    let list_stdout = String::from_utf8_lossy(&list_output.stdout);

    // Extract PID from the list output
    let process_pid = extract_pid_from_list_output(&list_stdout, "long-running")
        .expect("Could not find PID for long-running process");

    // Verify the process is actually running in the system
    assert!(
        is_process_alive(process_pid),
        "Process should be alive after start"
    );

    // Stop the process
    let stop_output = test_env.stop_process("long-running");
    assert!(stop_output.status.success(), "Failed to stop process");

    // Wait a moment for the stop to take effect
    tokio::time::sleep(Duration::from_millis(500)).await;

    // CRITICAL TEST: Verify the process is actually killed in the system
    assert!(
        !is_process_alive(process_pid),
        "CRITICAL: Process PID {process_pid} is still alive after stop command. The stop command did not actually terminate the system process."
    );

    // Also verify through runcept that the process is stopped
    let status_output = test_env.status();
    assert!(
        status_output.status.success(),
        "Status command should succeed"
    );
    assert_output_contains(&status_output, "stopped");
}

#[tokio::test]
async fn test_restart_terminates_old_process_and_starts_new() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "process-restart-termination".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Create config with a long-running process
    let config_content = r#"
[environment]
name = "restart-termination-env"

[processes.restartable]
name = "restartable"
command = "sleep 60"
auto_restart = false
"#;

    test_env.create_config_file(config_content).await.unwrap();

    // Activate the environment
    let activate_output =
        test_env.activate_environment(Some(&test_env.project_dir().to_string_lossy()));
    assert!(
        activate_output.status.success(),
        "Failed to activate environment"
    );

    // Start the process
    let start_output = test_env.start_process("restartable");
    assert!(start_output.status.success(), "Start should succeed");
    assert_output_contains(&start_output, "started");

    // Get the initial PID
    let list_output = test_env.list_processes();
    assert!(list_output.status.success(), "Failed to list processes");

    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    let initial_pid = extract_pid_from_list_output(&list_stdout, "restartable")
        .expect("Could not find initial PID");

    assert!(
        is_process_alive(initial_pid),
        "Initial process should be alive"
    );

    // Restart the process
    let restart_output = test_env.restart_process("restartable");
    assert!(restart_output.status.success(), "Restart should succeed");
    assert_output_contains(&restart_output, "restarted");

    // Wait for restart to complete
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Get the new PID
    let new_list_output = test_env.list_processes();
    assert!(
        new_list_output.status.success(),
        "Failed to list processes after restart"
    );

    let new_list_stdout = String::from_utf8_lossy(&new_list_output.stdout);
    let new_pid = extract_pid_from_list_output(&new_list_stdout, "restartable")
        .expect("Could not find new PID after restart");

    // CRITICAL TESTS:
    // 1. Old process should be terminated
    assert!(
        !is_process_alive(initial_pid),
        "CRITICAL: Old process PID {initial_pid} is still alive after restart. The restart command did not terminate the old process."
    );

    // 2. New process should be running
    assert!(
        is_process_alive(new_pid),
        "CRITICAL: New process PID {new_pid} is not alive after restart. The restart command did not start a new process."
    );

    // 3. PIDs should be different (new process created)
    assert_ne!(
        initial_pid, new_pid,
        "CRITICAL: Process PID did not change after restart. The process was not actually restarted."
    );

    // Verify through runcept that the process is running
    let status_output = test_env.status();
    assert!(
        status_output.status.success(),
        "Status command should succeed"
    );
    assert_output_contains(&status_output, "running");
}

#[tokio::test]
async fn test_restart_after_stop_works_without_database_errors() {
    // This test ensures that restarting a previously stopped process
    // doesn't fail due to database constraint issues
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "restart-after-stop".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    let config_content = r#"
[environment]
name = "restart-after-stop-env"

[processes.stoppable-process]
name = "stoppable-process"
command = "sleep 10"
auto_restart = false
"#;

    test_env.create_config_file(config_content).await.unwrap();

    // Activate environment
    let activate_output =
        test_env.activate_environment(Some(&test_env.project_dir().to_string_lossy()));
    assert!(
        activate_output.status.success(),
        "Failed to activate environment"
    );

    // Start process
    let start_output = test_env.start_process("stoppable-process");
    assert!(start_output.status.success(), "Start should succeed");
    assert_output_contains(&start_output, "started");

    // Stop process
    let stop_output = test_env.stop_process("stoppable-process");
    assert!(stop_output.status.success(), "Stop should succeed");
    assert_output_contains(&stop_output, "stopped");

    // Try to restart the stopped process - this should work without database errors
    let restart_output = test_env.restart_process("stoppable-process");
    assert!(restart_output.status.success(), "Restart should succeed");
    assert_output_contains(&restart_output, "restarted");

    // Verify the process is running
    let status_output = test_env.status();
    assert!(
        status_output.status.success(),
        "Status command should succeed"
    );
    assert_output_contains(&status_output, "running");
}

/// Helper function to check if a process is alive using sysinfo
fn is_process_alive(pid: i32) -> bool {
    let system = System::new_all();
    system.process(sysinfo::Pid::from_u32(pid as u32)).is_some()
}

/// Helper function to extract PID from runcept list output
fn extract_pid_from_list_output(output: &str, process_name: &str) -> Option<i32> {
    for line in output.lines() {
        if line.contains(process_name) {
            println!("{line}");
            // Parse the line to extract PID
            // Assumes format like: "process_name    running   12345    ..."
            let parts: Vec<&str> = line.split_whitespace().collect();
            for (i, part) in parts.iter().enumerate() {
                if part == &process_name {
                    // Look for PID in subsequent columns
                    for part in parts.iter().skip(i + 1) {
                        if let Ok(pid) = part.parse::<i32>() {
                            // Sanity check - PID should be reasonable
                            if pid > 0 && pid < 99999 {
                                return Some(pid);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}
