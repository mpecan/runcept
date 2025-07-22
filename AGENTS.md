# Agent Development Guide for Runcept

## Essential Commands
- **Build**: `cargo build` | **Check**: `cargo check` | **Format**: `cargo fmt`
- **Lint**: `cargo clippy -- -D warnings` | **Test All**: `cargo test`
- **Single Test**: `cargo test test_name` | **Module Tests**: `cargo test module::tests`
- **Integration Tests**: `cargo test --test integration` | **With Output**: `cargo test -- --nocapture`
- **Pre-commit**: `cargo fmt && cargo clippy -- -D warnings && cargo test`

## Code Style (Rust 2024 Edition)
- **Clippy**: All warnings treated as errors - fix immediately, no `#[allow(...)]`
- **Format Strings**: Use `format!("Hello {name}")` not `format!("Hello {}", name)`
- **Methods**: Max 7 params, use structs for more. `new()` must return `Self`
- **Errors**: Use `std::io::Error::other("msg")` not deprecated patterns
- **Imports**: Use module re-exports, avoid accessing private modules directly

## Project Structure
- **Modules**: `process/` (core), `mcp/` (server), `cli/` (interface), `error.rs`, `config/`
- **File Limits**: 300 lines soft, 500 hard | **Test Coverage**: >90%
- **Dependencies**: Tokio async, SQLx database, thiserror errors, clap CLI

## Key Patterns
- TDD approach - write tests first | Use `thiserror` for structured errors
- Async with Tokio | Signal handling for process management | SQLite persistence
- Module separation: process management, MCP server, CLI interface