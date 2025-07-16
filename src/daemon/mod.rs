pub mod server;
pub mod auto_spawn;

pub use server::{DaemonServer, ServerConfig};
pub use auto_spawn::{DaemonAutoSpawner, ensure_daemon_running_for_cli, ensure_daemon_running_for_mcp};
