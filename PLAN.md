# Runit Implementation Plan

## Project Overview

Build a Rust-based process manager with dual interfaces:
1. **MCP Server**: Provides AI assistants with process management capabilities
2. **CLI Interface**: Direct command-line process management

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
│   └── storage.rs      # Persistence layer (JSON/file-based)
├── mcp/                # MCP server implementation
│   ├── mod.rs          # Module exports
│   ├── server.rs       # MCP protocol handler
│   └── tools.rs        # MCP tool implementations
├── cli/                # Command line interface
│   ├── mod.rs          # Module exports
│   └── commands.rs     # CLI command handlers
├── error.rs            # Error types and handling
└── config.rs           # Configuration management
```

## Implementation Phases

### Phase 1: Core Foundation
1. **Error Handling System** (`error.rs`)
   - Custom error types for different failure modes
   - Error conversion traits
   - User-friendly error messages

2. **Configuration Management** (`config.rs`)
   - Default storage locations
   - Process configuration validation
   - Environment variable support

3. **Process Representation** (`process/process.rs`)
   - Process struct with metadata
   - Process state enum (Running, Stopped, Failed, etc.)
   - Serialization support

### Phase 2: Process Management
4. **Storage Layer** (`process/storage.rs`)
   - JSON-based process persistence
   - Atomic file operations
   - Process registry management

5. **Process Manager** (`process/manager.rs`)
   - Process lifecycle operations (start, stop, restart)
   - Signal handling and cleanup
   - Log capture and management
   - Process monitoring and health checks

### Phase 3: Interfaces
6. **CLI Implementation** (`cli/commands.rs`)
   - Command parsing with clap
   - Interactive command handlers
   - Output formatting and error display

7. **MCP Server** (`mcp/server.rs`, `mcp/tools.rs`)
   - MCP protocol implementation
   - Tool registration and dispatch
   - Async request/response handling

### Phase 4: Testing & Quality
8. **Unit Tests**
   - Test each module in isolation
   - Mock external dependencies
   - Achieve >90% code coverage

9. **Integration Tests**
   - End-to-end CLI testing
   - MCP server functionality testing
   - Process lifecycle testing

## Key Features to Implement

### Process Management
- **Start Process**: Launch with command, arguments, environment
- **Stop Process**: Graceful shutdown with SIGTERM, force with SIGKILL
- **Restart Process**: Stop and start cycle
- **List Processes**: Show all managed processes with status
- **Process Status**: Detailed information (PID, uptime, memory, etc.)
- **Log Management**: Capture stdout/stderr, log rotation

### MCP Tools
- `start_process`: Start a new managed process
- `stop_process`: Stop a running process  
- `restart_process`: Restart a process
- `list_processes`: List all managed processes
- `get_process_logs`: Retrieve process logs
- `get_process_status`: Get detailed process status

### CLI Commands
- `runit start --name <name> --command <cmd> [--env KEY=VALUE]`
- `runit stop <name>`
- `runit restart <name>`
- `runit list`
- `runit status <name>`
- `runit logs <name> [--follow] [--lines N]`

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