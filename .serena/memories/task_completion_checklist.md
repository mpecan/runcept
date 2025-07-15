# Task Completion Checklist

## Before Committing Code
Always run this sequence before any commit:

```bash
# 1. Format code (MANDATORY)
cargo fmt

# 2. Run linting checks
cargo clippy -- -D warnings

# 3. Run all tests
cargo test

# 4. Optional: Run integration tests
cargo test --test integration

# 5. Check that build succeeds
cargo build
```

## Progress Tracking Requirements
- **IMPORTANT**: Always update PLAN.md to track implementation progress
- Mark completed sub-tasks with ✅ in the PLAN.md file
- Update phase completion status as you progress
- Use clear markers like "✅ COMPLETED" or "🚧 IN PROGRESS" or "⏳ PENDING"
- When starting a new phase, mark it as "🚧 IN PROGRESS"
- When completing tests, mark them as "✅ Tests completed"
- When completing implementation, mark as "✅ Implementation completed"

## Testing Requirements
- Maintain >90% test coverage
- Write tests before implementation (TDD)
- Ensure all tests pass before marking tasks complete
- Run tests with output for debugging: `cargo test -- --nocapture`

## Code Quality Checks
- No clippy warnings allowed
- All code must be formatted with rustfmt
- Follow the file size limits (300 soft, 500 hard)
- Maintain clear module separation
- Proper error handling throughout

## Documentation
- Update README.md if new features are added
- Update PLAN.md progress markers
- Add inline documentation for complex logic
- Maintain clear commit messages