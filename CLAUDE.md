# Claude Development Instructions for Runit

## Project Context
This is the "runit" project - a Rust-based process manager with both MCP server and CLI capabilities for managing long-running processes.

## Development Guidelines

### Code Quality Standards
- **File Size Limits**: Soft limit 300 lines, hard limit 500 lines per file
- **Test Coverage**: Maintain >90% test coverage
- **TDD Approach**: Write tests before implementation
- **Modular Design**: Clear separation of concerns between modules

### Testing Commands
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

### Build and Lint Commands
```bash
# Build the project
cargo build

# Build release version
cargo build --release

# Check code without building
cargo check

# Format code
cargo fmt

# Run clippy lints
cargo clippy -- -D warnings

# Run all checks
cargo fmt && cargo clippy -- -D warnings && cargo test
```

### Project Structure Rules
- Keep modules focused and small
- Use `mod.rs` files for module organization
- Separate concerns: process management, MCP server, CLI
- Centralize error handling in `error.rs`
- Configuration in `config.rs`

### Key Implementation Notes
- Use Tokio for async operations
- Implement proper signal handling for process management
- Persist process state to survive runit restarts
- Provide both graceful and forceful process termination
- Capture and manage process logs effectively

### Module Responsibilities
- `process/`: Core process management logic
- `mcp/`: MCP server implementation
- `cli/`: Command-line interface
- `error.rs`: Error types and handling
- `config.rs`: Configuration management

### When Adding Dependencies
Always justify new dependencies and prefer:
- Standard library when possible
- Well-maintained crates with good documentation
- Minimal feature sets to reduce binary size

### Testing Strategy
1. **Unit Tests**: Test individual functions and methods
2. **Integration Tests**: Test module interactions
3. **End-to-End Tests**: Test complete CLI and MCP workflows
4. Make tests deterministic and fast
5. Use mocks for external dependencies

### Error Handling
- Use `thiserror` for structured error types
- Provide helpful error messages for users
- Log errors appropriately for debugging
- Handle edge cases gracefully