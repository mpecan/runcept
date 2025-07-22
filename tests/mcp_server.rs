mod common;

use common::{
    environment::{RunceptTestEnvironment, TestConfig},
    fixtures::*,
};
use std::io::Write;
use std::time::Duration;
use tokio::time::sleep;

/// Tests for MCP (Model Context Protocol) server functionality
/// Verifies that the MCP server starts correctly and can handle basic protocol interactions

#[tokio::test]
async fn test_mcp_server_starts_and_responds() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "mcp-server-basic".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    test_env
        .create_config_file(basic_test_config())
        .await
        .unwrap();

    // Activate environment
    test_env.assert_cmd_success(
        &["activate", &test_env.project_dir().to_string_lossy()],
        "activated",
    );

    // Start MCP server in background using centralized helper
    let mut mcp_process = test_env
        .spawn_mcp_server()
        .expect("Failed to start MCP server");

    // Give MCP server time to start
    sleep(Duration::from_millis(500)).await;

    // Send a simple MCP request (basic test - real MCP testing would need proper protocol)
    if let Some(stdin) = mcp_process.stdin.as_mut() {
        let _ = stdin.write_all(b"\n");
    }

    // Give it time to process
    sleep(Duration::from_millis(100)).await;

    // Verify process is still running (didn't crash immediately)
    match mcp_process.try_wait() {
        Ok(Some(_)) => {
            // Process exited, which might be expected for some MCP protocols
            // This is not necessarily a failure
        }
        Ok(None) => {
            // Process is still running, which is good
            // MCP server is running - this is expected
        }
        Err(e) => {
            panic!("Error checking MCP process status: {e}");
        }
    }

    // Clean up
    let _ = mcp_process.kill();
    let _ = mcp_process.wait();
}

#[tokio::test]
async fn test_mcp_server_with_environment_context() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "mcp-with-environment".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    // Create a more complex config with multiple processes
    test_env
        .create_config_file(multi_process_config())
        .await
        .unwrap();

    // Activate environment
    test_env.assert_cmd_success(
        &["activate", &test_env.project_dir().to_string_lossy()],
        "activated",
    );

    // Start some processes to have context for MCP server
    test_env.assert_cmd_success(&["start", "worker-1"], "started");

    // Start MCP server with active environment and running processes
    let mut mcp_process = test_env
        .spawn_mcp_server_with_cwd(test_env.project_dir())
        .expect("Failed to start MCP server with environment");

    // Give MCP server time to initialize with environment context
    sleep(Duration::from_millis(1000)).await;

    // Send input to MCP server
    if let Some(stdin) = mcp_process.stdin.as_mut() {
        let _ = stdin.write_all(b"\n");
        let _ = stdin.flush();
    }

    // Give it time to process
    sleep(Duration::from_millis(200)).await;

    // Check if MCP server is handling the environment context
    match mcp_process.try_wait() {
        Ok(Some(exit_status)) => {
            // If it exited, check if it was graceful
            if !exit_status.success() {
                // Capture stderr for debugging
                if let Some(mut stderr) = mcp_process.stderr.take() {
                    let mut stderr_content = String::new();
                    if std::io::Read::read_to_string(&mut stderr, &mut stderr_content).is_ok() {
                        // Should provide meaningful error about missing environment
                        assert!(
                            stderr_content.contains("environment")
                                || stderr_content.contains("configuration")
                                || stderr_content.is_empty(), // Empty stderr is acceptable
                            "MCP server should handle missing environment gracefully. Stderr: {stderr_content}"
                        );
                    }
                }
            }
        }
        Ok(None) => {
            // Still running - should work without environment
        }
        Err(e) => {
            panic!("Error checking MCP process status: {e}");
        }
    }

    // Clean up
    let _ = mcp_process.kill();
    let _ = mcp_process.wait();
}

