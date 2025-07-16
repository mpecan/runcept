use runcept::error::Result;
use runcept::mcp::create_mcp_server;
use std::process;

#[tokio::main]
async fn main() -> Result<()> {
    // Create and run the MCP server
    let server = create_mcp_server().await?;

    // Run the server (this will block until the server is stopped)
    if let Err(_) = server.run().await {
        // Don't print to stderr as it interferes with MCP protocol
        // The error will be logged to the MCP server log file
        process::exit(1);
    }

    Ok(())
}
