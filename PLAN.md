# Runcept Implementation Plan

## Project Overview

Build a Rust-based process manager with dual interfaces and project-based configuration:
1. **MCP Server**: Provides AI assistants with process and environment management capabilities
2. **CLI Interface**: Direct command-line process and environment management
3. **Project Configuration**: `.runcept.toml` files define project-specific processes
4. **Environment Activation**: Activate/deactivate entire project environments
5. **Auto-shutdown**: Configurable inactivity timeouts for resource management

## Configuration Format (.runcept.toml)

```toml
[project]
name = "my-web-app"
description = "Development environment for web application"
inactivity_timeout = "30m"  # Auto-shutdown after 30 minutes of inactivity
working_dir = "."           # Base working directory for all processes

[[process]]
name = "web-server"
command = "python manage.py runserver"
working_dir = "."
env = { DEBUG = "true", PORT = "8000" }
auto_restart = true
health_check = "http://localhost:8000/health"
health_check_interval = "30s"
depends_on = ["database"]

[[process]]
name = "database"
command = "postgres -D /usr/local/var/postgres"
working_dir = "."
auto_restart = true
health_check = "tcp://localhost:5432"

[[process]]
name = "redis"
command = "redis-server"
working_dir = "."
auto_restart = true
health_check = "tcp://localhost:6379"

[[process]]
name = "worker"
command = "celery worker -A app.celery"
working_dir = "."
depends_on = ["redis", "database"]
auto_restart = true
```

## Architecture Design

### Module Structure
```
src/
├── main.rs              # CLI entry point
├── mcp_main.rs          # MCP server entry point
├── lib.rs               # Library exports
├── process/             # Core process management (≤300 lines each)
│   ├── mod.rs          # Module exports
│   ├── manager.rs      # ProcessManager - lifecycle management
│   ├── process.rs      # Process struct and state
│   └── monitor.rs      # Process monitoring and health checks
├── config/             # Configuration system
│   ├── mod.rs          # Module exports
│   ├── project.rs      # .runcept.toml parsing and validation
│   ├── environment.rs  # Environment activation/deactivation
│   └── settings.rs     # Global settings and defaults
├── database/           # SQLite persistence layer
│   ├── mod.rs          # Module exports
│   ├── schema.rs       # Database schema definitions
│   ├── migrations.rs   # Database migrations
│   └── queries.rs      # Database operations
├── mcp/                # MCP server implementation
│   ├── mod.rs          # Module exports
│   ├── server.rs       # MCP protocol handler
│   ├── tools.rs        # MCP tool implementations
│   └── environment_tools.rs # Environment-specific MCP tools
├── cli/                # Command line interface
│   ├── mod.rs          # Module exports
│   ├── commands.rs     # CLI command handlers
│   └── environment_commands.rs # Environment-specific CLI commands
├── scheduler/          # Auto-shutdown and monitoring
│   ├── mod.rs          # Module exports
│   ├── inactivity.rs   # Inactivity tracking and auto-shutdown
│   └── health.rs       # Health check scheduling
└── error.rs            # Error types and handling
```

## Implementation Phases (TDD Approach)

### Phase 1: Core Foundation
1. **Error Handling System** (`error.rs`)
   - ✅ Write tests for error types and conversions
   - ✅ Implement custom error types for different failure modes
   - ✅ Implement error conversion traits and user-friendly messages

2. **Database Layer** (`database/`)
   - ✅ Write tests for database operations and migrations
   - ✅ Implement SQLite database setup with migrations
   - ✅ Implement schema for processes, environments, and activity tracking
   - ✅ Implement database operations and connection management
   - ✅ Implement comprehensive environment CRUD operations
   - ✅ Implement environment search and filtering capabilities
   - ✅ Implement activity tracking and cleanup operations

3. **Process Representation** (`process/process.rs`)
   - ✅ Write tests for process struct and state transitions
   - ✅ Implement process struct with metadata
   - ✅ Implement process state enum (Running, Stopped, Failed, etc.)
   - ✅ Implement serialization support

### Phase 2: Configuration System
4. **Global Configuration** (`config/global.rs`)
   - ✅ Write tests for global configuration loading and saving
   - ✅ Implement global configuration in ~/.runit/config.toml
   - ✅ Write tests for default settings and user preferences
   - ✅ Implement default settings and user preferences
   - ✅ Write tests for global environment variables and paths
   - ✅ Implement global environment variables and paths

5. **Project Configuration** (`config/project.rs`)
   - ✅ Write tests for .runcept.toml parsing and validation
   - ✅ Implement .runcept.toml parsing and validation
   - ✅ Write tests for process dependency resolution
   - ✅ Implement process dependency resolution
   - ✅ Write tests for configuration merging (global + project)
   - ✅ Implement configuration merging (global + project)

