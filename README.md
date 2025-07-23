# Runcept

**Run + Intercept** - A powerful Rust-based process manager that intelligently manages long-running processes with both AI assistant integration and command-line interface capabilities.

## Overview

Runcept simplifies development workflow management by providing:
- **Project-based process orchestration** with dependency management
- **Intelligent auto-shutdown** to save system resources
- **AI assistant integration** via Model Context Protocol (MCP)
- **Comprehensive logging and monitoring**
- **Cross-platform support** (Linux, macOS; Windows support planned)

Perfect for managing complex development environments with multiple services, databases, and background processes.

## Key Features

### 🚀 Process Management
- **Smart orchestration**: Start, stop, restart, and monitor processes with dependency resolution (in theory)
- **Health monitoring**: HTTP, TCP, and command-based health checks
- **Auto-restart**: Configurable automatic restart on process failure (in theory)
- **Graceful shutdown**: Proper signal handling and cleanup

### 📁 Project-based Configuration
- **`.runcept.toml` files**: Define all project processes in a single configuration
- **Environment activation**: One command to start/stop entire development environments
- **Working directory management**: Flexible path resolution and environment variables

### ⚡ Resource Management
- **Inactivity timeout**: Automatically stop unused processes to save resources
- **Activity tracking**: Monitor process usage and optimize resource allocation
- **Persistent state**: SQLite database ensures process state survives restarts

### 🤖 AI Integration
- **MCP Server**: Native support for AI assistants (Claude, etc.)
- **Intelligent management**: AI can start, stop, and monitor your development environment
- **Context-aware**: AI understands your project structure and dependencies

### 🛠️ Developer Experience
- **Rich CLI**: Intuitive command-line interface with comprehensive process control
- **Real-time logs**: Request process logs with limits (more to come)
- **Cross-platform**: Works seamlessly on Linux and macOS (Windows support in development)
- **High performance**: Built with Rust for speed and reliability

## Architecture

The project is structured into modular components:

- `process/`: Core process management and storage
- `config/`: Configuration management and project activation
- `mcp/`: MCP server implementation with process management tools
- `cli/`: Command-line interface
- `database/`: SQLite database for process persistence
- `error.rs`: Centralized error handling

## Quick Start

### Installation

