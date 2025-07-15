# Code Style and Conventions for Runcept

## File Organization
- **File Size Limits**: Soft limit 300 lines, hard limit 500 lines per file
- **Modular Design**: Clear separation of concerns between modules
- Use `mod.rs` files for module organization
- Keep modules focused and small

## Code Quality Standards
- **Test Coverage**: Maintain >90% test coverage
- **TDD Approach**: Write tests before implementation
- **ALWAYS run `cargo fmt` before committing code**
- Code formatting is enforced with rustfmt to maintain consistency

## Error Handling
- Use `thiserror` for structured error types
- Use `anyhow` for error context where appropriate
- Provide helpful error messages for users
- Log errors appropriately for debugging
- Handle edge cases gracefully
- Centralize error handling in `error.rs`

## Async Design Patterns
- Use Tokio for async operations
- Non-blocking I/O for process communication
- Concurrent process management
- Implement proper signal handling for process management
- Signal handling for graceful shutdown

## Module Responsibilities
- `process/`: Core process management logic
- `config/`: Configuration management and project activation
- `mcp/`: MCP server implementation
- `cli/`: Command-line interface
- `database/`: SQLite database for process persistence
- `daemon/`: Daemon server with RPC communication
- `scheduler/`: Auto-shutdown and monitoring
- `error.rs`: Error types and handling

## Testing Strategy
1. **Unit Tests**: Test individual functions and methods
2. **Integration Tests**: Test module interactions
3. **End-to-End Tests**: Test complete CLI and MCP workflows
4. Make tests deterministic and fast
5. Use mocks for external dependencies

## Dependency Management
- Always justify new dependencies
- Prefer standard library when possible
- Use well-maintained crates with good documentation
- Prefer minimal feature sets to reduce binary size

## Configuration Standards
- Use TOML format for configuration files
- Project configuration in `.runcept.toml` files
- Global configuration in `~/.runcept/config.toml`
- Centralize configuration logic in `config.rs`