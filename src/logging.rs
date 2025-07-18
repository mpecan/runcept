use crate::config::GlobalConfig;
use crate::error::{Result, RunceptError};
use std::path::PathBuf;
use std::sync::Once;
use tracing::{debug, error, info, warn};
use tracing_subscriber::{filter::EnvFilter, fmt, prelude::*, registry::Registry};

static LOGGER_INIT: Once = Once::new();

/// Initialize the logging system for a specific component
fn init_component_logging(
    config: &GlobalConfig,
    component: &str,
    log_to_stdout: bool,
) -> Result<()> {
    let mut init_result = Ok(());

    LOGGER_INIT.call_once(|| {
        init_result = init_component_logging_internal(config, component, log_to_stdout);
    });

    init_result
}

/// Internal logging initialization (only called once)
fn init_component_logging_internal(
    config: &GlobalConfig,
    component: &str,
    log_to_stdout: bool,
) -> Result<()> {
    let log_level = config.logging.level.to_lowercase();

    // Create the log directory if file logging is enabled
    let log_file_path = if config.logging.file_enabled {
        let log_dir = if let Some(path) = &config.logging.file_path {
            PathBuf::from(path)
        } else {
            // Use default: ~/.runcept/logs/
            let config_dir = crate::config::global::get_config_dir()?;
            config_dir.join("logs")
        };

        // Create directory if it doesn't exist
        std::fs::create_dir_all(&log_dir).map_err(|e| {
            RunceptError::ConfigError(format!("Failed to create log directory: {e}"))
        })?;

        Some(log_dir.join(format!("{component}.log")))
    } else {
        None
    };

    // Create the filter
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(&log_level))
        .map_err(|e| RunceptError::ConfigError(format!("Invalid log level '{log_level}': {e}")))?;

    // Set up the subscriber
    let registry = Registry::default().with(filter);

    if let Some(ref log_path) = log_file_path {
        // File logging enabled
        let file_appender = tracing_appender::rolling::never(
            log_path.parent().unwrap(),
            format!("{component}.log"),
        );
        let file_layer = fmt::layer()
            .with_writer(file_appender)
            .with_ansi(false)
            .with_target(true)
            .with_thread_ids(true)
            .with_file(true)
            .with_line_number(true);

        if log_to_stdout {
            // Also log to stdout
            let stdout_layer = fmt::layer()
                .with_writer(std::io::stdout)
                .with_ansi(true)
                .with_target(false);

            registry.with(file_layer).with(stdout_layer).init();
        } else {
            // File logging only
            registry.with(file_layer).init();
        }
    } else {
        // Console logging only (if allowed)
        if log_to_stdout {
            let stdout_layer = fmt::layer()
                .with_writer(std::io::stdout)
                .with_ansi(true)
                .with_target(false);

            registry.with(stdout_layer).init();
        } else {
            return Err(RunceptError::ConfigError(
                "File logging must be enabled for components that don't support stdout".to_string(),
            ));
        }
    }

    info!(
        "{} logging initialized with level: {}",
        component, log_level
    );
    if let Some(ref log_path) = log_file_path {
        info!("Log file: {}", log_path.display());
    }

    Ok(())
}

/// Initialize logging for the daemon process
pub fn init_daemon_logging(config: &GlobalConfig) -> Result<()> {
    init_component_logging(config, "daemon", true)?;
    info!("Daemon logging initialized");
    Ok(())
}

/// Initialize logging for CLI commands
pub fn init_cli_logging(config: &GlobalConfig) -> Result<()> {
    // For CLI, we typically want less verbose logging
    let mut cli_config = config.clone();

    // Use info level for CLI unless explicitly set to debug/trace
    if !matches!(
        cli_config.logging.level.to_lowercase().as_str(),
        "debug" | "trace"
    ) {
        cli_config.logging.level = "info".to_string();
    }

    // Enable file logging for CLI with separate log file
    cli_config.logging.file_enabled = true;

    init_component_logging(&cli_config, "cli", true)?;
    debug!("CLI logging initialized");
    Ok(())
}

/// Initialize logging for MCP server - NEVER logs to stdout/stderr
pub fn init_mcp_logging(config: &GlobalConfig) -> Result<()> {
    // Ensure file logging is enabled for MCP
    let mut mcp_config = config.clone();
    mcp_config.logging.file_enabled = true;

    // MCP server never logs to stdout/stderr to avoid protocol interference
    init_component_logging(&mcp_config, "mcp-server", false)?;
    info!("MCP server logging initialized");
    Ok(())
}

/// Log a structured message for daemon operations
pub fn log_daemon_event(event: &str, details: &str) {
    info!(target: "daemon", event = event, details = details);
}

/// Log a structured message for process operations
pub fn log_process_event(process_name: &str, event: &str, details: &str) {
    info!(target: "process", process = process_name, event = event, details = details);
}

/// Log a structured message for environment operations
pub fn log_environment_event(env_name: &str, event: &str, details: &str) {
    info!(target: "environment", env = env_name, event = event, details = details);
}

/// Log an error with context
pub fn log_error(component: &str, error: &str, context: Option<&str>) {
    if let Some(ctx) = context {
        error!(component = component, error = error, context = ctx);
    } else {
        error!(component = component, error = error);
    }
}

/// Log a warning with context
pub fn log_warning(component: &str, warning: &str, context: Option<&str>) {
    if let Some(ctx) = context {
        warn!(component = component, warning = warning, context = ctx);
    } else {
        warn!(component = component, warning = warning);
    }
}

/// Log debug information
pub fn log_debug(component: &str, message: &str, context: Option<&str>) {
    if let Some(ctx) = context {
        debug!(component = component, message = message, context = ctx);
    } else {
        debug!(component = component, message = message);
    }
}

/// Log info information
pub fn log_info(component: &str, message: &str, context: Option<&str>) {
    if let Some(ctx) = context {
        info!(component = component, message = message, context = ctx);
    } else {
        info!(component = component, message = message);
    }
}
