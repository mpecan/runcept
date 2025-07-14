use clap::Parser;
use runcept::cli::{CliArgs, CliHandler, CliResult};
use std::process;

#[tokio::main]
async fn main() {
    // Parse command line arguments
    let args = CliArgs::parse();

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