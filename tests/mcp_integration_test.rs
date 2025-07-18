use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tempfile::TempDir;
use tokio::process::{Child, Command};
use tokio::time::sleep;

mod common;
use common::get_binary_path;

/// Integration tests for MCP server functionality
/// Tests the MCP server with real daemon communication and auto-configuration
#[cfg(test)]
mod mcp_integration_tests {
    use super::*;
    use rmcp::model::{CallToolRequestParam, ProtocolVersion};
    use rmcp::service::{DynService, RunningService};
    use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
    use rmcp::{RoleClient, ServiceExt};
    use std::borrow::Cow;

    struct McpTestEnvironment {
        temp_dir: TempDir,
        home_dir: PathBuf,
        project_dir: PathBuf,
        runcept_dir: PathBuf,
    }

    impl McpTestEnvironment {
        fn new() -> Self {
            let temp_dir = TempDir::new().unwrap();
            let home_dir = temp_dir.path().join("home");
            let project_dir = temp_dir.path().join("test_project");
            let runcept_dir = home_dir.join(".runcept");

            // Create directory structure
            std::fs::create_dir_all(&home_dir).unwrap();
            std::fs::create_dir_all(&project_dir).unwrap();
            std::fs::create_dir_all(&runcept_dir).unwrap();

            Self {
                temp_dir,
                home_dir,
                project_dir,
                runcept_dir,
            }
        }

        async fn start_daemon(&self) -> Child {
            let runcept_path = get_binary_path("runcept");
            let mut cmd = Command::new(runcept_path);
            cmd.args(["daemon", "start"])
                .env("HOME", &self.home_dir)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true);

            cmd.spawn().expect("Failed to start daemon")
        }

        async fn create_mcp_client(&self) -> RunningService<RoleClient, ()> {
            let runcept_path = get_binary_path("runcept");
            ().serve(
                TokioChildProcess::new(Command::new(runcept_path).configure(|cmd| {
                    cmd.arg("mcp").env("HOME", &self.home_dir);
                }))
                .expect("Should create MCP client process"),
            )
            .await
            .expect("Should create MCP client process")
        }

        async fn wait_for_daemon(&self) {
            let socket_path = self.runcept_dir.join("daemon.sock");

            // Wait up to 5 seconds for daemon to start
            for _ in 0..50 {
                if socket_path.exists() {
                    return;
                }
                sleep(Duration::from_millis(1000)).await;
            }
            panic!("Daemon failed to start within timeout");
        }

