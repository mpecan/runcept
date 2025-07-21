mod common;

use common::{
    assertions::*,
    environment::{RunceptTestEnvironment, TestConfig},
};
use nix::sys::signal;
use nix::unistd::Pid;
use std::time::Duration;

/// Tests for process group management - ensuring parent processes and their children are properly managed

#[tokio::test]
async fn test_process_group_cleanup_kills_child_processes() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "process-group-management".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Create config with a parent process that spawns children
    let config_content = r#"
[environment]
name = "process-group-env"

[processes.parent-with-children]
name = "parent-with-children"
command = "bash -c 'echo Parent PID: $$; sleep 2 & echo Child1 PID: $!; sleep 4 & echo Child2 PID: $!; wait'"
auto_restart = false
"#;

    test_env.create_config_file(config_content).await.unwrap();

    // Activate the environment
    test_env
        .runcept_cmd()
        .args(["activate", &test_env.project_dir().to_string_lossy()])
        .assert()
        .success();

    // Start the parent process
    let start_output = test_env
        .runcept_cmd()
        .args(["start", "parent-with-children"])
        .output()
        .expect("Failed to start process");

    if !start_output.status.success() {
        println!("Start command failed!");
        println!("stdout: {}", String::from_utf8_lossy(&start_output.stdout));
        println!("stderr: {}", String::from_utf8_lossy(&start_output.stderr));
        panic!("Failed to start parent process");
    }

    // Wait a moment for the process to fully start
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Get the parent PID
    let list_output = test_env
        .runcept_cmd()
        .args(["list"])
        .output()
        .expect("Failed to list processes");

    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    
    let parent_pid = extract_pid_from_list_output(&list_stdout, "parent-with-children")
        .unwrap_or_else(|| {
            println!("Could not find PID for parent process 'parent-with-children'");
            println!("Full list output:\n{}", list_stdout);
            
            // Get process logs for debugging
            if let Ok(logs_output) = test_env
                .runcept_cmd()
                .args(["logs", "parent-with-children"])
                .output()
            {
                println!("Process logs:");
                println!("stdout: {}", String::from_utf8_lossy(&logs_output.stdout));
                println!("stderr: {}", String::from_utf8_lossy(&logs_output.stderr));
            }
            
            panic!("Could not find PID for parent process");
        });

    // Verify the parent process is running
    assert!(
        is_process_alive(parent_pid),
        "Parent process should be alive after start"
    );

    // Wait a moment for child processes to be spawned
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Find child processes (this is platform-specific and may need adjustment)
    let child_pids = find_child_processes(parent_pid);

    if !child_pids.is_empty() {
        println!(
            "Found {} child processes: {:?}",
            child_pids.len(),
            child_pids
        );

        // Verify at least some child processes are running
        let alive_children: Vec<_> = child_pids
            .iter()
            .filter(|&&pid| is_process_alive(pid))
            .collect();

        assert!(
            !alive_children.is_empty(),
            "Expected to find running child processes, but none were alive"
        );
    }

    // Stop the parent process - this should clean up children too
    let stop_output = test_env
        .runcept_cmd()
        .args(["stop", "parent-with-children"])
        .output()
        .expect("Failed to stop process");

    assert!(
        stop_output.status.success(),
        "Failed to stop parent process"
    );

    // Wait for cleanup to complete
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // CRITICAL TEST: Verify parent process is killed
    assert!(
        !is_process_alive(parent_pid),
        "CRITICAL: Parent process PID {} is still alive after stop command",
        parent_pid
    );

    // CRITICAL TEST: Verify child processes are also cleaned up
    for &child_pid in &child_pids {
        assert!(
            !is_process_alive(child_pid),
            "CRITICAL: Child process PID {} is still alive after parent was stopped. Process group cleanup failed.",
            child_pid
        );
    }

    // Verify through runcept that the process is stopped
    let cmd = test_env.runcept_cmd();
    assert_process_status(cmd, "parent-with-children", "stopped");
}

#[tokio::test]
async fn test_nested_process_groups_cleanup() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "nested-process-groups".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Create config with deeply nested process structure
    let config_content = r#"
[environment]
name = "nested-process-env"

