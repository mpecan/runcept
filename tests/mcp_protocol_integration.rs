#![allow(unused_imports)]

mod common;

use common::environment::{RunceptTestEnvironment, TestConfig};
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::sleep;

/// Comprehensive MCP protocol integration tests
/// Tests the MCP server with real daemon communication, protocol interactions, and auto-configuration
///
/// These tests validate the same functionality as the old mcp_integration_test.rs but using
/// the new centralized test environment.

#[cfg(test)]
mod mcp_protocol_tests {
    use super::*;
    use rmcp::model::{CallToolRequestParam, ProtocolVersion};
    use rmcp::service::{DynService, RunningService};
    use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
    use rmcp::{RoleClient, ServiceExt};
    use std::borrow::Cow;

    struct McpTestEnvironment {
        test_env: RunceptTestEnvironment,
    }

    impl McpTestEnvironment {
        async fn new(project_name: &str) -> Self {
            let test_env = RunceptTestEnvironment::with_config(TestConfig {
                project_name: project_name.to_string(),
                enable_logging: true,
                ..TestConfig::default()
            })
            .await;

            Self { test_env }
        }

        async fn start_daemon(&mut self) -> Result<(), Box<dyn std::error::Error>> {
            self.test_env.start_daemon().await
        }

        async fn wait_for_daemon(&self) -> Result<(), Box<dyn std::error::Error>> {
            let socket_path = self.test_env.get_socket_path();

            // Wait up to 10 seconds for daemon to start
            for _ in 0..100 {
                if socket_path.exists() {
                    return Ok(());
                }
                sleep(Duration::from_millis(100)).await;
            }
            Err("Daemon failed to start within timeout".into())
        }

        async fn create_mcp_client(
            &self,
            working_directory: Option<PathBuf>,
        ) -> Result<RunningService<RoleClient, ()>, Box<dyn std::error::Error>> {
            let binary_path = self.test_env.binary_path();
            let client = ()
                .serve(TokioChildProcess::new(
                    Command::new(binary_path).configure(|cmd| {
                        cmd.arg("mcp").env("HOME", self.test_env.home_dir());
                        if let Some(dir) = working_directory {
                            cmd.current_dir(dir);
                        } else {
                            cmd.current_dir(self.test_env.project_dir());
                        }
                    }),
                )?)
                .await?;
            Ok(client)
        }

        async fn stop_daemon(&mut self) {
            let _ = self.test_env.stop_daemon().await;
        }

        fn project_dir(&self) -> &std::path::Path {
            self.test_env.project_dir()
        }

        async fn create_config_file(&self, content: &str) -> Result<PathBuf, std::io::Error> {
            self.test_env.create_config_file(content).await
        }

        fn init_project(&self) -> Result<(), Box<dyn std::error::Error>> {
            let mut cmd = std::process::Command::new(self.test_env.binary_path());
            cmd.args(["init", &self.project_dir().to_string_lossy()])
                .env("HOME", self.test_env.home_dir());
            let output = cmd.output()?;
            if !output.status.success() {
                return Err(format!(
                    "Init command failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                )
                .into());
            }
            Ok(())
        }
    }

    /// Test MCP server startup and basic functionality
    #[tokio::test]
    async fn test_mcp_server_startup() {
        let mut test_env = McpTestEnvironment::new("mcp-server-startup").await;

        // Start daemon first
        test_env
            .start_daemon()
            .await
            .expect("Failed to start daemon");

        // Create MCP client
        let client = test_env
            .create_mcp_client(None)
            .await
            .expect("Failed to create MCP client");

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
    }

    /// Test MCP tools listing functionality
    #[tokio::test]
    async fn test_mcp_list_tools() {
        let mut test_env = McpTestEnvironment::new("mcp-list-tools").await;

        // Start daemon first
        test_env
            .start_daemon()
            .await
            .expect("Failed to start daemon");

        // Create MCP client
        let client = test_env
            .create_mcp_client(None)
            .await
            .expect("Failed to create MCP client");

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
    }

    /// Test MCP auto-configuration feature
    #[tokio::test]
    async fn test_mcp_auto_configuration() {
        let mut test_env = McpTestEnvironment::new("mcp-auto-config").await;

        // Start daemon first
        test_env
            .start_daemon()
            .await
            .expect("Failed to start daemon");

        // Create MCP client
        let client = test_env
            .create_mcp_client(None)
            .await
            .expect("Failed to create MCP client");

        // Test activate_environment with auto-configuration
        // This should create a .runcept.toml file automatically
        let args = rmcp::object!({
            "path": test_env.project_dir().to_string_lossy().to_string()
        });

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
            response.content.iter().any(|c| {
                c.clone()
                    .raw
                    .as_text()
                    .map(|text| text.clone().text.contains("Environment activated"))
                    .unwrap_or(false)
            }),
            "Response should contain success message"
        );

