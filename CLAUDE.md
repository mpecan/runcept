# Claude Development Instructions for Runcept

## Project Context
This is the "runcept" project - a Rust-based process manager with both MCP server and CLI capabilities for managing long-running processes.

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

# Check test coverage (requires cargo-llvm-cov)
cargo llvm-cov --html

# Run coverage for all tests including integration tests
cargo llvm-cov --html --all-targets

# Generate coverage report in different formats
cargo llvm-cov --lcov --output-path coverage.lcov
cargo llvm-cov --json --output-path coverage.json
```

### Build and Lint Commands
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

### Development Workflow
1. **Before starting any code changes**:
   ```bash
   cargo fmt && cargo clippy -- -D warnings
   ```
2. **During development** (run frequently):
   ```bash
   cargo check  # Fast compilation check
   cargo clippy -- -D warnings  # Check for issues early
   ```
3. **Before committing**:
   ```bash
   cargo fmt && cargo clippy -- -D warnings && cargo test
   ```
4. **If clippy warnings appear**:
   - Fix them immediately, don't let them accumulate
   - Use the specific examples in this guide
   - Prefer fixing over allowing warnings

### Code Formatting
- **ALWAYS run `cargo fmt` before committing code**
- Code formatting is enforced with rustfmt to maintain consistency
- All code should follow the project's formatting standards
- Use `cargo fmt` during development to catch formatting issues early

### Rust Code Quality Guidelines

#### Clippy Compliance
- **MANDATORY**: All code must pass `cargo clippy -- -D warnings` (warnings treated as errors)
- Fix clippy warnings immediately, do not use `#[allow(...)]` unless absolutely necessary
- Common issues to avoid:
  - **Format String Warnings**: Use direct variable interpolation: `format!("Hello {name}")` instead of `format!("Hello {}", name)`
  - **Too Many Arguments**: Methods with >7 parameters should use struct parameters instead
  - **Method Naming**: Methods named `new()` must return `Self`, use descriptive names like `create_channels()` for other constructors
  - **Needless Borrows**: Remove unnecessary `&` when the compiler suggests it

#### Format String Best Practices
- **Always use direct variable interpolation in format strings**:
  ```rust
  // ✅ GOOD
  format!("Process '{name}' failed with error: {error}")
  
  // ❌ BAD - triggers clippy::uninlined_format_args
  format!("Process '{}' failed with error: {}", name, error)
  ```
- **Apply this to all formatting macros**: `format!()`, `println!()`, `error!()`, `info!()`, `debug!()`, etc.

#### Method Design Guidelines
- **Limit method parameters to 7 or fewer**
- **For methods with many parameters**, use struct parameters:
  ```rust
  // ✅ GOOD
  pub async fn insert_process(&self, process: &Process) -> Result<()>
  
  // ❌ BAD - too many arguments
  pub async fn insert_process(&self, id: &str, name: &str, command: &str, 
                              working_dir: &str, env_id: &str, status: &str, 
                              pid: Option<i64>) -> Result<()>
  ```

#### Constructor Naming Conventions
- **Methods named `new()`** must return `Self`
- **For other constructors**, use descriptive names:
  ```rust
  // ✅ GOOD
  impl MyStruct {
      pub fn new() -> Self { ... }                    // Returns Self
      pub fn create_channels() -> Channels { ... }    // Returns different type
      pub fn with_config(config: Config) -> Self { ... } // Returns Self with parameters
  }
  ```

#### Error Handling Patterns
- **Use modern error creation patterns**:
  ```rust
  // ✅ GOOD
  std::io::Error::other("Custom error message")
  
  // ❌ BAD - deprecated pattern
  std::io::Error::new(std::io::ErrorKind::Other, "Custom error message")
  ```

#### Import Organization
- **Use module re-exports properly**:
  ```rust
  // ✅ GOOD - use re-exported types
  use crate::process::Process;
  
  // ❌ BAD - accessing private modules
  use crate::process::types::Process;
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

### Progress Tracking
- **IMPORTANT**: Always update PLAN.md to track implementation progress
- Mark completed sub-tasks with ✅ in the PLAN.md file
- Update phase completion status as you progress
- Use clear markers like "✅ COMPLETED" or "🚧 IN PROGRESS" or "⏳ PENDING"
- When starting a new phase, mark it as "🚧 IN PROGRESS"
- When completing tests, mark them as "✅ Tests completed"
- When completing implementation, mark as "✅ Implementation completed"

### Progress Tracking Example
```markdown
### Phase 1: Core Foundation
1. **Error Handling System** (`error.rs`)
   - ✅ Write tests for error types and conversions
   - ✅ Implement custom error types for different failure modes
   - ✅ Implement error conversion traits and user-friendly messages

2. **Database Layer** (`database/`)
   - 🚧 Write tests for database operations and migrations
   - ⏳ Implement SQLite database setup with migrations
   - ⏳ Implement schema for processes, environments, and activity tracking
   - ⏳ Implement database operations and connection management
```

This helps track what's been completed and what still needs work across development sessions.