[processes.nested-parent]
name = "nested-parent"
command = "bash -c 'bash -c \"sleep 10 & sleep 15 & wait\" & bash -c \"sleep 20\" & wait'"
auto_restart = false
"#;

    test_env.create_config_file(config_content).await.unwrap();

    // Activate the environment
    test_env
        .runcept_cmd()
        .args(["activate", &test_env.project_dir().to_string_lossy()])
        .assert()
        .success();

    // Start the nested process structure
    let start_output = test_env
        .runcept_cmd()
        .args(["start", "nested-parent"])
        .output()
        .expect("Failed to start nested parent");

    if !start_output.status.success() {
        println!("Start command failed for nested-parent!");
        println!("stdout: {}", String::from_utf8_lossy(&start_output.stdout));
        println!("stderr: {}", String::from_utf8_lossy(&start_output.stderr));
        panic!("Failed to start nested parent process");
    }

    // Wait a moment for the process to fully start
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // Get the parent PID
    let list_output = test_env
        .runcept_cmd()
        .args(["list"])
        .output()
        .expect("Failed to list processes");

    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    
    let parent_pid = extract_pid_from_list_output(&list_stdout, "nested-parent")
        .unwrap_or_else(|| {
            println!("Could not find PID for nested parent process 'nested-parent'");
            println!("Full list output:\n{}", list_stdout);
            
            // Get process logs for debugging
            if let Ok(logs_output) = test_env
                .runcept_cmd()
                .args(["logs", "nested-parent"])
                .output()
            {
                println!("Process logs:");
                println!("stdout: {}", String::from_utf8_lossy(&logs_output.stdout));
                println!("stderr: {}", String::from_utf8_lossy(&logs_output.stderr));
            }
            
            panic!("Could not find PID for nested parent process");
        });

    // Wait for nested processes to be established
    tokio::time::sleep(Duration::from_millis(2000)).await;

    // Find all descendant processes
    let descendant_pids = find_all_descendant_processes(parent_pid);

    println!(
        "Found {} descendant processes: {:?}",
        descendant_pids.len(),
        descendant_pids
    );

    // Stop the parent process
    let mut cmd = test_env.runcept_cmd();
    cmd.args(["stop", "nested-parent"]);
    assert_success_with_output(cmd, "stopped");

    // Wait for cleanup
    tokio::time::sleep(Duration::from_millis(2000)).await;

    // Verify all processes in the tree are cleaned up
    assert!(
        !is_process_alive(parent_pid),
        "Parent process should be terminated"
    );

    for &desc_pid in &descendant_pids {
        assert!(
            !is_process_alive(desc_pid),
            "Descendant process PID {} should be terminated after parent stop",
            desc_pid
        );
    }
}

/// Helper function to check if a process is alive using Unix signals
fn is_process_alive(pid: i32) -> bool {
    let nix_pid = Pid::from_raw(pid);
    signal::kill(nix_pid, None).is_ok()
}

/// Helper function to extract PID from runcept list output
/// Expected format: "process-name running   PID    timestamp ..."
fn extract_pid_from_list_output(output: &str, process_name: &str) -> Option<i32> {
    for line in output.lines() {
        // Skip header lines and empty lines
        if line.contains("PROCESS") || line.contains("-------") || line.trim().is_empty() {
            continue;
        }
        
        // Skip log lines that start with timestamps
        if line.contains("[") && line.contains("INFO") {
            continue;
        }
        
        if line.contains(process_name) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            
            // Look for the process name, then status, then PID
            // Expected format: process-name status PID uptime environment
            for (i, part) in parts.iter().enumerate() {
                if part == &process_name && i + 2 < parts.len() {
                    // Process name should be at index i
                    // Status should be at i + 1 (e.g., "running")
                    // PID should be at i + 2
                    if let Ok(pid) = parts[i + 2].parse::<i32>() {
                        if pid > 0 {
                            return Some(pid);
                        }
                    }
                }
            }
            
            // Fallback: look for any valid PID in the line
            for part in parts.iter() {
                if let Ok(pid) = part.parse::<i32>() {
                    if pid > 1000 && pid < 100000 {
                        return Some(pid);
                    }
                }
            }
        }
    }
    
    None
}

/// Find immediate child processes of a parent PID
fn find_child_processes(parent_pid: i32) -> Vec<i32> {
    let mut children = Vec::new();

    // Use ps to find child processes
    if let Ok(output) = std::process::Command::new("ps")
        .args(["-o", "pid,ppid", "--no-headers"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.trim().split_whitespace().collect();
            if parts.len() >= 2 {
                if let (Ok(child_pid), Ok(ppid)) =
                    (parts[0].parse::<i32>(), parts[1].parse::<i32>())
                {
                    if ppid == parent_pid {
                        children.push(child_pid);
                    }
                }
            }
        }
    }

    children
}

/// Find all descendant processes (children, grandchildren, etc.) of a parent PID
fn find_all_descendant_processes(parent_pid: i32) -> Vec<i32> {
    let mut descendants = Vec::new();
    let mut to_check = vec![parent_pid];

    while let Some(current_pid) = to_check.pop() {
        let children = find_child_processes(current_pid);
        for &child_pid in &children {
            descendants.push(child_pid);
            to_check.push(child_pid); // Add children to check for their children
        }
    }

    descendants
}
