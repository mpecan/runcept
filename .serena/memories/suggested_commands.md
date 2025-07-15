# Essential Commands for Runcept Development

## Testing Commands
```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific test module
cargo test process::tests

# Run integration tests
cargo test --test integration

# Check test coverage (requires cargo-tarpaulin)
cargo tarpaulin --out Html
```

## Build and Development Commands
```bash
# Build the project
cargo build

# Build release version
cargo build --release

# Check code without building
cargo check

# Format code (ALWAYS run this)
cargo fmt

# Run clippy lints
cargo clippy -- -D warnings

# Run all checks (recommended before commits)
cargo fmt && cargo clippy -- -D warnings && cargo test
```

## Running the Applications
```bash
# Run CLI interface
cargo run --bin runcept

# Run MCP server
cargo run --bin runcept-mcp

# Run with release optimizations
cargo run --release --bin runcept
cargo run --release --bin runcept-mcp
```

## System Commands (Darwin/macOS)
```bash
# Standard Unix commands work on Darwin
ls          # List directory contents
cd          # Change directory
grep        # Search text patterns
find        # Find files and directories
ps          # List running processes
kill        # Send signals to processes
tail        # Follow log files
git         # Version control
```

## Project-specific CLI Usage
```bash
# Environment management
cargo run --bin runcept activate          # Activate current project environment
cargo run --bin runcept deactivate        # Deactivate current project environment
cargo run --bin runcept status            # Show environment and process status

# Process management
cargo run --bin runcept list              # List all processes in current environment
cargo run --bin runcept start web-server  # Start a specific process
cargo run --bin runcept stop web-server   # Stop a specific process
cargo run --bin runcept restart web-server # Restart a specific process
cargo run --bin runcept logs web-server   # View process logs
```