        // Verify that .runcept.toml was created
        let config_path = test_env.project_dir().join(".runcept.toml");
        assert!(
            config_path.exists(),
            "MCP should have auto-created .runcept.toml"
        );

        // Verify config content
        let config_content = std::fs::read_to_string(&config_path).unwrap();
        assert!(config_content.contains("name ="));
        assert!(config_content.contains("worker"));

        // Clean up
        let _ = client.cancel().await;
        test_env.stop_daemon().await;
    }

    /// Test MCP tools functionality workflow
    #[tokio::test]
    async fn test_mcp_tools_workflow() {
        let mut test_env = McpTestEnvironment::new("mcp-tools-workflow").await;

        // Start daemon first
        test_env
            .start_daemon()
            .await
            .expect("Failed to start daemon");

        // Initialize project
        test_env
            .init_project()
            .expect("Failed to initialize project");

        // Create MCP client
        let client = test_env
            .create_mcp_client(None)
            .await
            .expect("Failed to create MCP client");

        // Test sequence: activate -> get status -> start process -> list processes
        let test_cases = vec![
            (
                "activate_environment",
                rmcp::object!({
                    "path": test_env.project_dir().to_string_lossy().to_string()
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
    }

    /// Test MCP process management tools (add, remove, update)
    #[tokio::test]
    async fn test_mcp_process_management_tools() {
        let mut test_env = McpTestEnvironment::new("mcp-process-mgmt").await;

        // Start daemon first
        test_env
            .start_daemon()
            .await
            .expect("Failed to start daemon");

        // Initialize project
        test_env
            .init_project()
            .expect("Failed to initialize project");

        // Create MCP client
        let client = test_env
            .create_mcp_client(None)
            .await
            .expect("Failed to create MCP client");

        // First activate environment
        let activate_result = client
            .peer()
            .call_tool(CallToolRequestParam {
                name: Cow::Owned("activate_environment".to_string()),
                arguments: Some(rmcp::object!({
                    "path": test_env.project_dir().to_string_lossy().to_string()
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
    }

    /// Test MCP process management tools with environment override
    #[tokio::test]
    async fn test_mcp_process_management_with_environment_override() {
        let mut test_env = McpTestEnvironment::new("mcp-env-override").await;

        // Start daemon first
        test_env
            .start_daemon()
            .await
            .expect("Failed to start daemon");

        // Initialize project
        test_env
            .init_project()
            .expect("Failed to initialize project");

        // Create MCP client
        let client = test_env
            .create_mcp_client(None)
            .await
            .expect("Failed to create MCP client");

        // First activate environment
        let activate_result = client
            .peer()
            .call_tool(CallToolRequestParam {
                name: Cow::Owned("activate_environment".to_string()),
                arguments: Some(rmcp::object!({
                    "path": test_env.project_dir().to_string_lossy().to_string()
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
                    "environment": "mcp-env-override",
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
                    "environment": "mcp-env-override"
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
    }

    /// Test MCP working directory resolution for processes
    #[tokio::test]
    async fn test_mcp_working_directory_resolution() {
        let mut test_env = McpTestEnvironment::new("mcp-working-dir").await;

        // Start daemon first
        test_env
            .start_daemon()
            .await
            .expect("Failed to start daemon");

        // Create a test file in the project directory to verify working directory
        let test_file_path = test_env.project_dir().join("test_marker.txt");
        std::fs::write(&test_file_path, "working_directory_test").unwrap();

        // Initialize project
        test_env
            .init_project()
            .expect("Failed to initialize project");

        // Create MCP client
        let client = test_env
            .create_mcp_client(Some(test_env.project_dir().to_path_buf()))
            .await
            .expect("Failed to create MCP client");

        // First activate environment
        let activate_result = client
            .peer()
            .call_tool(CallToolRequestParam {
                name: Cow::Owned("activate_environment".to_string()),
                arguments: Some(rmcp::object!({
                    "path": test_env.project_dir().to_string_lossy().to_string()
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
        assert!(
            !add_response.is_error.unwrap_or(false),
            "add_process should not return error"
        );

        let add_content = add_response
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
        println!("{add_content}");

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
        let sub_dir = test_env.project_dir().join("subdir");
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
    }
}
