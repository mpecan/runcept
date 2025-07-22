# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Initial implementation of Runcept process manager
- MCP server integration for process management
- CLI interface for process control
- SQLite database for process persistence
- Process health monitoring and lifecycle management
- Environment-based process organization
- Comprehensive test suite with >90% coverage

### Features
- **Process Management**: Start, stop, restart, and monitor long-running processes
- **MCP Server**: Model Context Protocol server for AI assistant integration
- **CLI Interface**: Command-line tools for process management
- **Database Persistence**: SQLite-based storage for process state
- **Health Monitoring**: Automatic process health checks and recovery
- **Environment Support**: Organize processes by environment (dev, staging, prod)
- **Logging**: Comprehensive logging and tracing support

### Technical Details
- Built with Rust 2024 edition
- Async/await with Tokio runtime
- SQLx for database operations
- Clap for CLI argument parsing
- Comprehensive error handling with thiserror
- Cross-platform support (Linux, macOS, Windows)

---

*This changelog is automatically maintained by [Release Please](https://github.com/googleapis/release-please).*