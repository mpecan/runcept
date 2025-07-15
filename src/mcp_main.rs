use runcept::error::Result;
use runcept::mcp::create_mcp_server;
use std::process;

#[tokio::main]
async fn main() -> Result<()> {
    // Create and run the MCP server
    let server = create_mcp_server().await?;

    // Run the server (this will block until the server is stopped)
    if let Err(e) = server.run().await {
        eprintln!("MCP server error: {e}");
        process::exit(1);
    }

    Ok(())
}
