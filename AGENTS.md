# AGENTS.md

This file provides guidance for autonomous AI agents working on the codr repository.

## Quick Start

```bash
# Build the project
cargo build

# Run in TUI mode (interactive chat) - default
cargo run --

# Run in direct mode (single task, non-interactive)
cargo run -- -d "your task here"

# Run with YOLO mode (auto-approve all actions)
cargo run -- --yolo

# Run tests
cargo test
```

## Development Workflow

### Building

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# Run directly without building
cargo run
```

### Code Quality

**CRITICAL**: Always run these commands before committing changes:

```bash
# Format code
cargo fmt

# Check formatting (fails if not formatted)
cargo fmt -- --check

# Run linter
cargo clippy

# Run linter with strict checks
cargo clippy -- -W clippy::all -W clippy::pedantic

# Auto-fix some Clippy warnings
cargo clippy --fix --allow-dirty

# Check for unused code (Rust analyzer)
cargo check --all-targets
```

### Dead Code Detection

The project uses multiple approaches to detect and prevent dead code:

**Compiler Warnings:**
- Rust compiler warns about unused variables and dead code by default
- Enable `unused_warnings` as `warn` level to catch unused code
- Use `#[allow(dead_code)]` sparingly and document reasons

**Automated Detection (CI & Pre-commit):**
```bash
# Check for unused dependencies locally
./scripts/check-dead-code.sh

# Or run directly with cargo-udeps
cargo +nightly udeps --all-targets

# CI/CD pipeline automatically checks for unused dependencies
# See .github/workflows/code-quality.yml
```

