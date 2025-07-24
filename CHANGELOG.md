# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.1](https://github.com/mpecan/runcept/compare/runcept-v0.1.0...runcept-v0.1.1) (2025-07-23)


### Features

* add mockall support to remaining traits and migrate health scheduler tests ([f4e1bb3](https://github.com/mpecan/runcept/commit/f4e1bb3c44d97375f68f9451179879c159cf546f))
* complete cli_init_commands.rs migration to centralized utility methods ([0955442](https://github.com/mpecan/runcept/commit/09554428880b7ddfb3ae19d0bf269b05629d5b82))
* complete health monitoring system and MCP activity tracking integration ([cc68ccd](https://github.com/mpecan/runcept/commit/cc68ccd3ec536c4fe37a148b706002a76589c858))
* complete integration test migration to centralized utility methods ([ed117bf](https://github.com/mpecan/runcept/commit/ed117bfe2c3da71d3cea33d8d9114f7474ceb69c))
* complete mockall migration analysis and migrate lifecycle manager tests ([8e2f944](https://github.com/mpecan/runcept/commit/8e2f94461e18b386cd949144a8ea99e0169efe8b))
* complete old pattern cleanup and enable MCP concurrent access test ([75e43a9](https://github.com/mpecan/runcept/commit/75e43a961c5b0050412c8bdf1fdc1c184ae1abc5))
* comprehensive CI optimizations and test fixes ([9656178](https://github.com/mpecan/runcept/commit/96561784678d54dd02d79326217ec75436c6efc7))
* comprehensive code quality improvements and clippy compliance ([7300d59](https://github.com/mpecan/runcept/commit/7300d59d502a5b42a884bea699a996a7cb281766))
* comprehensive process management refactoring with trait-based architecture ([479f65f](https://github.com/mpecan/runcept/commit/479f65f02718df73cc125b38d5fdee9566654f84))
* disable concurrent access test for MCP server ([159a836](https://github.com/mpecan/runcept/commit/159a836b2eb2ff4a28d741461cd902d4d9a1804f))
* disable concurrent access test for MCP server ([2ddc74a](https://github.com/mpecan/runcept/commit/2ddc74a6a23eea4ece64ca03aec19b6866dadb53))
* enhance process management with unique constraints and lifecycle logging ([a25b3fb](https://github.com/mpecan/runcept/commit/a25b3fb07a2e38a018284d403173b76f4f65ac7e))
* fix starting process if it existed before ([a1cc052](https://github.com/mpecan/runcept/commit/a1cc052cfd5f801d4f1ae19a172b91b0ee122738))
* implement comprehensive environment enforcement with validation ([28cd3f3](https://github.com/mpecan/runcept/commit/28cd3f3cb94398664eef860c966e59579209a948))
* implement cross-platform IPC system for Unix and Windows support ([bfc51c4](https://github.com/mpecan/runcept/commit/bfc51c405632d1920ac6f1cccaf114e812320eef))
* implement environment-aware CLI commands and daemon handlers ([994611b](https://github.com/mpecan/runcept/commit/994611b6c75522278084e2349352cc9daa906ed8))
* implement process group killing to terminate child processes ([2b83a5e](https://github.com/mpecan/runcept/commit/2b83a5ee71adbec568c6e8f53ad980e8c0958012))
* implement Release Please for automated releases ([#5](https://github.com/mpecan/runcept/issues/5)) ([c0e4283](https://github.com/mpecan/runcept/commit/c0e42833ab256502e98a04b8ed57def88134e1d4))
* implement repository pattern with database-first architecture ([9e4ebae](https://github.com/mpecan/runcept/commit/9e4ebae2865a879bdfebd47aab798da3e9c39424))
* implement unified Process type with normalized retrieval and environment validation ([547dac6](https://github.com/mpecan/runcept/commit/547dac64479d3694a23ed9da7c3c8f57e5afb28c))
* replace nix dependency with sysinfo for cross-platform process management ([49fcbb6](https://github.com/mpecan/runcept/commit/49fcbb6635907a4d2a3ecf5e0d745502e0219cea))


### Bug Fixes

* apply cargo fmt and clippy fixes to improve code quality ([59e6b8d](https://github.com/mpecan/runcept/commit/59e6b8da12b22a72f4ebb7a88691c462116369c6))
* prevent CLI logging from contaminating command output ([54536a0](https://github.com/mpecan/runcept/commit/54536a01815c632b28c3a3c5f20e48feb75d9661))
* remove unnecessary whitespace in IPC trait definitions ([75fcbfd](https://github.com/mpecan/runcept/commit/75fcbfd1234e9600a23aa9737fbeadcf378dcc23))
* resolve actionlint errors in CI workflow ([697cbb3](https://github.com/mpecan/runcept/commit/697cbb3b9b8eaa2c7754dbb1fa0bc7789d6a68c4))
* resolve all clippy warnings and improve code quality ([5b66029](https://github.com/mpecan/runcept/commit/5b660291a72c80acf1af3aedd715ae6de2e60c16))
* resolve process group management test failures with improved PID extraction ([670cc01](https://github.com/mpecan/runcept/commit/670cc013fdcb98c61198c0f242d164ef14dd35b5))
* update action-gh-release to v2 to resolve runner compatibility ([535e0b8](https://github.com/mpecan/runcept/commit/535e0b803cdbc19d9a7dd916e5d63f736e4731ec))


### Documentation

* fix documentation accuracy and remove broken references ([#8](https://github.com/mpecan/runcept/issues/8)) ([2ae4772](https://github.com/mpecan/runcept/commit/2ae477296560bf6dc2cb8ce7bc09dc79a62c3739))
* update coverage tooling from tarpaulin to cargo-llvm-cov ([a954db2](https://github.com/mpecan/runcept/commit/a954db2cdf34a6c33a77c1f8c38b1517d9597962))


### Code Refactoring

* encapsulate process_id as internal implementation detail in ProcessRepository ([e05585b](https://github.com/mpecan/runcept/commit/e05585b5e81c275d75df98ba97e64dc65cc7f616))
* extract focused services from ProcessExecutionService for better modularity ([7f4ee01](https://github.com/mpecan/runcept/commit/7f4ee01c66224b1ed59bcd62005ec408cf7be947))
* implement ProcessOrchestrationTrait for dependency injection and testing ([65a3b39](https://github.com/mpecan/runcept/commit/65a3b39a3b50d0dbc2da5afb416ec25b5c490ef1))
* migrate all manual mocks to mockall for better maintainability ([237be18](https://github.com/mpecan/runcept/commit/237be1869f906f5f88ca4aa4f8c5e80c548c09b1))
* reduce code duplication and improve testability ([921324c](https://github.com/mpecan/runcept/commit/921324ce7cde9d61df76b23340586a4649a0d7dc))
* remove database activity logs in favor of file-based logging only ([00725b6](https://github.com/mpecan/runcept/commit/00725b673f6fb114d6c63ced2ba843081a872088))
* remove unnecessary blank lines for improved code clarity ([858e805](https://github.com/mpecan/runcept/commit/858e80580acc7dd4af2addde2edb05930a115629))
* remove unused code and simplify architecture following YAGNI principles ([de9067f](https://github.com/mpecan/runcept/commit/de9067f593158ee6851d706bdc60e6fda53b4431))
* replace custom IPC implementation with interprocess library ([27d2a2b](https://github.com/mpecan/runcept/commit/27d2a2bf4a0a9d4b055fefae988b257fef725c10))
* streamline mock health check setup in tests for improved readability ([9a4b0ef](https://github.com/mpecan/runcept/commit/9a4b0ef04b2f93f9536dc20ad2804b5aae53c297))

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
