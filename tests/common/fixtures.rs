#![allow(dead_code)]
#![allow(unused_imports)]

/// Common test configuration fixtures and templates
/// Basic test environment configuration
pub fn basic_test_config() -> &'static str {
    r#"
[environment]
name = "test-env"

[processes.test-process]
name = "test-process"
command = "echo 'Hello World'"
auto_restart = false
"#
}

/// Configuration with health check (will fail)
pub fn failing_health_check_config() -> &'static str {
    r#"
[environment]
name = "health-check-env"

[processes.health-check-process]
name = "health-check-process"
command = "sleep 30"
health_check_url = "http://localhost:9999/health"
health_check_interval = 1
auto_restart = false
"#
}

/// Configuration with successful health check
pub fn successful_health_check_config(port: u16) -> String {
    format!(
        r#"
[environment]
name = "health-check-env"

[processes.web-server]
name = "web-server"
command = "python3 -m http.server {port}"
health_check_url = "http://localhost:{port}/"
health_check_interval = 2
auto_restart = false
"#
    )
}

/// Multi-process configuration
pub fn multi_process_config() -> &'static str {
    r#"
[environment]
name = "multi-env"

[processes.worker-1]
name = "worker-1"
command = "sleep 10"
auto_restart = false

[processes.worker-2]
name = "worker-2"
command = "sleep 5"
auto_restart = false

[processes.quick-task]
name = "quick-task"
command = "echo 'Done'"
auto_restart = false
"#
}

/// Configuration with auto-restart enabled
pub fn auto_restart_config() -> &'static str {
    r#"
[environment]
name = "restart-env"

[processes.auto-restart-process]
name = "auto-restart-process"
command = "bash -c 'echo Starting; sleep 2; exit 1'"
auto_restart = true
restart_delay = 1000
max_restarts = 3
"#
}

/// Configuration with working directory
pub fn working_dir_config() -> &'static str {
    r#"
[environment]
name = "working-dir-env"

[processes.pwd-process]
name = "pwd-process"
command = "pwd"
working_dir = "src"
auto_restart = false
"#
}

/// Configuration with environment variables
pub fn env_vars_config() -> &'static str {
    r#"
[environment]
name = "env-vars-env"

[processes.env-process]
name = "env-process"
command = "env | grep TEST_VAR"
auto_restart = false

[processes.env-process.env]
TEST_VAR = "test-value"
ANOTHER_VAR = "another-value"
"#
}

/// Configuration for testing process groups (parent spawns children)
pub fn process_group_config() -> &'static str {
    r#"
[environment]
name = "process-group-env"

[processes.parent-process]
name = "parent-process"
command = "bash -c 'echo Parent PID: $$; sleep 2 & sleep 4 & wait'"
auto_restart = false
"#
}

/// Configuration for MCP testing
pub fn mcp_test_config() -> &'static str {
    r#"
[environment]
name = "mcp-env"
"#
}

/// Invalid configuration (for error testing)
pub fn invalid_config() -> &'static str {
    r#"
[environment]
name = "invalid-env"

[processes.invalid-process]
# Missing required fields
auto_restart = false
"#
}

/// Empty configuration
pub fn empty_config() -> &'static str {
    r#"
[environment]
name = "empty-env"
"#
}

/// Configuration with TCP health check
pub fn tcp_health_check_config(port: u16) -> String {
    format!(
        r#"
[environment]
name = "tcp-env"

[processes.tcp-server]
name = "tcp-server"
command = "nc -l {port}"
health_check_url = "tcp://localhost:{port}"
health_check_interval = 1
auto_restart = false
"#
    )
}

/// Configuration with command health check
pub fn command_health_check_config() -> &'static str {
    r#"
[environment]
name = "command-env"

[processes.command-process]
name = "command-process"
command = "sleep 10"
health_check_url = "cmd://echo 'healthy'"
health_check_interval = 1
auto_restart = false
"#
}

/// Get a random available port for testing
pub fn get_random_port() -> u16 {
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind to random port");
    let port = listener
        .local_addr()
        .expect("Failed to get local addr")
        .port();
    drop(listener); // Release the port
    port
}

/// Test data for various scenarios
pub struct TestFixtures;

impl TestFixtures {
    /// Get a basic process list for testing
    pub fn basic_process_list() -> Vec<(&'static str, &'static str)> {
        vec![
            ("web-server", "echo 'Web server running'"),
            ("worker", "sleep 5"),
            ("quick-task", "echo 'Task completed'"),
        ]
    }

    /// Get test environment variables
    pub fn test_env_vars() -> Vec<(&'static str, &'static str)> {
        vec![
            ("TEST_MODE", "integration"),
            ("LOG_LEVEL", "debug"),
            ("SERVICE_NAME", "test-service"),
        ]
    }

    /// Get test file content for various scenarios
    pub fn test_script_content() -> &'static str {
        r#"#!/bin/bash
echo "Script started at $(date)"
sleep 2
echo "Script completed"
"#
    }
}