6. **Environment Management** (`config/environment.rs`)
   - ✅ Write tests for environment activation/deactivation
   - ✅ Implement environment activation/deactivation logic
   - ✅ Write tests for project discovery and registration
   - ✅ Implement project discovery and registration
   - ✅ Write tests for configuration inheritance
   - ✅ Implement configuration inheritance
   - ✅ Integrate database persistence with EnvironmentManager
   - ✅ Implement hybrid in-memory + database architecture
   - ✅ Implement automatic environment state synchronization
   - ✅ Implement database-backed environment search and discovery

### Phase 3: Process Management
7. **Process Manager** (`process/manager.rs`)
   - ✅ Write tests for process lifecycle operations
   - ✅ Implement process lifecycle operations (start, stop, restart)
   - ✅ Write tests for signal handling and cleanup
   - ✅ Implement signal handling and cleanup
   - ✅ Write tests for async process spawning and monitoring
   - ✅ Implement async process spawning and monitoring
   - ✅ Write tests for environment variable injection
   - ✅ Implement environment variable injection
   - ✅ Write tests for batch operations (stop all, cleanup)
   - ✅ Implement batch operations (stop all, cleanup)

8. **Process Monitoring** (`process/monitor.rs`)
   - ✅ Write tests for health check execution
   - ✅ Implement health check execution
   - ✅ Write tests for process status tracking
   - ✅ Implement process status tracking
   - ✅ Write tests for crash detection and auto-restart
   - ✅ Implement crash detection and auto-restart

### Phase 4: Scheduling & Auto-shutdown
8. **Inactivity Tracking** (`scheduler/inactivity.rs`)
   - ✅ Write tests for activity monitoring
   - ✅ Implement activity monitoring per process and environment
   - ✅ Write tests for configurable timeout handling
   - ✅ Implement configurable timeout handling
   - ✅ Write tests for graceful shutdown sequences
   - ✅ Implement graceful shutdown sequences

9. **Health Monitoring** (`scheduler/health.rs`)
    - ✅ Write tests for periodic health check execution
    - ✅ Implement periodic health check execution
    - ✅ Write tests for health status reporting
    - ✅ Implement health status reporting
    - ✅ Write tests for failure notification system
    - ✅ Implement failure notification system
### Phase 5: Interfaces
10. **CLI Implementation** (`cli/`)
    - ✅ Write tests for environment commands
    - ✅ Implement environment commands (activate, deactivate, status)
    - ✅ Write tests for process commands with environment awareness
    - ✅ Implement process commands with environment awareness
    - ✅ Write tests for interactive command handlers
    - ✅ Implement interactive command handlers

11. **Daemon Server Implementation** (`daemon/`)
    - ✅ Write tests for daemon server and RPC communication
    - ✅ Implement daemon server with Unix socket communication
    - ✅ Implement process management through daemon
    - ✅ Implement environment management through daemon
    - ✅ Implement proper shutdown mechanism
    - ✅ Implement process log retrieval system
    - ✅ Integrate database with DaemonServer
    - ✅ Implement automatic database initialization on daemon start
    - ✅ Enable persistent environment management across daemon restarts

12. **Process Logging System** (`process/logging.rs`)
    - ✅ Write tests for process output logging
    - ✅ Implement process output capture (stdout/stderr)
    - ✅ Implement JSON Line format for structured logging
    - ✅ Implement log storage in .runcept/logs/ directory
    - ✅ Implement log retrieval for running and finished processes
    - ✅ Implement backward compatibility with legacy formats

13. **System Logging** (`logging.rs`)
    - ✅ Implement comprehensive system logging with tracing
    - ✅ Implement daemon logging to $HOME/.runcept/logs/daemon.log
    - ✅ Implement configurable log levels
    - ✅ Implement structured logging for debugging

14. **MCP Server** (`mcp/`)
    - ✅ Write tests for MCP protocol implementation
    - ✅ Implement MCP protocol implementation with rmcp 0.2.1
    - ✅ Write tests for environment management tools
    - ✅ Implement environment management tools (activate, deactivate, status)
    - ✅ Write tests for process management tools
    - ✅ Implement process management tools (start, stop, restart, list, logs)
    - ✅ Implement daemon communication via Unix socket
    - ✅ Implement proper error handling and response formatting
    - ✅ Implement environment-level activity tracking integration
    - ✅ Add environment state management to MCP tools
    - ✅ Integrate activity tracking into all MCP operations
    - ✅ Write tests for activity tracking integration

### Phase 6: Database Integration & Persistence
15. **Database Integration** (`config/environment.rs`, `daemon/server.rs`, `main.rs`)
    - ✅ Implement QueryManager with comprehensive environment operations
    - ✅ Integrate database with EnvironmentManager for hybrid architecture
    - ✅ Add database initialization to daemon startup
    - ✅ Implement automatic environment persistence on state changes
    - ✅ Enable environment loading from database on daemon restart
    - ✅ Implement path canonicalization for environment consistency
    - ✅ Add database-backed environment search and filtering
    - ✅ **Manual Testing**: Database integration verified with test environment
    - ✅ **Persistence Testing**: Environment data survives daemon restarts
    - ✅ **State Management**: All environment transitions saved to database