**Prerequisites:**
- Rust 1.70+ (install from [rustup.rs](https://rustup.rs/))
- Git
- **Supported Platforms:** Linux, macOS (Windows support coming soon)

**Install from source:**
```bash
git clone https://github.com/mpecan/runcept.git
cd runcept
cargo build --release

# Add to PATH (optional)
cp target/release/runcept ~/.local/bin/  # Linux/macOS
# or add target/release/ to your PATH
```

### Initialize Your First Project

```bash
# Navigate to your project directory
cd my-web-app

# Initialize runcept configuration
runcept init

# Edit the generated .runcept.toml file
# Then activate your environment
runcept activate
```

## Usage

### Global Options

All commands support these global options:
- `-v, --verbose`: Enable verbose output for debugging
- `--socket <PATH>`: Override default daemon socket path

### Environment Options

Most process commands support:
- `-e, --environment <PATH>`: Specify environment path (defaults to current directory)

## Configuration

### Basic Project Setup

Runcept uses `.runcept.toml` files to define your project's processes. Here's a complete example:

```toml
[project]
name = "my-web-app"
description = "Full-stack web application development environment"
inactivity_timeout = "30m"  # Auto-shutdown after 30 minutes of inactivity

[[process]]
name = "database"
command = "postgres -D /usr/local/var/postgres"
working_dir = "."
auto_restart = true
health_check = "tcp://localhost:5432"
health_check_interval = "10s"

[[process]]
name = "redis"
command = "redis-server"
working_dir = "."
auto_restart = true
health_check = "tcp://localhost:6379"

[[process]]
name = "web-server"
command = "python manage.py runserver"
working_dir = "."
env = { DEBUG = "true", PORT = "8000", DATABASE_URL = "postgresql://localhost/myapp" }
auto_restart = true
health_check = "http://localhost:8000/health"
depends_on = ["database", "redis"]  # Start after database and redis

[[process]]
name = "worker"
command = "celery worker -A app.celery"
working_dir = "."
depends_on = ["redis", "database"]
auto_restart = true

[[process]]
name = "frontend"
command = "npm run dev"
working_dir = "./frontend"
env = { NODE_ENV = "development" }
depends_on = ["web-server"]
```

### Configuration Options

| Field                   | Description                                   | Example                             |
|-------------------------|-----------------------------------------------|-------------------------------------|
| `name`                  | Unique process identifier                     | `"web-server"`                      |
| `command`               | Command to execute                            | `"python manage.py runserver"`      |
| `working_dir`           | Working directory (relative to .runcept.toml) | `"."` or `"./backend"`              |
| `env`                   | Environment variables                         | `{ DEBUG = "true", PORT = "8000" }` |
| `auto_restart`          | Restart on failure                            | `true` or `false`                   |
| `health_check`          | Health check URL/command                      | `"http://localhost:8000/health"`    |
| `health_check_interval` | Check frequency                               | `"30s"`, `"1m"`, `"5m"`             |
| `depends_on`            | Process dependencies                          | `["database", "redis"]`             |

## Usage

### Environment Management

```bash
# Initialize a new project
runcept init                    # Creates .runcept.toml template

# Environment lifecycle
runcept activate               # Start all processes in dependency order
runcept deactivate            # Stop all processes gracefully
runcept status                # Show environment and process status

# Quick project switching
runcept activate /path/to/other/project  # Activate different project
```

### Process Management

```bash
# List and inspect processes
runcept list                   # List processes in current environment
runcept ps                     # List all processes across environments
runcept status                 # Show environment and process status

# Process lifecycle
runcept start web-server       # Start specific process
runcept stop web-server        # Stop specific process gracefully
runcept restart web-server     # Restart process

# Cross-environment process management
runcept start web-server -e /path/to/project    # Start process in specific environment
runcept stop web-server -e /path/to/project     # Stop process in specific environment
runcept restart web-server -e /path/to/project  # Restart process in specific environment

# Emergency operations
runcept kill-all              # Stop all processes in all environments
```

### Log Management

```bash
# View logs
runcept logs web-server        # Show recent logs
runcept logs web-server -n 100   # Show last 100 lines
runcept logs web-server --lines 100  # Show last 100 lines (alternative)

# Cross-environment logs
runcept logs web-server -e /path/to/project  # Get logs from specific environment

# Note: Follow mode (-f, --follow) is defined but not yet implemented
```

### Daemon Management

```bash
# Daemon control
runcept daemon start          # Start runcept daemon in background
runcept daemon start -f       # Start daemon in foreground
runcept daemon start --foreground  # Start daemon in foreground (alternative)
runcept daemon stop           # Stop daemon gracefully
runcept daemon status         # Check daemon status
runcept daemon restart        # Restart daemon
```

## AI Assistant Integration (MCP)

Runcept provides native integration with AI assistants through the Model Context Protocol (MCP).

### Setup MCP Server

```bash
# Start the MCP server (usually done automatically)
runcept mcp

# Or configure in your AI assistant's MCP settings
# Server command: runcept mcp
# Server args: []
```

### Available MCP Tools

**Environment Management:**
- `activate_environment` - Activate a project environment
- `deactivate_environment` - Deactivate current environment  
- `list_environments` - List all available projects
- `get_environment_status` - Get detailed environment status

**Process Management:**
- `start_process` - Start specific processes
- `stop_process` - Stop running processes
- `restart_process` - Restart processes
- `list_processes` - List processes in current environment
- `get_process_logs` - Retrieve process logs with filtering
- `get_process_status` - Get detailed process information

**Global Operations:**
- `list_all_processes` - List processes across all environments
- `kill_all_processes` - Emergency stop all processes

### Example AI Interactions

```
You: "Start my web development environment"
AI: *uses activate_environment tool to start your project*

You: "The web server seems slow, check its logs"
AI: *uses get_process_logs to retrieve and analyze web-server logs*

You: "Restart the database and check if the web server is healthy"
AI: *uses restart_process for database, then checks web-server health*
```

## Examples

### Web Development Stack

```toml
[project]
name = "fullstack-app"
description = "React + Node.js + PostgreSQL development environment"
inactivity_timeout = "45m"

[[process]]
name = "postgres"
command = "postgres -D /usr/local/var/postgres"
working_dir = "."
auto_restart = true
health_check = "tcp://localhost:5432"

[[process]]
name = "backend"
command = "npm run dev"
working_dir = "./backend"
env = { NODE_ENV = "development", DATABASE_URL = "postgresql://localhost/myapp" }
depends_on = ["postgres"]
health_check = "http://localhost:3001/health"

[[process]]
name = "frontend"
command = "npm start"
working_dir = "./frontend"
env = { REACT_APP_API_URL = "http://localhost:3001" }
depends_on = ["backend"]
```

### Microservices Environment

```toml
[project]
name = "microservices"
description = "Microservices development environment"

[[process]]
name = "redis"
command = "redis-server"
working_dir = "."
health_check = "tcp://localhost:6379"

[[process]]
name = "user-service"
command = "python -m uvicorn main:app --reload --port 8001"
working_dir = "./services/user"
depends_on = ["redis"]
health_check = "http://localhost:8001/health"

[[process]]
name = "order-service"
command = "python -m uvicorn main:app --reload --port 8002"
working_dir = "./services/order"
depends_on = ["redis", "user-service"]
health_check = "http://localhost:8002/health"

[[process]]
name = "api-gateway"
command = "node server.js"
working_dir = "./gateway"
depends_on = ["user-service", "order-service"]
health_check = "http://localhost:8000/health"
```

## Platform Support

### Supported Platforms
- ✅ **Linux**: Fully supported and tested
- ✅ **macOS**: Fully supported and tested
- ⚠️ **Windows**: In development (use WSL2 as workaround)

### Windows Status
Windows support is currently incomplete due to platform-specific dependencies:
- Process management relies on Unix process groups and signals
- IPC uses Unix domain sockets (though Windows named pipes are partially implemented)
- Signal handling (SIGTERM/SIGKILL) needs Windows equivalents

**Windows users** can use WSL2 (Windows Subsystem for Linux) to run runcept with full functionality.

## Troubleshooting

### Common Issues

**Process won't start:**
- Check if the command is available in PATH
- Verify working directory exists
- Check environment variables are set correctly
- Review process logs: `runcept logs <process-name>`

**Health checks failing:**
- Ensure health check URL/port is correct
- Check if process is actually listening on expected port
- Verify health check endpoint returns 200 status
- Increase health_check_interval if process is slow to start

**Dependency issues:**
- Check dependency names match process names exactly
- Verify there are no circular dependencies
- Use `runcept status` to see dependency resolution order

**Permission errors:**
- Ensure runcept has permission to execute commands
- Check file permissions on scripts and executables
- Verify working directory permissions

### Getting Help

- Check logs: `runcept logs <process-name>`
- Daemon status: `runcept daemon status`
- Verbose output: `runcept --verbose <command>` or `runcept -v <command>`
- List all processes: `runcept ps`
- Environment status: `runcept status`

## Development

### Development Workflow

This project uses a **PR-based development workflow** with automated releases:

#### Branch Protection
- **Main branch is protected** - no direct pushes allowed
- All changes must go through **Pull Requests**
- **CI checks must pass** before merging
- **At least one approval** required for PRs
- **Linear git history** enforced (squash/rebase merging)

#### Making Changes
1. **Create a feature branch** from `main`:
   ```bash
   git checkout -b feat/your-feature-name
   ```

2. **Make your changes** following conventional commits:
   ```bash
   git commit -m "feat: add process health monitoring"
   git commit -m "fix: resolve memory leak in process cleanup"
   git commit -m "docs: update README with installation instructions"
   ```

3. **Push your branch** and create a Pull Request:
   ```bash
   git push -u origin feat/your-feature-name
   ```

4. **Address review feedback** and ensure CI passes

5. **Merge via GitHub** once approved (squash merge preferred)

#### Automated Releases
- **Release Please** automatically creates release PRs based on conventional commits
- **Semantic versioning** is automatically determined from commit messages
- **CHANGELOG.md** is automatically updated
- **GitHub releases** are created automatically when release PRs are merged

#### Commit Message Format
Use [Conventional Commits](https://www.conventionalcommits.org/) format:

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

**Types:**
- `feat:` - New features
- `fix:` - Bug fixes  
- `docs:` - Documentation changes
- `style:` - Code style changes (formatting, etc.)
- `refactor:` - Code refactoring
- `test:` - Adding or updating tests
- `chore:` - Maintenance tasks

**Examples:**
```bash
feat: add process dependency resolution
fix: resolve daemon connection timeout issue
docs: update installation guide for Windows
refactor: simplify process lifecycle management
test: add integration tests for MCP server
chore: update dependencies to latest versions
```

### Building from Source

```bash
git clone https://github.com/mpecan/runcept.git
cd runcept
cargo build --release
```

### Running Tests

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run integration tests
cargo test --test integration

# Check code quality
cargo clippy -- -D warnings
cargo fmt --check

# Pre-commit checks (run before pushing)
cargo fmt && cargo clippy -- -D warnings && cargo test
```

### Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes with tests
4. Ensure all tests pass and code is formatted
5. Submit a pull request

The project follows these principles:
- **TDD**: Test-driven development with high code coverage
- **File Size Limits**: Soft limit of 300 lines, hard limit of 500 lines per file
- **Modular Design**: Clear separation of concerns
- **Error Handling**: Comprehensive error types and handling

See [CLAUDE.md](CLAUDE.md) for detailed development guidelines.

## Documentation

- **[README.md](README.md)** - Main documentation with usage examples and configuration
- **[docs/INSTALLATION.md](docs/INSTALLATION.md)** - Detailed installation and setup guide
- **[CLAUDE.md](CLAUDE.md)** - Development guidelines and coding standards

## Architecture

The project is structured into modular components:

- `process/` - Core process management and lifecycle
- `config/` - Configuration parsing and project management  
- `mcp/` - Model Context Protocol server implementation
- `cli/` - Command-line interface and user interaction
- `database/` - SQLite persistence layer
- `scheduler/` - Health monitoring and inactivity management
- `daemon/` - Background daemon and IPC communication

## License

MIT License - see [LICENSE](LICENSE) for details.