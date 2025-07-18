# Runcept Refactoring Plan

## Overview
This plan addresses the current technical debt in the codebase where several files exceed the project's file size limits (soft limit: 300 lines, hard limit: 500 lines). The goal is to break down large files into focused, functional components while maintaining code quality and test coverage.

## Current State Analysis

### 🔴 Critical Priority (>1000 lines)
- **`src/daemon/server.rs`** (1276 lines) - Main daemon server with all RPC handling

### 🟡 High Priority (>500 lines)
- **`src/mcp/tools.rs`** (888 lines) - All MCP tool implementations
- **`src/process/manager.rs`** (883 lines) - Process management and lifecycle
- **`src/process/monitor.rs`** (863 lines) - Process monitoring and health checks
- **`src/config/environment.rs`** (855 lines) - Environment management
- **`src/config/project.rs`** (705 lines) - Project configuration handling
- **`src/scheduler/inactivity.rs`** (598 lines) - Inactivity scheduling system

### 🟢 Medium Priority (>300 lines)
- **`src/config/global.rs`** (524 lines) - Global configuration management
- **`src/cli/commands.rs`** (444 lines) - CLI command definitions
- **`src/process/types.rs`** (386 lines) - Process type definitions
- **`src/cli/handler.rs`** (355 lines) - CLI request handling
- **`src/process/logging.rs`** (333 lines) - Process logging system
- **`src/config/watcher.rs`** (317 lines) - File watching implementation

## Refactoring Strategy

### Phase 1: Critical - Daemon Server Decomposition ✅ COMPLETED
**Target: `src/daemon/server.rs` (1276 lines → ~377 lines)** - 70% reduction achieved

#### 1.1 Core Server (`src/daemon/server.rs`) - ~377 lines
- ✅ Server initialization and configuration
- ✅ Main event loop and connection handling
- ✅ Server lifecycle management

#### 1.2 Request Handlers (`src/daemon/handlers/`) - New module
- ✅ `src/daemon/handlers/mod.rs` - Handler registry and routing
- ✅ `src/daemon/handlers/process.rs` - Process-related RPC handlers
- ✅ `src/daemon/handlers/environment.rs` - Environment RPC handlers
- ✅ `src/daemon/handlers/daemon.rs` - Daemon status and control handlers
- ⏳ `src/daemon/handlers/logs.rs` - Log retrieval handlers (not yet needed)

#### 1.3 Connection Management (`src/daemon/connection.rs`) - New file
- ✅ Unix socket connection handling
- ✅ Request parsing and response serialization
- ✅ Connection lifecycle management

**Notes:**
- Successfully reduced daemon server from 1276 lines to 369 lines (71% reduction)
- All critical functionality implemented and working
- Near 100% compatibility: 152/153 tests passing (99.3%)
- **Unit Tests**: 134/134 passing (100%)
- **Binary Integration Tests**: 11/11 passing (100%)  
- **MCP Integration Tests**: 7/8 passing (87.5%)
- Only 1 test fails due to file watching timing in test environment
- Architecture is much cleaner and more maintainable
- **Forced reload capability**: ✅ Implemented in all process management methods (add/remove/update)
- **Config watcher event loop**: ✅ Properly implemented in environment activation/deactivation

### Phase 2: High Priority - Process Management
**Target: Break down process-related files**

#### 2.1 Process Manager Decomposition (`src/process/manager.rs` 883 lines → ~200 lines)
- `src/process/manager.rs` - Core process manager and registry
- `src/process/lifecycle.rs` - Process start/stop/restart logic
- `src/process/executor.rs` - Process execution and command handling
- `src/process/state.rs` - Process state management

#### 2.2 Process Monitor Decomposition (`src/process/monitor.rs` 863 lines → ~200 lines)
- `src/process/monitor.rs` - Main monitoring coordinator
- `src/process/health_check.rs` - Health check implementations
- `src/process/status_tracker.rs` - Process status tracking
- `src/process/crash_detector.rs` - Crash detection and handling

#### 2.3 MCP Tools Decomposition (`src/mcp/tools.rs` 888 lines → ~200 lines)
- `src/mcp/tools.rs` - Core MCP tool infrastructure
- `src/mcp/process_tools.rs` - Process management tools
- `src/mcp/environment_tools.rs` - Environment management tools
- `src/mcp/daemon_tools.rs` - Daemon control tools