### Phase 7: Integration & End-to-End Testing
16. **Binary Integration Tests**
    - ✅ **End-to-End Testing**: Comprehensive binary integration tests created
    - ✅ **CLI Testing**: Environment activation, process management via real CLI
    - ✅ **Daemon Communication**: Unix socket communication testing
    - ✅ **Database Persistence**: Environment state survives daemon restarts
    - ✅ **Error Handling**: CLI error cases and edge conditions
    - ✅ **Concurrent Operations**: Multiple simultaneous CLI commands
    - ✅ **MCP Server Binary**: MCP server startup and basic functionality
    - ✅ **Log File Creation**: Process and daemon logging verification
    - ✅ **System Integration**: All 11 integration tests passing

## Key Features to Implement

### Configuration System
- **.runcept.toml Files**: Project-specific process definitions
- **Environment Activation**: Activate/deactivate entire project environments
- **Process Dependencies**: Define startup order and dependencies
- **Auto-shutdown**: Configurable inactivity timeouts
- **Health Checks**: HTTP/TCP/command-based health monitoring
- **Global Settings**: User-wide configuration and defaults

### Process Management
- **Start Process**: Launch with command, arguments, environment
- **Stop Process**: Graceful shutdown with SIGTERM, force with SIGKILL
- **Restart Process**: Stop and start cycle with dependency handling
- **List Processes**: Show all managed processes with status
- **Process Status**: Detailed information (PID, uptime, memory, etc.)
- **Log Management**: Capture stdout/stderr, log rotation
- **Activity Tracking**: Monitor process activity for auto-shutdown

### MCP Tools
**Environment Management:**
- `activate_environment`: Activate a project environment from .runcept.toml
- `deactivate_environment`: Deactivate current environment
- `list_environments`: List all available project environments
- `get_environment_status`: Get detailed environment status

**Process Management:**
- `start_process`: Start a specific process in the current environment
- `stop_process`: Stop a running process
- `restart_process`: Restart a process
- `list_processes`: List all processes in current environment
- `get_process_logs`: Retrieve process logs
- `get_process_status`: Get detailed process status

**Global Operations:**
- `list_all_processes`: List processes across all environments
- `kill_all_processes`: Emergency stop all processes

### CLI Commands
**Environment Commands:**
- `runit activate [path]`: Activate project environment
- `runit deactivate`: Deactivate current environment
- `runit status`: Show environment and process status

**Process Commands:**
- `runit start <name>`: Start a specific process
- `runit stop <name>`: Stop a specific process
- `runit restart <name>`: Restart a specific process
- `runit list`: List processes in current environment
- `runit logs <name> [--follow] [--lines N]`: View process logs

**Global Commands:**
- `runit ps`: List all processes across all environments
- `runit kill-all`: Stop all processes in all environments

## Technical Considerations

### File Size Management
- Keep modules under 300 lines (soft limit)
- Hard limit of 500 lines per file
- Extract shared utilities to separate modules
- Use composition over large implementations

### Testing Strategy
- **Unit Tests**: Test individual functions and methods
- **Integration Tests**: Test component interactions
- **End-to-End Tests**: Test full CLI and MCP workflows
- **Property Tests**: Test edge cases and invariants

### Error Handling
- Use `thiserror` for structured errors
- Provide context with `anyhow` where appropriate
- User-friendly error messages for CLI
- Structured error responses for MCP

### Async Design
- Use Tokio for async runtime
- Non-blocking I/O for process communication
- Concurrent process management
- Signal handling for graceful shutdown

## Dependencies

### Core Dependencies
- `tokio`: Async runtime and utilities
- `serde`: Serialization framework
- `clap`: CLI argument parsing
- `anyhow`/`thiserror`: Error handling
- `uuid`: Process ID generation
- `chrono`: Timestamp management

### System Dependencies
- `nix`: Unix system calls and signals
- `signal-hook`: Signal handling
- `signal-hook-tokio`: Async signal handling

### Development Dependencies
- `tokio-test`: Async testing utilities
- `tempfile`: Temporary file management for tests
- `assert_cmd`: CLI testing
- `predicates`: Test assertions

## Success Criteria

1. **Functionality**: All process management operations work correctly
2. **Reliability**: Processes are properly managed across restarts
3. **Performance**: Low overhead process monitoring
4. **Code Quality**: High test coverage, clear module boundaries
5. **Usability**: Intuitive CLI and comprehensive MCP tools
6. **Maintainability**: Well-documented, modular codebase