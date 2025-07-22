# Runcept

**Run + Intercept** - A Rust-based process manager that runs and intercepts synchronous processes, providing both MCP (Model Context Protocol) server capabilities and a command-line interface for intelligent process management.

## Features

- **Process Interception**: Run and intercept synchronous processes with intelligent management
- **Process Management**: Start, stop, restart, and monitor long-running processes
- **Project-based Configuration**: Define processes per project with `.runcept.toml` files
- **Environment Activation**: Activate/deactivate project environments with all their processes
- **Auto-shutdown**: Automatically stop inactive processes after configurable timeout
- **MCP Server**: Provides MCP tools for AI assistants to manage processes and environments
- **CLI Interface**: Command-line tools for direct process and environment management
- **Log Management**: Read and monitor process logs
- **Persistence**: Process state persisted in SQLite database
- **High Reliability**: Built with Rust for memory safety and performance

## Architecture

The project is structured into modular components:

- `process/`: Core process management and storage
- `config/`: Configuration management and project activation
- `mcp/`: MCP server implementation with process management tools
- `cli/`: Command-line interface
- `database/`: SQLite database for process persistence
- `error.rs`: Centralized error handling

## Installation

```bash
cargo build --release
```

## Usage

### Project Configuration

Create a `.runcept.toml` file in your project root:

```toml
[project]
name = "my-web-app"
description = "Development environment for web application"
inactivity_timeout = "30m"  # Auto-shutdown after 30 minutes of inactivity

[[process]]
name = "web-server"
command = "python manage.py runserver"
working_dir = "."
env = { DEBUG = "true", PORT = "8000" }
auto_restart = true
health_check = "http://localhost:8000/health"

[[process]]
name = "redis"
command = "redis-server"
working_dir = "."
auto_restart = true

[[process]]
name = "worker"
command = "celery worker -A app.celery"
working_dir = "."
depends_on = ["redis"]
```

### CLI Interface

```bash
# Environment management
runit activate          # Activate current project environment
runit deactivate        # Deactivate current project environment
runit status            # Show environment and process status

# Process management
runit list              # List all processes in current environment
runit start web-server  # Start a specific process
runit stop web-server   # Stop a specific process
runit restart web-server # Restart a specific process
runit logs web-server   # View process logs
runit logs --follow web-server # Follow logs in real-time

# Global process management
runit ps                # List all processes across all environments
runit kill-all          # Stop all processes in all environments
```

### MCP Server

```bash
# Start the MCP server
runit-mcp
```

The MCP server provides the following tools:

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

3. **Push and create a PR**:
   ```bash
   git push origin feat/your-feature-name
   # Then create PR via GitHub UI
   ```

4. **PR Review Process**:
   - CI checks must pass (formatting, linting, tests)
   - At least one approval required
   - All conversations must be resolved
   - Use "Squash and merge" to maintain linear history

#### Conventional Commits
This project uses [Conventional Commits](https://www.conventionalcommits.org/) for automated changelog generation:

- `feat:` - New features
- `fix:` - Bug fixes  
- `docs:` - Documentation changes
- `refactor:` - Code refactoring
- `perf:` - Performance improvements
- `test:` - Test additions/improvements
- `ci:` - CI/CD changes
- `chore:` - Maintenance tasks

#### Automated Releases
- **Release Please** automatically creates release PRs based on conventional commits
- **Semantic versioning** is automatically determined from commit types
- **CHANGELOG.md** is automatically updated
- **GitHub releases** are created with binaries for multiple platforms
- **Crates.io publishing** (when configured)

### Testing

```bash
# Run unit tests
cargo test

# Run tests with coverage
cargo test --coverage

# Run integration tests
cargo test --test integration

# Pre-commit checks (run before pushing)
cargo fmt && cargo clippy -- -D warnings && cargo test
```

### Code Quality

The project follows these principles:
- **TDD**: Test-driven development with high code coverage
- **File Size Limits**: Soft limit of 300 lines, hard limit of 500 lines per file
- **Modular Design**: Clear separation of concerns
- **Error Handling**: Comprehensive error types and handling
- **Clippy Compliance**: All clippy warnings treated as errors
- **Consistent Formatting**: Enforced via `cargo fmt`

### Repository Setup

#### Branch Protection Configuration
To set up branch protection rules (requires admin access):

1. Go to **Settings → Branches** in GitHub
2. Add rule for `main` branch with these settings:
   - ✅ Require pull request before merging (1 approval)
   - ✅ Require status checks to pass before merging
   - ✅ Require conversation resolution before merging
   - ✅ Require linear history
   - ✅ Do not allow bypassing the above settings

See [`.github/BRANCH_PROTECTION.md`](.github/BRANCH_PROTECTION.md) for detailed configuration instructions.

#### Required Status Checks
The following CI checks must pass before merging:
- `Check` - Code formatting and linting
- `Test Suite (ubuntu-latest, stable)` - Linux tests
- `Test Suite (windows-latest, stable)` - Windows tests  
- `Test Suite (macos-latest, stable)` - macOS tests
- `Documentation` - Documentation generation

## License

MIT License