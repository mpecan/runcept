pub mod auto_spawn;
pub mod server;

pub use auto_spawn::{
    DaemonAutoSpawner, ensure_daemon_running_for_cli, ensure_daemon_running_for_mcp,
};
pub use server::{DaemonServer, ServerConfig};