**Best Practices:**
- Remove unused code promptly (don't comment out, delete it)
- Use compiler warnings as the first line of defense
- Run `cargo check` before committing to catch dead code early
- Document intentionally unused code with `#[allow(dead_code)]` and reason
- Pre-commit hooks warn about unused dependencies (blocking commit is optional)

### Duplicate Code Detection

The project uses automated tools to detect duplicate code and enforce DRY principles:

**Automated Detection (CI & Local):**
```bash
# Check for duplicate code locally
./scripts/check-duplicate-code.sh

# Or run directly with cargo-duplicated
cargo duplicated

# CI/CD pipeline automatically checks for duplicate code
# See .github/workflows/code-quality.yml
```

**Configuration:**
- **Threshold**: 50 tokens minimum for duplicate detection
- **Config file**: `dups.toml` (auto-created on first run)
- **Test files**: Included in duplicate detection
- **Excludes**: `target/`, `*.lock` files

**Best Practices:**
- Extract common patterns into shared functions
- Use macros or generics to reduce repetition
- Create utility modules for reusable code
- Consider trait implementations for shared behavior
- Review duplication reports and refactor when found

### Technical Debt Tracking

The project uses automated tech debt tracking to ensure TODO/FIXME comments are actionable and traceable:

**Enforced Standards:**
- **Issue Linking Required**: All TODO/FIXME comments must reference an issue or person
- **Supported Formats**:
  - `TODO(#123)` - References GitHub issue #123
  - `FIXME(@alice)` - References responsible person @alice
  - `TODO(Trisert/codr#45)` - References PR or issue in repository
  - `FIXME(username/repo#123)` - References external repository issue

**Automated Validation:**
```bash
# Check tech debt tracking locally
./scripts/check-tech-debt.sh

# CI/CD pipeline automatically validates TODO/FIXME comments
# See .github/workflows/code-quality.yml
```

**Best Practices:**
- Create a GitHub issue for significant technical debt before marking it in code
- Use `TODO(#issue-number)` for actionable items with GitHub issues
- Use `FIXME(@username)` for items needing specific person's attention
- Avoid vague TODOs without context - explain what needs to be done
- Update TODO comments when work is in progress or completed

**Examples:**
```rust
// ❌ BAD - Vague TODO without tracking
// TODO: Fix this later

// ✅ GOOD - Tracked TODO with issue reference
// TODO(#45): Implement streaming for large file responses

// ✅ GOOD - Tracked FIXME with person responsible
// FIXME(@alice): Handle edge case where config file is malformed

// ✅ GOOD - Tracked TODO with PR reference
// TODO(Trisert/codr#123): Refactor module for better testability
```

**Why Track Technical Debt?**
- **Accountability**: Clear ownership and responsibility for each item
- **Traceability**: Links directly to issues/PRs for full context
- **Prioritization**: Issue numbers indicate priority and scheduling
- **Documentation**: GitHub issues provide detailed context and discussion
- **Visibility**: Tech debt is visible and can be tracked alongside features

### Pre-commit Hooks

The project has pre-commit hooks that automatically run before each commit:
- **Large file detection**: Blocks files larger than 500KB
- **Line count warnings**: Warns about files exceeding 1000 lines
- Checks code formatting with rustfmt
- Runs Clippy linter
- Prevents commits with formatting or linting issues

Install hooks (run once per clone):
```bash
./scripts/install-hooks.sh
```

Skip hooks if needed (not recommended):
```bash
git commit --no-verify
```

**File size limits:**
- Maximum file size: 500KB (enforced)
- Recommended maximum lines: 1000 (warning only)
- These limits help maintain code quality and prevent monolithic files

### Testing

**Note**: The project currently has unit tests in `src/tests/` and integration tests in `tests/`. When adding tests:

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific test
cargo test test_name

# Run tests in a module
cargo test --lib module_name

# Run tests with performance timing
cargo test -- --nocapture -- --test-threads=1

# Run tests and show execution time for each test
cargo test -- --nocapture
```

**Test Coverage:**

The project uses Rust's built-in testing framework with enforced coverage thresholds.

**Coverage Enforcement:**

- **CI/CD Pipeline**: GitHub Actions workflow (`.github/workflows/test-coverage.yml`) enforces minimum 50% code coverage
- **Local Coverage Check**: Run `./scripts/coverage.sh` to verify coverage before pushing
- **Coverage Threshold**: Minimum 50% coverage required (configurable in workflow and script)

```bash
# Install tarpaulin for coverage tracking
cargo install cargo-tarpaulin

# Run coverage check locally (enforces 50% threshold)
./scripts/coverage.sh

# Generate coverage report without threshold
cargo tarpaulin --out Xml

# Generate HTML coverage report
cargo tarpaulin --output-dir coverage

# Run coverage with custom threshold
cargo tarpaulin --fail-under 60

# Run coverage with line-by-line output
cargo tarpaulin --verbose

# Run coverage with line-by-line output
cargo tarpaulin --verbose

# Enforce minimum coverage threshold (e.g., 50%)
cargo tarpaulin --ignore-panics --ignore-tests --fail-under 50
```

**Coverage tooling:**
- **cargo-tarpaulin**: Rust code coverage tool (alternative to grcov)
- **Integration**: Can be integrated into CI/CD pipelines
- **Output formats**: HTML, XML, JSON for CI integration
- **Thresholds**: Use `--fail-under` to enforce minimum coverage percentages

**Current coverage goals:**
- Focus on testing critical paths (config loading, tool registry, parsing)
- Aim for >70% coverage on core modules before enforcement
- Use coverage reports to identify untested code paths
- Prioritize coverage on business logic over edge cases

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific test
cargo test test_name

# Run tests in a module
cargo test --lib module_name

# Run tests with performance timing
cargo test -- --nocapture -- --test-threads=1

# Run tests and show execution time for each test
cargo test -- --nocapture
```

**Test Performance Tracking:**

Rust's test harness automatically tracks and displays test execution time:
- **Slowest tests shown**: After test run completes, the test harness displays execution time for each test
- **Parallel execution**: Tests run in parallel by default (faster overall, harder to measure individual test time)
- **Serial execution**: Use `-- --test-threads=1` to run tests one at a time for accurate timing
- **Format**: Test timing is shown in the format: `test name ... ok (duration in ms)`

**Viewing test performance:**
```bash
# Run tests serially to see individual test times
cargo test -- --test-threads=1

# Run tests with verbose output
cargo test -- --nocapture

# Run specific test module and measure performance
cargo test --lib config_tests -- --test-threads=1

# Format output for better readability
cargo test -- --format=pretty
```

**Test Naming Conventions:**

The project follows Rust testing conventions:
- **Unit test files**: Located in `src/tests/` directory (e.g., `src/tests/config_tests.rs`)
- **Integration test files**: Located in `tests/` directory (e.g., `tests/integration_test.rs`)
- **Test modules**: Each file is a module with a `_tests.rs` suffix (unit tests) or `*_test.rs` (integration tests)
- **Test functions**: Use `test_` prefix with descriptive names (e.g., `fn test_config_default_values()`)
- **Test organization**: Group related tests in module files by functionality
  - `config_tests.rs` - Configuration loading and management
  - `error_tests.rs` - Error type handling
  - `parser_tests.rs` - XML and JSON action parsing
  - `tools_tests.rs` - Tool registry and role-based access
  - `integration_test.rs` - End-to-end workflows and component interactions

**Example test structure:**
```rust
//! Tests for configuration loading and management

use crate::config::Config;

#[test]
fn test_config_default_values() {
    let config = Config::default();
    // Test implementation
}
```

**Best practices:**
- Use descriptive test names that explain what is being tested
- Follow the pattern `test_<component>_<scenario>` (e.g., `test_config_default_values`)
- Group related tests in the same module file
- Keep tests focused and independent
- Use `#[test]` attribute for test functions
- **Test isolation**: Each test should be independent and not rely on shared state
  - Tests run in parallel by default (using multiple threads)
  - Use `tempfile` crate for temporary file/directory isolation
  - Avoid shared static mutable state across tests
  - Each test should create its own test data and clean up afterwards

**Running tests in isolation:**
```bash
# Run tests in parallel (default, fastest)
cargo test

# Run tests serially (one at a time)
cargo test -- --test-threads=1

# Run specific test module in isolation
cargo test --lib config_tests

# Run single test
cargo test test_config_default_values
```

**Test isolation in Rust:**
- Rust's test harness runs tests in parallel by default
- Each test runs in its own thread
- No shared state between tests by default
- Use `tempfile` crate for file system isolation (already in dependencies)
- Tests should not depend on execution order

## Project Architecture

codr is a terminal-based AI agent that supports multiple LLM providers (OpenAI-compatible/llama.cpp, Anthropic Claude) using a tool-calling approach.

### Core Components

```
src/
├── main.rs          # Entry point, CLI parsing, direct mode execution
├── model.rs         # Multi-provider LLM abstraction with streaming support
├── parser.rs        # Parses codr_tool/codr_bash XML blocks and JSON tool calls
├── config.rs        # TOML configuration loader with XDG directory support
├── error.rs         # Custom error types (Timeout, Terminating)
├── agent/           # Unified agent loop implementation
│   ├── mod.rs       # Exports LoopConfig, LoopResult, ActionExecutor, etc.
│   ├── executor.rs  # ActionExecutor trait with DirectExecutor and TUIExecutor
│   ├── loop_.rs     # Core agent loop with unified streaming/non-streaming support
│   ├── tui_executor.rs # TUI-specific executor with approval workflow
│   └── updates.rs   # TuiUpdate types for agent→UI communication
├── tui/             # Modular TUI implementation
│   ├── mod.rs       # Main TUI entry point and agent loop integration
│   ├── theme.rs     # Theme configuration (colors, styles)
│   ├── markdown.rs  # Markdown renderer using pulldown-cmark
│   ├── events.rs    # Event handling (keyboard, mouse)
│   └── widgets/     # Modular widget architecture
│       ├── mod.rs   # Widget exports
│       ├── conversation.rs # Message display with markdown rendering
│       ├── input.rs # User input widget
│       ├── banner.rs # Status bar (model, role, tokens, cost)
│       └── status.rs # Toast notifications and progress
└── tools/
    ├── mod.rs       # Tool trait, registry, categories, role filtering
    ├── impl.rs      # Tool implementations (read, bash, edit, write, grep, find)
    ├── schema.rs    # JSON schema types for tool parameters
    ├── context.rs   # Tool execution context (cwd, env, limits)
    └── async_wrapper.rs # Async tool wrappers (experimental)
```

### Tool System

Tools implement the `Tool` trait and are registered in `ToolRegistry`:

```rust
trait Tool {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> &ToolSchema;
    fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError>;
}
```

**Available tools**: `read`, `bash`, `edit`, `write`, `grep`, `find`, `file_info`

**Tool Categories** (for role filtering):
- `FileOps` - read, write, edit, file_info
- `Search` - grep, find
- `System` - bash

### Role System

Three operation modes (cycled with `Shift+Tab`):
- **YOLO** (Red) - Full access, all tools auto-approved
- **SAFE** (Green, default) - All tools available, write/edit/bash require approval
- **PLAN** (Blue) - Read-only: read, bash, grep, find, file_info only

## Configuration

Config loading priority:
1. `./codr.toml` (current directory)
2. `~/.config/codr/config.toml` (XDG config home)
3. Default (OpenAI on localhost:8080)

**Security**: Never commit API keys or secrets to the repository! Use:
- `codr.toml.example` - Template configuration file (committed)
- `codr.toml` - Your local configuration (gitignored, contains secrets)
- Environment variables - `ANTHROPIC_API_KEY`, `NVIDIA_API_KEY`, etc.

**Example setup**:
```bash
# Copy the example template
cp codr.toml.example codr.toml

# Edit codr.toml and add your API key
# Use environment variables instead (recommended)
export NVIDIA_API_KEY="your-key-here"
export ANTHROPIC_API_KEY="your-key-here"
```

**Example `codr.toml`**:
```toml
model = "openai"  # or "anthropic"

[openai]
base_url = "http://localhost:8080"
model = "default"
# api_key = "your-api-key-here"  # Use env var instead

[anthropic]
# api_key = "your-api-key-here"  # Use ANTHROPIC_API_KEY env var
```

## Code Conventions

### Formatting
- Max line width: 100 characters
- 4 spaces for indentation (no hard tabs)
- Unix line endings (LF)
- Use `cargo fmt` to format code

### Linting
- Cognitive complexity threshold: 20
- Max function arguments: 7
- Type complexity: 250
- Use `cargo clippy` to check code quality

### Code Conventions

**TODO/FIXME Comments:**

When adding TODO or FIXME comments, follow this format for technical debt tracking:

```rust
// TODO(#123) - Brief description of what needs to be done
// FIXME(user/team) - Brief description of what needs fixing
// TODO: Brief description (for minor items not worth an issue)
```

**Examples:**
```rust
// TODO(#45) - Implement streaming for large file responses
// FIXME(@alice) - Handle edge case where config file is malformed
// TODO: Add validation for user input
```

**Technical Debt Tracking:**

The project uses GitHub issues for tracking technical debt:
- TODO/FIXME comments should reference issue numbers when possible
- Use `TODO(issue-number)` for items tracked in GitHub
- Use `FIXME(assignee)` for items needing specific attention
- Unclassified TODOs are allowed for minor items

**Finding Technical Debt:**
```bash
# Search for TODO comments
grep -r "TODO" src/

# Search for FIXME comments
grep -r "FIXME" src/

# Search for TODO/FIXME with issue references
grep -rE "TODO|FIXME" src/ | grep -E "#[0-9]+"
```

### Naming
- Follow Rust naming conventions (snake_case for functions/variables, PascalCase for types)
- Use descriptive names that explain purpose
- Avoid abbreviations unless widely known
- **Enforced by clippy.toml**: Non-descriptive names are disallowed and will trigger linter errors
  - Generic non-descriptive names: `foo`, `bar`, `baz`, `data`, `info`, `result`, `value`, `item`, `object`
  - Vague generic names: `temp`, `stuff`, `things`, `helper`, `util`, `manager`, `processor`, `handler`
  - Single-letter names (except common patterns like `i`, `j`, `k` for loops, `x`, `y`, `z` for coordinates, `q` for queue)
  - **Run `cargo clippy` to check naming compliance** - violations will block commits via pre-commit hooks

**Specific conventions:**
- **Functions & variables**: `snake_case` (e.g., `get_tool`, `user_name`, `parse_json`)
- **Types & structs**: `PascalCase` (e.g., `ToolRegistry`, `Message`, `ModelType`)
- **Constants**: `SCREAMING_SNAKE_CASE` (e.g., `MAX_RETRIES`, `DEFAULT_TIMEOUT`)
- **Acronyms in names**: Preserve capitalization (e.g., `parse_json`, `to_html`, `run_tui`, not `parseJSON`, `toHTML`, `runTUI`)
- **Boolean variables**: Use `is_`, `has_`, `can_` prefixes (e.g., `is_ready`, `has_tools`, `can_execute`)
- **Iterator variables**: Use descriptive names (e.g., `tool` instead of `t`, `registry` instead of `r`)
- **Conversion functions**: Use `to_` and `from_` prefixes (e.g., `to_model_type`, `from_str`)

**Examples of GOOD names:**
```rust
// Clear and descriptive
fn get_tool_by_name(name: &str) -> Option<&Tool>
let user_name = "alice";
let is_authenticated = true;
const MAX_RETRIES: usize = 3;

// Iterator with descriptive name
for tool in registry.get_tools() {
    println!("{}", tool.name());
}
```

**Examples of BAD names (blocked by clippy):**
```rust
// ❌ Non-descriptive - blocked by clippy.toml
fn process_data(data: &Data) -> Result
let temp = calculate();
let helper = Helper::new();

// ✅ Better - use descriptive names
fn process_config(config: &Config) -> Result<ConfigError>
let result = calculate_checksum();
let parser = ConfigParser::new();
```

### Error Handling
- Use `Result<T, E>` for fallible operations
- Use custom error types from `src/error.rs`
- Prefer proper error propagation over `unwrap()` in production code
- `unwrap()` and `expect()` are acceptable in tests

### Module Organization
- Keep modules focused and cohesive
- Use `mod.rs` to re-export public API
- Separate implementation into sub-modules when appropriate

## Important Implementation Notes

### Adding New Tools
1. Implement `Tool` trait in `src/tools/impl.rs`
2. Implement `category()` returning `ToolCategory` (FileOps/Search/System)
3. Register in `create_coding_tools()` in `src/tools/mod.rs`
4. Update `Role::tool_available()` if the tool should be restricted

### Edit Tool Modes
The `edit` tool supports two modes:
1. **String replacement**: `{"file_path": "src/main.rs", "old_text": "old", "new_text": "new"}`
2. **Line-based editing**: `{"file_path": "src/main.rs", "line_start": 10, "line_end": 20, "new_content": "new"}`

Both require reading the file first to verify contents.

### Parser
The parser (`parse_action()` in `src/parser.rs`) handles multiple formats:
- **Native tool calling** - JSON format from OpenAI/Anthropic APIs
- **XML Tool actions**: `<codr_tool name="read">{"file_path": "src/main.rs"}</codr_tool>`
- **XML Bash actions**: `<codr_bash>ls -la</codr_bash>`
- **Plain responses**: Text without tool calls (conversation end)

Also provides `clean_message_content()` for removing XML tags from content for display.

## Performance Considerations

- Use streaming responses for long outputs
- Implement pagination for file operations on large files
- Cache tool registry lookups where appropriate
- Use async operations for I/O-bound tasks

## Security Considerations

- Role-based access control (PLAN/SAFE/YOLO) prevents accidental damage
- Approval workflow in SAFE mode for dangerous operations
- Tool validation prevents malformed parameters from executing
- Environment variables should be used for sensitive data (API keys)

## Common Tasks

### Adding a new dependency
1. Add to `Cargo.toml` under `[dependencies]`
2. Run `cargo build` to fetch and compile
3. Update this file if the dependency affects development workflow

### Debugging the TUI
- Use `eprintln!()` for debug output (won't appear in TUI)
- Check `src/tui/events.rs` for keybinding handling
- Widget rendering is in `src/tui/widgets/`

### Testing model integration
- Use direct mode: `cargo run -- -d "test prompt"`
- Check `src/model.rs` for provider-specific behavior
- Streaming vs non-streaming is controlled by `LoopConfig`

## Getting Help

- Run `codr --help` for CLI options
- Check CLAUDE.md for detailed architecture documentation
- Review README.md for project overview and usage
- Check examples in `codr.toml` for configuration options

## Repository Standards

This repository follows these quality standards:
- **Linter**: Clippy with custom configuration (clippy.toml)
- **Formatter**: rustfmt with custom configuration (rustfmt.toml)
- **Pre-commit hooks**: Automated formatting, linting, and file size checks
- **Tests**: Unit tests in `src/tests/` following Rust conventions

When contributing:
1. Run `cargo fmt` before committing
2. Run `cargo clippy` and address warnings
3. Run `cargo test` to verify tests pass
4. Test your changes manually in both TUI and direct modes
5. Update documentation if changing behavior
6. Follow test naming conventions when adding new tests (see Testing section above)