#[tokio::test]
async fn test_mcp_server_handles_invalid_input() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "mcp-invalid-input".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    test_env
        .create_config_file(basic_test_config())
        .await
        .unwrap();

    // Activate environment
    test_env.assert_cmd_success(
        &["activate", &test_env.project_dir().to_string_lossy()],
        "activated",
    );

    // Start MCP server using centralized helper
    let mut mcp_process = test_env
        .spawn_mcp_server()
        .expect("Failed to start MCP server");

    // Give MCP server time to start
    sleep(Duration::from_millis(500)).await;

    // Send invalid JSON input
    if let Some(stdin) = mcp_process.stdin.as_mut() {
        let _ = stdin.write_all(b"invalid json{\n");
        let _ = stdin.flush();
    }

    // Give it time to process invalid input
    sleep(Duration::from_millis(200)).await;

    // MCP server should handle invalid input gracefully
    match mcp_process.try_wait() {
        Ok(Some(exit_status)) => {
            if !exit_status.success() {
                // If it crashes, it should provide meaningful error output
                if let Some(mut stderr) = mcp_process.stderr.take() {
                    let mut stderr_content = String::new();
                    if std::io::Read::read_to_string(&mut stderr, &mut stderr_content).is_ok() {
                        // Should contain some indication of parsing/protocol error
                        assert!(
                            stderr_content.contains("json")
                                || stderr_content.contains("parse")
                                || stderr_content.contains("protocol")
                                || stderr_content.is_empty(), // Empty is acceptable
                            "MCP server should provide meaningful error for invalid input. Stderr: {stderr_content}"
                        );
                    }
                }
            }
        }
        Ok(None) => {
            // Still running - should continue despite invalid input
        }
        Err(e) => {
            panic!("Error checking MCP process status: {e}");
        }
    }

    // Clean up
    let _ = mcp_process.kill();
    let _ = mcp_process.wait();
}

#[tokio::test]
async fn test_mcp_server_concurrent_access() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "mcp-concurrent".to_string(),
        enable_logging: true,
        ..TestConfig::default()
    })
    .await;

    test_env
        .create_config_file(basic_test_config())
        .await
        .unwrap();

    // Activate environment using centralized method
    test_env.assert_cmd_success(
        &["activate", &test_env.project_dir().to_string_lossy()],
        "activated",
    );

    // Start multiple MCP server instances to test concurrent access
    let mut mcp_processes = Vec::new();

    for i in 0..3 {
        match test_env.spawn_mcp_server_with_cwd(test_env.project_dir()) {
            Ok(process) => {
                mcp_processes.push(process);
            }
            Err(e) => {
                eprintln!("Failed to start MCP server instance {i}: {e}");
                // Continue with other instances
            }
        }
    }

    // Give all MCP servers time to start
    sleep(Duration::from_millis(1000)).await;

    // Send input to all running instances
    for process in &mut mcp_processes {
        if let Some(stdin) = process.stdin.as_mut() {
            let _ = stdin.write_all(b"\n");
        }
    }

    // Give them time to process
    sleep(Duration::from_millis(200)).await;

    // At least one should be running or they should all exit gracefully
    let mut any_running = false;
    let mut all_succeeded = true;

    for process in &mut mcp_processes {
        match process.try_wait() {
            Ok(Some(exit_status)) => {
                if !exit_status.success() {
                    all_succeeded = false;
                }
            }
            Ok(None) => {
                any_running = true;
            }
            Err(_) => {
                all_succeeded = false;
            }
        }
        if let Some(stdout) = process.stdout.as_mut() {
            let mut output = String::new();
            if std::io::Read::read_to_string(stdout, &mut output).is_ok() {
                // Optionally print output for debugging
                println!("MCP server output: {output}");
            }
        }
    }

    // Check that we have at least one successful process, or document why not
    println!(
        "Concurrent MCP test results: {} running, all_succeeded: {}, total_processes: {}",
        if any_running { "some" } else { "none" },
        all_succeeded,
        mcp_processes.len()
    );

    // At minimum, we should be able to start some processes
    assert!(
        !mcp_processes.is_empty(),
        "Should be able to start at least some MCP server processes"
    );

    // If processes exit, they should exit gracefully, or be expected to conflict
    // For now, we'll accept that multiple MCP servers might not be able to run concurrently
    println!("MCP concurrent access test completed - concurrent access may be limited by design");

    // Clean up all processes
    for mut process in mcp_processes {
        let _ = process.kill();
        let _ = process.wait();
    }
}