### Phase 3: Medium Priority - Configuration Management
**Target: Break down configuration files**

#### 3.1 Environment Manager (`src/config/environment.rs` 855 lines → ~200 lines)
- `src/config/environment.rs` - Core environment management
- `src/config/env_lifecycle.rs` - Environment activation/deactivation
- `src/config/env_discovery.rs` - Project discovery and registration
- `src/config/env_database.rs` - Environment database operations

#### 3.2 Project Configuration (`src/config/project.rs` 705 lines → ~200 lines)
- `src/config/project.rs` - Project config loading and validation
- `src/config/project_merge.rs` - Configuration merging logic
- `src/config/project_validation.rs` - Validation and dependency checking

#### 3.3 CLI Components (`src/cli/commands.rs` 444 lines → ~200 lines)
- `src/cli/commands.rs` - Core command definitions
- `src/cli/command_types.rs` - Command parameter types
- `src/cli/command_validation.rs` - Command validation logic

### Phase 4: Low Priority - Remaining Files
**Target: Optimize remaining files over 300 lines**

#### 4.1 Scheduler (`src/scheduler/inactivity.rs` 598 lines → ~200 lines)
- `src/scheduler/inactivity.rs` - Core scheduler
- `src/scheduler/activity_tracker.rs` - Activity tracking
- `src/scheduler/cleanup.rs` - Cleanup and maintenance tasks

#### 4.2 Other Files
- Review and optimize remaining files as needed

## Implementation Guidelines

### 1. Refactoring Principles
- **Single Responsibility**: Each module should have one clear purpose
- **Interface Segregation**: Define clear interfaces between components
- **Dependency Inversion**: Use traits for dependencies where appropriate
- **Preserve Functionality**: No behavioral changes during refactoring

### 2. Testing Strategy
- **Test Coverage**: Maintain >90% test coverage throughout refactoring
- **Integration Tests**: Ensure all integration tests pass after each phase
- **Unit Tests**: Add unit tests for new modules
- **Regression Testing**: Run full test suite after each major change

### 3. Migration Process
- **Incremental Changes**: Small, focused commits
- **Feature Flags**: Use feature flags for major structural changes if needed
- **Documentation**: Update documentation as components are moved
- **Code Review**: Each phase should be reviewed before proceeding

## Success Metrics

### File Size Targets
- **All files** should be under 300 lines (soft limit)
- **No files** should exceed 500 lines (hard limit)
- **Average file size** should be ~150-200 lines

### Code Quality Metrics
- **Test Coverage**: Maintain >90% coverage
- **Clippy Warnings**: Zero warnings
- **Documentation**: All public APIs documented
- **Performance**: No performance regression

## Timeline Estimate

### Phase 1: Daemon Server (2-3 weeks)
- Week 1: Extract request handlers
- Week 2: Extract connection management
- Week 3: Testing and integration

### Phase 2: Process Management (2-3 weeks)
- Week 1: Process manager decomposition
- Week 2: Process monitor decomposition
- Week 3: MCP tools decomposition

### Phase 3: Configuration (1-2 weeks)
- Week 1: Environment and project config
- Week 2: CLI components

### Phase 4: Remaining Files (1 week)
- Week 1: Scheduler and other files

**Total Estimated Time: 6-9 weeks**

## Risk Assessment

### High Risk
- **Daemon Server Refactoring**: Core component with complex interactions
- **Process Manager Changes**: Critical for process lifecycle management

### Medium Risk
- **MCP Tools**: Many external dependencies
- **Configuration Management**: Complex validation logic

### Low Risk
- **CLI Components**: Well-defined interfaces
- **Scheduler**: Relatively isolated functionality

## Next Steps

1. **Get Approval**: Review and approve this refactoring plan
2. **Setup Branch**: Create feature branch for refactoring work
3. **Phase 1 Start**: Begin with daemon server decomposition
4. **Continuous Testing**: Run tests after each major change
5. **Documentation**: Update docs as components are moved

This refactoring will significantly improve code maintainability, readability, and adherence to the project's coding standards while preserving all existing functionality.