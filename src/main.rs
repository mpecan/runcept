use clap::Parser;
use runcept::cli::{CliArgs, CliHandler, CliResult};
use runcept::config::GlobalConfig;
use runcept::daemon::{DaemonServer, ServerConfig};
use runcept::database::Database;
use runcept::logging;
use std::path::PathBuf;
use std::process;
use tracing::info;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Check if this is a daemon process invocation
    if args.len() > 1 && args[1] == "daemon-process" {
        run_daemon_process(args).await;
        return;
    }

    // Parse command line arguments for regular CLI
    let args = CliArgs::parse();

    // Load global configuration for logging
    let global_config = match GlobalConfig::load().await {
        Ok(config) => config,
        Err(_) => {
            // If config loading fails, use default config for logging
            eprintln!("Warning: Failed to load global configuration, using defaults");
            GlobalConfig::default()
        }
    };

    // Initialize CLI logging
    if let Err(e) = logging::init_cli_logging(&global_config) {
        eprintln!("Warning: Failed to initialize logging: {e}");
    }

    // Create CLI handler
    let mut handler = CliHandler::new(args.socket.clone()).with_verbose(args.verbose);

    // Handle the command
    let result = match handler.handle_command(args).await {
        Ok(result) => result,
        Err(e) => CliResult::Error(format!("Failed to execute command: {e}")),
    };

    // Print result and set exit code
    match result {
        CliResult::Success(msg) => {
            println!("{msg}");
            process::exit(0);
        }
        CliResult::Error(msg) => {
            eprintln!("{msg}");
            process::exit(1);
        }
    }
}

/// Run the daemon process
async fn run_daemon_process(args: Vec<String>) {
    let mut socket_path: Option<PathBuf> = None;
    let mut verbose = false;

    // Parse daemon arguments
    let mut i = 2; // Skip program name and "daemon-process"
    while i < args.len() {
        match args[i].as_str() {
            "--socket" => {
                if i + 1 < args.len() {
                    socket_path = Some(PathBuf::from(&args[i + 1]));
                    i += 2;
                } else {
                    eprintln!("Error: --socket requires a path argument");
                    process::exit(1);
                }
            }
            "--verbose" => {
                verbose = true;
                i += 1;
            }
            _ => {
                eprintln!("Error: Unknown daemon argument: {}", args[i]);
                process::exit(1);
            }
        }
    }

    // Use default socket path if not provided
    let socket_path = socket_path.unwrap_or_else(runcept::cli::client::get_default_socket_path);

    if verbose {
        println!("Starting daemon with socket: {socket_path:?}");
    }

    // Load global configuration
    let global_config = match GlobalConfig::load().await {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Failed to load global configuration: {e}");
            process::exit(1);
        }
    };

    // Initialize daemon logging
    if let Err(e) = logging::init_daemon_logging(&global_config) {
        eprintln!("Failed to initialize daemon logging: {e}");
        process::exit(1);
    }

    info!("Starting daemon process");

    // Initialize database
    let database = match setup_database(&global_config).await {
        Ok(db) => {
            info!("Database initialized successfully");
            Some(db)
        }
        Err(e) => {
            eprintln!("Warning: Failed to initialize database: {e}");
            eprintln!("Continuing without database persistence...");
            None
        }
    };

    // Create server configuration
    let server_config = ServerConfig {
        socket_path,
        global_config,
        database,
    };

    // Create and run daemon server
    let mut server = match DaemonServer::new(server_config).await {
        Ok(server) => server,
        Err(e) => {
            eprintln!("Failed to create daemon server: {e}");
            process::exit(1);
        }
    };

    if verbose {
        println!("Daemon server created successfully");
    }

    // Set up signal handling for graceful shutdown
    let server_handle = server.clone_handles();
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for Ctrl+C");
        println!("Received Ctrl+C, shutting down...");
        if let Err(e) = server_handle.request_shutdown().await {
            eprintln!("Error during shutdown: {e}");
        }
    });

    // Run the server
    if let Err(e) = server.run().await {
        eprintln!("Daemon server error: {e}");
        process::exit(1);
    }

    if verbose {
        println!("Daemon stopped");
    }
}

/// Set up the database for the daemon
async fn setup_database(global_config: &GlobalConfig) -> runcept::error::Result<Database> {
    // Use the same directory as daemon logs for the database
    let runcept_dir = global_config.get_log_dir();
    let database_path = runcept_dir.join("runcept.db");
    
    // Create the directory if it doesn't exist
    if let Some(parent) = database_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    
    let database_url = format!("sqlite://{}", database_path.display());
    info!("Initializing database at: {}", database_url);
    
    let database = Database::new(&database_url).await?;
    database.init().await?;
    
    Ok(database)
}
