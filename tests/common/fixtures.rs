#![allow(dead_code)]
#![allow(unused_imports)]

/// Common test configuration fixtures and templates
/// Basic test environment configuration
pub fn basic_test_config() -> String {
    let echo_cmd = if cfg!(windows) {
        "echo Hello World"
    } else {
        "echo 'Hello World'"
    };

    format!(
        r#"
[environment]
name = "test-env"

[processes.test-process]
name = "test-process"
command = "{echo_cmd}"
auto_restart = false
"#
    )
}

/// Configuration with health check (will fail)
pub fn failing_health_check_config() -> String {
    let sleep_cmd = if cfg!(windows) {
        "timeout /t 30 /nobreak"
    } else {
        "sleep 30"
    };

    format!(
        r#"
[environment]
name = "health-check-env"

[processes.health-check-process]
name = "health-check-process"
command = "{sleep_cmd}"
health_check_url = "http://localhost:9999/health"
health_check_interval = 1
auto_restart = false
"#
    )
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
pub fn multi_process_config() -> String {
    let (sleep_10, sleep_5, echo_done) = if cfg!(windows) {
        (
            "timeout /t 10 /nobreak",
            "timeout /t 5 /nobreak",
            "echo Done",
        )
    } else {
        ("sleep 10", "sleep 5", "echo 'Done'")
    };

    format!(
        r#"
[environment]
name = "multi-env"

[processes.worker-1]
name = "worker-1"
command = "{sleep_10}"
auto_restart = false

[processes.worker-2]
name = "worker-2"
command = "{sleep_5}"
auto_restart = false

[processes.quick-task]
name = "quick-task"
command = "{echo_done}"
auto_restart = false
"#
    )
}

/// Configuration with auto-restart enabled
pub fn auto_restart_config() -> String {
    let restart_cmd = if cfg!(windows) {
        "cmd /c \"echo Starting && timeout /t 2 /nobreak && exit 1\""
    } else {
        "bash -c 'echo Starting; sleep 2; exit 1'"
    };

    format!(
        r#"
[environment]
name = "restart-env"

[processes.auto-restart-process]
name = "auto-restart-process"
command = "{restart_cmd}"
auto_restart = true
restart_delay = 1000
max_restarts = 3
"#
    )
}

/// Configuration with working directory
pub fn working_dir_config() -> String {
    let pwd_cmd = if cfg!(windows) { "cd" } else { "pwd" };

    format!(
        r#"
[environment]
name = "working-dir-env"

[processes.pwd-process]
name = "pwd-process"
command = "{pwd_cmd}"
working_dir = "src"
auto_restart = false
"#
    )
}

/// Configuration with environment variables
pub fn env_vars_config() -> String {
    let env_cmd = if cfg!(windows) {
        "cmd /c \"set | findstr TEST_VAR\""
    } else {
        "env | grep TEST_VAR"
    };

    format!(
        r#"
[environment]
name = "env-vars-env"

[processes.env-process]
name = "env-process"
command = "{env_cmd}"
auto_restart = false

[processes.env-process.env]
TEST_VAR = "test-value"
ANOTHER_VAR = "another-value"
"#
    )
}

/// Configuration for testing process groups (parent spawns children)
pub fn process_group_config() -> String {
    let parent_cmd = if cfg!(windows) {
        "cmd /c \"echo Parent PID: %RANDOM% && start /b timeout /t 2 /nobreak && start /b timeout /t 4 /nobreak && timeout /t 5 /nobreak\""
    } else {
        "bash -c 'echo Parent PID: $$; sleep 2 && sleep 4 && wait'"
    };

    format!(
        r#"
[environment]
name = "process-group-env"

[processes.parent-process]
name = "parent-process"
command = "{parent_cmd}"
auto_restart = false
"#
    )
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
    let tcp_cmd = if cfg!(windows) {
        format!(
            "powershell -Command \"$listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Any, {port}); $listener.Start(); Start-Sleep 30; $listener.Stop()\""
        )
    } else {
        format!("nc -l {port}")
    };

    format!(
        r#"
[environment]
name = "tcp-env"

[processes.tcp-server]
name = "tcp-server"
command = "{tcp_cmd}"
health_check_url = "tcp://localhost:{port}"
health_check_interval = 1
auto_restart = false
"#
    )
}

/// Configuration with command health check
pub fn command_health_check_config() -> String {
    let (sleep_cmd, health_cmd) = if cfg!(windows) {
        ("timeout /t 10 /nobreak", "cmd://echo healthy")
    } else {
        ("sleep 10", "cmd://echo 'healthy'")
    };

    format!(
        r#"
[environment]
name = "command-env"

[processes.command-process]
name = "command-process"
command = "{sleep_cmd}"
health_check_url = "{health_cmd}"
health_check_interval = 1
auto_restart = false
"#
    )
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
