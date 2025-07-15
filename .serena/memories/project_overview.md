# Runcept Project Overview

## Purpose
Runcept is a Rust-based process manager that runs and intercepts synchronous processes. It provides both MCP (Model Context Protocol) server capabilities and a command-line interface for intelligent process management.

## Key Features
- **Process Interception**: Run and intercept synchronous processes with intelligent management
- **Project-based Configuration**: Define processes per project with `.runcept.toml` files
- **Environment Activation**: Activate/deactivate project environments with all their processes
- **Auto-shutdown**: Automatically stop inactive processes after configurable timeout
- **MCP Server**: Provides MCP tools for AI assistants to manage processes and environments
- **CLI Interface**: Command-line tools for direct process and environment management
- **Persistence**: Process state persisted in SQLite database

## Tech Stack
- **Language**: Rust (Edition 2024)
- **Async Runtime**: Tokio with full features
- **Database**: SQLite with sqlx
- **CLI**: clap for command parsing
- **MCP**: rmcp 0.2.1 for MCP server implementation
- **Serialization**: serde with JSON support
- **Error Handling**: thiserror + anyhow
- **Logging**: tracing with tracing-subscriber
- **Configuration**: TOML format
- **System Calls**: nix for Unix system calls and signals

## Architecture
The project follows a modular design with clear separation of concerns:
- `process/`: Core process management and storage
- `config/`: Configuration management and project activation
- `mcp/`: MCP server implementation with process management tools
- `cli/`: Command-line interface
- `database/`: SQLite database for process persistence
- `daemon/`: Daemon server for RPC communication
- `scheduler/`: Auto-shutdown and monitoring
- `error.rs`: Centralized error handling