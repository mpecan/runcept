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
    use rmcp::model::{
        CallToolRequestParam, ErrorData, JsonObject, PaginatedRequestParam, ProtocolVersion,
    };
    use rmcp::service::{DynService, RunningService};
    use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
    use rmcp::{RoleClient, ServiceExt};
    use std::borrow::Cow;
    use std::ops::Deref;

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
            cmd.args(&["daemon", "start"])
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
            cmd.args(&["daemon", "stop"])
                .env("HOME", &self.home_dir);
            let _ = cmd.output().await;
        }
    }

    impl Drop for McpTestEnvironment {
        fn drop(&mut self) {
            // Best effort cleanup - can't use async in Drop
            let runcept_path = get_binary_path("runcept");
            let _ = std::process::Command::new(runcept_path)
                .args(&["daemon", "stop"])
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
        let mut client = test_env.create_mcp_client().await;

        let result = client.service().get_info();
        assert_eq!(
            result.protocol_version,
            ProtocolVersion::V_2025_03_26,
            "MCP initialization should succeed: {:?}",
            result
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
        let mut client = test_env.create_mcp_client().await;

        // List available tools
        let tools_result = client.peer().list_tools(None).await;
        assert!(
            tools_result.is_ok(),
            "Should be able to list tools: {:?}",
            tools_result
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
        ];

        for expected_tool in expected_tools {
            assert!(
                tools.tools.iter().any(|t| t.name == expected_tool),
                "Expected tool '{}' should be available",
                expected_tool
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
        let mut client = test_env.create_mcp_client().await;

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
            "activate_environment should succeed: {:?}",
            call_result
        );

        let response = call_result.unwrap();
        println!("Activation response: {:?}", response);

        // Should contain success message
        assert!(response.content.iter().any(move |c| {
            c.clone().raw.as_text()
                .map(|text| text.clone().text.contains("Environment activated"))
                .unwrap_or(false)
        }), "Response should contain success message");

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
            .args(&[
                "init",
                test_env.project_dir.to_str().unwrap(),
            ])
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
            let result = client.peer().call_tool(CallToolRequestParam {
                name: Cow::Owned(tool_name.to_string()),
                arguments: Some(args),
            }).await;
            assert!(result.is_ok(), "{} should succeed: {:?}", action, result);

            let response = result.unwrap();
            println!("{} response: {:?}", action, response);

            // Basic success check - no error in response
            assert!(
                !response.is_error.unwrap_or(false),
                "{} should not return error",
                action
            );

            // Small delay between requests
            sleep(Duration::from_millis(100)).await;
        }

        // Clean up
        let _ = client.cancel().await;
        test_env.stop_daemon().await;
        let _ = daemon_process.wait().await;
    }
}