        async fn stop_daemon(&self) {
            let runcept_path = get_binary_path("runcept");
            let mut cmd = Command::new(runcept_path);
            cmd.args(["daemon", "stop"]).env("HOME", &self.home_dir);
            let _ = cmd.output().await;
        }
    }

    /// Test MCP working directory resolution for processes
    #[tokio::test]
    async fn test_mcp_working_directory_resolution() {
        let test_env = McpTestEnvironment::new();

        // Start daemon first
        let mut daemon_process = test_env.start_daemon().await;
        test_env.wait_for_daemon().await;

        // Create a test file in the project directory to verify working directory
        let test_file_path = test_env.project_dir.join("test_marker.txt");
        std::fs::write(&test_file_path, "working_directory_test").unwrap();

        // Create a project with configuration first
        let runcept_path = get_binary_path("runcept");
        let mut init_cmd = Command::new(runcept_path);
        init_cmd
            .args(["init", test_env.project_dir.to_str().unwrap()])
            .env("HOME", &test_env.home_dir);
        let init_output = init_cmd.output().await.expect("Failed to run init command");
        assert!(init_output.status.success(), "Init command should succeed");

        // Create MCP client
        let client = test_env.create_mcp_client().await;

        // First activate environment
        let activate_result = client
            .peer()
            .call_tool(CallToolRequestParam {
                name: Cow::Owned("activate_environment".to_string()),
                arguments: Some(rmcp::object!({
                    "path": test_env.project_dir.to_string_lossy().to_string()
                })),
            })
            .await;
        assert!(
            activate_result.is_ok(),
            "Environment activation should succeed"
        );

        // Test 1: Add process with working_dir = "." (should resolve to project directory)
        let add_result = client
            .peer()
            .call_tool(CallToolRequestParam {
                name: Cow::Owned("add_process".to_string()),
                arguments: Some(rmcp::object!({
                    "name": "test-working-dir",
                    "command": "ls test_marker.txt && sleep 1",
                    "working_dir": ".",
                    "auto_restart": false
                })),
            })
            .await;
        assert!(
            add_result.is_ok(),
            "add_process with working_dir='.' should succeed: {add_result:?}"
        );
        let add_response = add_result.unwrap();

        // Print the add_process response for debugging
        let add_response_content = add_response
            .content
            .iter()
            .filter_map(|c| {
                if let Some(text) = c.clone().raw.as_text() {
                    Some(text.text.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        println!("Add process response: {}", add_response_content);

        assert!(
            !add_response.is_error.unwrap_or(false),
            "add_process should not return error. Response: {add_response_content}"
        );

        // Test 2: Start the process to verify it runs in correct directory
        let start_result = client
            .peer()
            .call_tool(CallToolRequestParam {
                name: Cow::Owned("start_process".to_string()),
                arguments: Some(rmcp::object!({
                    "name": "test-working-dir"
                })),
            })
            .await;
        assert!(
            start_result.is_ok(),
            "start_process should succeed: {start_result:?}"
        );

        // Give process time to complete
        sleep(Duration::from_millis(1500)).await;

        // First, let's verify the process was actually added to the environment by listing processes
        let list_result = client
            .peer()
            .call_tool(CallToolRequestParam {
                name: Cow::Owned("list_processes".to_string()),
                arguments: Some(rmcp::object!({})),
            })
            .await;
        assert!(
            list_result.is_ok(),
            "list_processes should succeed: {list_result:?}"
        );
        let list_response = list_result.unwrap();
        let process_list = list_response
            .content
            .iter()
            .filter_map(|c| {
                if let Some(text) = c.clone().raw.as_text() {
                    Some(text.text.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        println!("Process list: {}", process_list);
        assert!(
            process_list.contains("test-working-dir"),
            "Process list should contain test-working-dir. List: {process_list}"
        );

        // Test 3: Check logs to verify the process found the test file (indicating correct working directory)
        let logs_result = client
            .peer()
            .call_tool(CallToolRequestParam {
                name: Cow::Owned("get_process_logs".to_string()),
                arguments: Some(rmcp::object!({
                    "name": "test-working-dir"
                })),
            })
            .await;
        assert!(
            logs_result.is_ok(),
            "get_process_logs should succeed: {logs_result:?}"
        );
        let logs_response = logs_result.unwrap();

        // Extract log content and verify the process found the test file
        let log_content = logs_response
            .content
            .iter()
            .filter_map(|c| {
                if let Some(text) = c.clone().raw.as_text() {
                    Some(text.text.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            log_content.contains("test_marker.txt"),
            "Process logs should contain test_marker.txt, indicating correct working directory. Logs: {log_content}"
        );

        // Test 4: Test relative path resolution
        // Create a subdirectory
        let sub_dir = test_env.project_dir.join("subdir");
        std::fs::create_dir_all(&sub_dir).unwrap();
        std::fs::write(sub_dir.join("sub_marker.txt"), "subdirectory_test").unwrap();

        let add_subdir_result = client
            .peer()
            .call_tool(CallToolRequestParam {
                name: Cow::Owned("add_process".to_string()),
                arguments: Some(rmcp::object!({
                    "name": "test-subdir",
                    "command": "ls sub_marker.txt && sleep 1",
                    "working_dir": "subdir",
                    "auto_restart": false
                })),
            })
            .await;
        assert!(
            add_subdir_result.is_ok(),
            "add_process with relative working_dir should succeed: {add_subdir_result:?}"
        );

        let start_subdir_result = client
            .peer()
            .call_tool(CallToolRequestParam {
                name: Cow::Owned("start_process".to_string()),
                arguments: Some(rmcp::object!({
                    "name": "test-subdir"
                })),
            })
            .await;
        assert!(
            start_subdir_result.is_ok(),
            "start_process for subdir test should succeed: {start_subdir_result:?}"
        );

        // Give process time to complete
        sleep(Duration::from_millis(1500)).await;

        let logs_subdir_result = client
            .peer()
            .call_tool(CallToolRequestParam {
                name: Cow::Owned("get_process_logs".to_string()),
                arguments: Some(rmcp::object!({
                    "name": "test-subdir"
                })),
            })
            .await;
        assert!(
            logs_subdir_result.is_ok(),
            "get_process_logs for subdir should succeed: {logs_subdir_result:?}"
        );
        let logs_subdir_response = logs_subdir_result.unwrap();

        let log_subdir_content = logs_subdir_response
            .content
            .iter()
            .filter_map(|c| {
                if let Some(text) = c.clone().raw.as_text() {
                    Some(text.text.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            log_subdir_content.contains("sub_marker.txt"),
            "Subdir process logs should contain sub_marker.txt, indicating correct relative path resolution. Logs: {log_subdir_content}"
        );

        // Clean up
        let _ = client.cancel().await;
        test_env.stop_daemon().await;
        let _ = daemon_process.wait().await;
    }
    impl Drop for McpTestEnvironment {
        fn drop(&mut self) {
            // Best effort cleanup - can't use async in Drop
            let runcept_path = get_binary_path("runcept");
            let _ = std::process::Command::new(runcept_path)
                .args(["daemon", "stop"])
                .env("HOME", &self.home_dir)
                .output();
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    /// Test MCP server startup and basic functionality
    #[tokio::test]
    async fn test_mcp_server_startup() {
        let test_env = McpTestEnvironment::new();

        // Start daemon first
        let mut daemon_process = test_env.start_daemon().await;
        test_env.wait_for_daemon().await;

        // Create MCP client
        let client = test_env.create_mcp_client().await;

        let result = client.service().get_info();
        assert_eq!(
            result.protocol_version,
            ProtocolVersion::V_2025_03_26,
            "MCP initialization should succeed: {result:?}"
        );

        println!("MCP server initialized successfully");

        // Clean up
        let _ = client.cancel().await;
        test_env.stop_daemon().await;
        let _ = daemon_process.wait().await;
    }

    /// Test MCP tools listing functionality
    #[tokio::test]
    async fn test_mcp_list_tools() {
        let test_env = McpTestEnvironment::new();

        // Start daemon first
        let mut daemon_process = test_env.start_daemon().await;
        test_env.wait_for_daemon().await;

        // Create MCP client
        let client = test_env.create_mcp_client().await;

        // List available tools
        let tools_result = client.peer().list_tools(None).await;
        assert!(
            tools_result.is_ok(),
            "Should be able to list tools: {tools_result:?}"
        );

        let tools = tools_result.unwrap();
        println!(
            "Available tools: {:?}",
            tools.tools.iter().map(|t| &t.name).collect::<Vec<_>>()
        );

        // Verify expected tools are available
        let expected_tools = vec![
            "activate_environment",
            "get_environment_status",
            "list_processes",
            "start_process",
            "stop_process",
            "restart_process",
            "get_process_logs",
            "deactivate_environment",
            "list_all_processes",
            "get_daemon_status",
            "kill_all_processes",
            "record_environment_activity",
            "add_process",
            "remove_process",
            "update_process",
        ];

        for expected_tool in expected_tools {
            assert!(
                tools.tools.iter().any(|t| t.name == expected_tool),
                "Expected tool '{expected_tool}' should be available"
            );
        }

        // Clean up
        let _ = client.cancel().await;
        test_env.stop_daemon().await;
        let _ = daemon_process.wait().await;
    }
    //
    /// Test MCP auto-configuration feature
    #[tokio::test]
    async fn test_mcp_auto_configuration() {
        let test_env = McpTestEnvironment::new();

        // Start daemon first
        let mut daemon_process = test_env.start_daemon().await;
        test_env.wait_for_daemon().await;

        // Create MCP client
        let client = test_env.create_mcp_client().await;

        // Test activate_environment with auto-configuration
        // This should create a .runcept.toml file automatically
        let args = rmcp::object!(
          {  "path" : test_env.project_dir.to_string_lossy().to_string()}
        );

        let call_result = client
            .peer()
            .call_tool(CallToolRequestParam {
                name: Cow::Owned("activate_environment".to_string()),
                arguments: Some(args),
            })
            .await;
        assert!(
            call_result.is_ok(),
            "activate_environment should succeed: {call_result:?}"
        );

        let response = call_result.unwrap();
        println!("Activation response: {response:?}");

        // Should contain success message
        assert!(
            response.content.iter().any(move |c| {
                c.clone()
                    .raw
                    .as_text()
                    .map(|text| text.clone().text.contains("Environment activated"))
                    .unwrap_or(false)
            }),
            "Response should contain success message"
        );

        // Verify that .runcept.toml was created
        let config_path = test_env.project_dir.join(".runcept.toml");
        assert!(
            config_path.exists(),
            "MCP should have auto-created .runcept.toml"
        );

        // Verify config content
        let config_content = std::fs::read_to_string(&config_path).unwrap();
        assert!(config_content.contains("name = \"test_project\""));
        assert!(config_content.contains("worker"));

        // Clean up
        let _ = client.cancel().await;
        test_env.stop_daemon().await;
        let _ = daemon_process.wait().await;
    }

    /// Test MCP tools functionality workflow
    #[tokio::test]
    async fn test_mcp_tools_workflow() {
        let test_env = McpTestEnvironment::new();

        // Start daemon first
        let mut daemon_process = test_env.start_daemon().await;
        test_env.wait_for_daemon().await;

        // Create a project with configuration first
        let runcept_path = get_binary_path("runcept");
        let mut init_cmd = Command::new(runcept_path);
        init_cmd
            .args(["init", test_env.project_dir.to_str().unwrap()])
            .env("HOME", &test_env.home_dir);
        let init_output = init_cmd.output().await.expect("Failed to run init command");
        assert!(init_output.status.success(), "Init command should succeed");

        // Create MCP client
        let client = test_env.create_mcp_client().await;

        // Test sequence: activate -> get status -> start process -> list processes
        let test_cases = vec![
            (
                "activate_environment",
                rmcp::object!({
                    "path": test_env.project_dir.to_string_lossy().to_string()
                }),
                "activate",
            ),
            ("get_environment_status", rmcp::object!({}), "status"),
            (
                "start_process",
                rmcp::object!({
                    "name": "worker"
                }),
                "start",
            ),
            ("list_processes", rmcp::object!({}), "list"),
        ];

        for (tool_name, args, action) in test_cases {
            let result = client
                .peer()
                .call_tool(CallToolRequestParam {
                    name: Cow::Owned(tool_name.to_string()),
                    arguments: Some(args),
                })
                .await;
            assert!(result.is_ok(), "{action} should succeed: {result:?}");

            let response = result.unwrap();
            println!("{action} response: {response:?}");

            // Basic success check - no error in response
            assert!(
                !response.is_error.unwrap_or(false),
                "{action} should not return error"
            );

            // Small delay between requests
            sleep(Duration::from_millis(100)).await;
        }

        // Clean up
        let _ = client.cancel().await;
        test_env.stop_daemon().await;
        let _ = daemon_process.wait().await;
    }

    /// Test MCP process management tools (add, remove, update)
    #[tokio::test]
    async fn test_mcp_process_management_tools() {
        let test_env = McpTestEnvironment::new();

        // Start daemon first
        let mut daemon_process = test_env.start_daemon().await;
        test_env.wait_for_daemon().await;

        // Create a project with configuration first
        let runcept_path = get_binary_path("runcept");
        let mut init_cmd = Command::new(runcept_path);
        init_cmd
            .args(["init", test_env.project_dir.to_str().unwrap()])
            .env("HOME", &test_env.home_dir);
        let init_output = init_cmd.output().await.expect("Failed to run init command");
        assert!(init_output.status.success(), "Init command should succeed");

        // Create MCP client
        let client = test_env.create_mcp_client().await;

        // First activate environment
        let activate_result = client
            .peer()
            .call_tool(CallToolRequestParam {
                name: Cow::Owned("activate_environment".to_string()),
                arguments: Some(rmcp::object!({
                    "path": test_env.project_dir.to_string_lossy().to_string()
                })),
            })
            .await;
        assert!(
            activate_result.is_ok(),
            "Environment activation should succeed"
        );

        // Test add_process tool
        let add_result = client
            .peer()
            .call_tool(CallToolRequestParam {
                name: Cow::Owned("add_process".to_string()),
                arguments: Some(rmcp::object!({
                    "name": "new-process",
                    "command": "echo 'Hello from new process'",
                    "auto_restart": false
                })),
            })
            .await;
        assert!(
            add_result.is_ok(),
            "add_process should succeed: {add_result:?}"
        );
        let add_response = add_result.unwrap();
        assert!(
            !add_response.is_error.unwrap_or(false),
            "add_process should not return error"
        );

        // Test update_process tool
        let update_result = client
            .peer()
            .call_tool(CallToolRequestParam {
                name: Cow::Owned("update_process".to_string()),
                arguments: Some(rmcp::object!({
                    "name": "new-process",
                    "command": "echo 'Updated process command'",
                    "auto_restart": true
                })),
            })
            .await;
        assert!(
            update_result.is_ok(),
            "update_process should succeed: {update_result:?}"
        );
        let update_response = update_result.unwrap();
        assert!(
            !update_response.is_error.unwrap_or(false),
            "update_process should not return error"
        );

        // Test remove_process tool
        let remove_result = client
            .peer()
            .call_tool(CallToolRequestParam {
                name: Cow::Owned("remove_process".to_string()),
                arguments: Some(rmcp::object!({
                    "name": "new-process"
                })),
            })
            .await;
        assert!(
            remove_result.is_ok(),
            "remove_process should succeed: {remove_result:?}"
        );
        let remove_response = remove_result.unwrap();
        assert!(
            !remove_response.is_error.unwrap_or(false),
            "remove_process should not return error"
        );

        // Clean up
        let _ = client.cancel().await;
        test_env.stop_daemon().await;
        let _ = daemon_process.wait().await;
    }

    /// Test automatic config reloading when .runcept.toml is modified
    #[tokio::test]
    async fn test_automatic_config_reload() {
        let test_env = McpTestEnvironment::new();

        // Start daemon first
        let mut daemon_process = test_env.start_daemon().await;
        test_env.wait_for_daemon().await;

        // Create a project with configuration first
        let runcept_path = get_binary_path("runcept");
        let mut init_cmd = Command::new(runcept_path);
        init_cmd
            .args(["init", test_env.project_dir.to_str().unwrap()])
            .env("HOME", &test_env.home_dir);
        let init_output = init_cmd.output().await.expect("Failed to run init command");
        assert!(init_output.status.success(), "Init command should succeed");

        // Create MCP client
        let client = test_env.create_mcp_client().await;

        // Activate environment
        let activate_result = client
            .peer()
            .call_tool(CallToolRequestParam {
                name: Cow::Owned("activate_environment".to_string()),
                arguments: Some(rmcp::object!({
                    "path": test_env.project_dir.to_string_lossy().to_string()
                })),
            })
            .await;
        assert!(
            activate_result.is_ok(),
            "Environment activation should succeed"
        );

        // Verify initial config has only the worker process
        let initial_list_result = client
            .peer()
            .call_tool(CallToolRequestParam {
                name: Cow::Owned("list_processes".to_string()),
                arguments: Some(rmcp::object!({})),
            })
            .await;
        assert!(initial_list_result.is_ok());
        let initial_list_response = initial_list_result.unwrap();
        let initial_process_list = initial_list_response
            .content
            .iter()
            .filter_map(|c| {
                if let Some(text) = c.clone().raw.as_text() {
                    Some(text.text.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        
        assert!(initial_process_list.contains("worker"));
        assert!(!initial_process_list.contains("auto-added-process"));

        // Modify the .runcept.toml file directly to add a new process
        let config_path = test_env.project_dir.join(".runcept.toml");
        let mut config_content = std::fs::read_to_string(&config_path).unwrap();
        
        // Add a new process to the config
        config_content.push_str("\n[processes.auto-added-process]\n");
        config_content.push_str("command = \"echo 'Auto-added process'\"\n");
        config_content.push_str("auto_restart = false\n");
        
        std::fs::write(&config_path, config_content).unwrap();

        // Give the file watcher time to detect the change and reload
        sleep(Duration::from_millis(1000)).await;

        // Verify the new process appears in the list
        let updated_list_result = client
            .peer()
            .call_tool(CallToolRequestParam {
                name: Cow::Owned("list_processes".to_string()),
                arguments: Some(rmcp::object!({})),
            })
            .await;
        assert!(updated_list_result.is_ok());
        let updated_list_response = updated_list_result.unwrap();
        let updated_process_list = updated_list_response
            .content
            .iter()
            .filter_map(|c| {
                if let Some(text) = c.clone().raw.as_text() {
                    Some(text.text.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        
        println!("Updated process list: {}", updated_process_list);
        assert!(
            updated_process_list.contains("auto-added-process"),
            "Process list should contain auto-added-process after config reload. List: {updated_process_list}"
        );

        // Clean up
        let _ = client.cancel().await;
        test_env.stop_daemon().await;
        let _ = daemon_process.wait().await;
    }

    /// Test MCP process management tools with environment override
    #[tokio::test]
    async fn test_mcp_process_management_with_environment_override() {
        let test_env = McpTestEnvironment::new();

        // Start daemon first
        let mut daemon_process = test_env.start_daemon().await;
        test_env.wait_for_daemon().await;

        // Create a project with configuration first
        let runcept_path = get_binary_path("runcept");
        let mut init_cmd = Command::new(runcept_path);
        init_cmd
            .args(["init", test_env.project_dir.to_str().unwrap()])
            .env("HOME", &test_env.home_dir);
        let init_output = init_cmd.output().await.expect("Failed to run init command");
        assert!(init_output.status.success(), "Init command should succeed");

        // Create MCP client
        let client = test_env.create_mcp_client().await;

        // First activate environment
        let activate_result = client
            .peer()
            .call_tool(CallToolRequestParam {
                name: Cow::Owned("activate_environment".to_string()),
                arguments: Some(rmcp::object!({
                    "path": test_env.project_dir.to_string_lossy().to_string()
                })),
            })
            .await;
        assert!(
            activate_result.is_ok(),
            "Environment activation should succeed"
        );

        // Test add_process tool with environment override
        let add_result = client
            .peer()
            .call_tool(CallToolRequestParam {
                name: Cow::Owned("add_process".to_string()),
                arguments: Some(rmcp::object!({
                    "name": "env-specific-process",
                    "command": "echo 'Environment specific process'",
                    "environment": "test_project",
                    "auto_restart": false
                })),
            })
            .await;
        assert!(
            add_result.is_ok(),
            "add_process with environment should succeed: {add_result:?}"
        );
        let add_response = add_result.unwrap();
        assert!(
            !add_response.is_error.unwrap_or(false),
            "add_process should not return error"
        );

        // Test remove_process tool with environment override
        let remove_result = client
            .peer()
            .call_tool(CallToolRequestParam {
                name: Cow::Owned("remove_process".to_string()),
                arguments: Some(rmcp::object!({
                    "name": "env-specific-process",
                    "environment": "test_project"
                })),
            })
            .await;
        assert!(
            remove_result.is_ok(),
            "remove_process with environment should succeed: {remove_result:?}"
        );
        let remove_response = remove_result.unwrap();
        assert!(
            !remove_response.is_error.unwrap_or(false),
            "remove_process should not return error"
        );

        // Clean up
        let _ = client.cancel().await;
        test_env.stop_daemon().await;
        let _ = daemon_process.wait().await;
    }
}
