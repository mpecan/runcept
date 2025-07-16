use crate::config::GlobalConfig;
use crate::error::{Result, RunceptError};
use crate::logging::{init_mcp_logging, log_daemon_event};
use crate::mcp::tools::RunceptTools;
use rmcp::service::ServiceExt;
use rmcp::transport::io;

/// MCP server configuration
#[derive(Debug, Clone)]
pub struct McpServerConfig {
    pub global_config: GlobalConfig,
    pub server_name: String,
    pub server_version: String,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            global_config: GlobalConfig::default(),
            server_name: "runcept-mcp".to_string(),
            server_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// MCP server handler that implements the ServerHandler trait
pub struct RunceptMcpServer {
    config: McpServerConfig,
    tools: RunceptTools,
}

impl RunceptMcpServer {
    /// Create a new MCP server
    pub fn new(config: McpServerConfig) -> Self {
        Self {
            config,
            tools: RunceptTools::new(),
        }
    }

    /// Start the MCP server
    pub async fn run(self) -> Result<()> {
        // Initialize MCP-specific logging (never logs to stdout/stderr)
        init_mcp_logging(&self.config.global_config)?;
        log_daemon_event("mcp_server_start", "MCP server starting");

        // Ensure daemon is running at startup
        let config_dir = crate::config::global::get_config_dir()?;
        let socket_path = config_dir.join("daemon.sock");

        if let Err(e) = crate::daemon::ensure_daemon_running_for_mcp(socket_path).await {
            return Err(RunceptError::ProcessError(format!(
                "Failed to ensure daemon is running during MCP startup: {e}"
            )));
        }

        log_daemon_event(
            "mcp_server_daemon_ready",
            "Daemon is ready for MCP requests",
        );

        // Create stdio transport
        let transport = io::stdio();

        // Serve the MCP server using the tools as the handler
        let service = self
            .tools
            .serve(transport)
            .await
            .map_err(|e| RunceptError::ProcessError(format!("MCP server error: {e}")))?;

        service
            .waiting()
            .await
            .map_err(|e| RunceptError::ProcessError(format!("MCP server error: {e}")))?;

        log_daemon_event("mcp_server_stop", "MCP server stopped");
        Ok(())
    }
}

/// Create and configure MCP server
pub async fn create_mcp_server() -> Result<RunceptMcpServer> {
    let global_config = GlobalConfig::load().await?;
    let config = McpServerConfig {
        global_config,
        server_name: "runcept-mcp".to_string(),
        server_version: env!("CARGO_PKG_VERSION").to_string(),
    };

    Ok(RunceptMcpServer::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_server_config_default() {
        let config = McpServerConfig::default();
        assert_eq!(config.server_name, "runcept-mcp");
        assert_eq!(config.server_version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn test_mcp_server_creation() {
        let config = McpServerConfig::default();
        let server = RunceptMcpServer::new(config.clone());
        assert_eq!(server.config.server_name, config.server_name);
    }

    #[tokio::test]
    async fn test_create_mcp_server() {
        let server = create_mcp_server().await.unwrap();
        assert_eq!(server.config.server_name, "runcept-mcp");
    }
}
