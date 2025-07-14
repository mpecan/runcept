# Runit

A Rust-based process manager that provides both MCP (Model Context Protocol) server capabilities and a command-line interface for managing long-running processes.

## Features

- **Process Management**: Start, stop, restart, and monitor long-running processes
- **MCP Server**: Provides MCP tools for AI assistants to manage processes
- **CLI Interface**: Command-line tools for direct process management
- **Log Management**: Read and monitor process logs
- **Persistence**: Process state persisted across restarts
- **High Reliability**: Built with Rust for memory safety and performance

## Architecture

The project is structured into modular components:

- `process/`: Core process management and storage
- `mcp/`: MCP server implementation with process management tools
- `cli/`: Command-line interface
- `error.rs`: Centralized error handling
- `config.rs`: Configuration management

## Installation

```bash
cargo build --release
```

## Usage

### CLI Interface

```bash
# List running processes
runit list

# Start a new process
runit start --name "my-service" --command "python server.py"

# Stop a process
runit stop my-service

# Restart a process
runit restart my-service

# View process logs
runit logs my-service

# Follow logs in real-time
runit logs --follow my-service
```

### MCP Server

```bash
# Start the MCP server
runit-mcp
```

The MCP server provides the following tools:
- `start_process`: Start a new managed process
- `stop_process`: Stop a running process
- `restart_process`: Restart a process
- `list_processes`: List all managed processes
- `get_process_logs`: Retrieve process logs
- `get_process_status`: Get detailed process status

## Development

### Testing

```bash
# Run unit tests
cargo test

# Run tests with coverage
cargo test --coverage

# Run integration tests
cargo test --test integration
```

### Code Quality

The project follows these principles:
- **TDD**: Test-driven development with high code coverage
- **File Size Limits**: Soft limit of 300 lines, hard limit of 500 lines per file
- **Modular Design**: Clear separation of concerns
- **Error Handling**: Comprehensive error types and handling

## License

MIT License