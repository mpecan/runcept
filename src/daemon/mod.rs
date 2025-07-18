pub mod auto_spawn;
pub mod connection;
pub mod handlers;
pub mod server;

pub use auto_spawn::{
    DaemonAutoSpawner, ensure_daemon_running_for_cli, ensure_daemon_running_for_mcp,
};
pub use connection::*;
pub use handlers::*;
pub use server::{DaemonServer, ServerConfig};
