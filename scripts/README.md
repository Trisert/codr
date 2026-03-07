# Scripts

This directory contains utility scripts for codr development.

## Coverage Scripts

### `coverage.sh`

Runs test coverage checks with enforced thresholds (50% minimum).

**Usage:**
```bash
./scripts/coverage.sh
```

## Dead Code Detection Scripts

### `check-dead-code.sh`

Checks for unused dependencies and dead code using cargo-udeps.

**Usage:**
```bash
./scripts/check-dead-code.sh
```

## Duplicate Code Detection Scripts

### `check-duplicate-code.sh`

Checks for duplicate code blocks using cargo-duplicated.

**Usage:**
```bash
./scripts/check-duplicate-code.sh
```

**What it does:**
1. Checks if `cargo-duplicated` is installed, installs it if missing
2. Scans source files for duplicate code blocks (threshold: 50 tokens)
3. Creates `dups.toml` configuration file if it doesn't exist
4. Reports any duplicate code found with helpful refactoring tips

**Why check for duplicate code?**
- **Maintainability**: Duplicate code is harder to maintain (fix once, forget elsewhere)
- **Bug risk**: Bug fixes in one location must be duplicated in others
- **Code quality**: DRY principle (Don't Repeat Yourself) reduces codebase complexity
- **Refactoring opportunities**: Duplicates indicate candidates for extraction

**Configuration:**
Edit `dups.toml` to customize:
- `threshold` - Minimum tokens for a duplicate block (default: 50)
- `excludes` - Paths to exclude from scanning
- `include_tests` - Whether to include test files (default: true)

**CI/CD Integration:**
The check runs automatically in GitHub Actions (`.github/workflows/code-quality.yml`) on every push and pull request.

**Refactoring Tips:**
When duplicate code is found:
1. Extract common patterns into shared functions
2. Use macros or generics to reduce repetition
3. Create utility modules for reusable code
4. Consider trait implementations for shared behavior

## Duplicate Code Detection Scripts

### `check-duplicate-code.sh`

Checks for duplicate code blocks using cargo-duplicated.

**Usage:**
```bash
./scripts/check-duplicate-code.sh
```

## Technical Debt Tracking Scripts

### `check-tech-debt.sh`

Validates that TODO/FIXME comments reference issues or people for traceability.

**Usage:**
```bash
./scripts/check-tech-debt.sh
```

**What it does:**
1. Scans source code for TODO/FIXME comments
2. Validates each TODO/FIXME has an issue reference (e.g., `#123`, `@username`, `repo#45`)
3. Reports violations with file location and line number
4. Provides examples of correct formatting

**Enforced Formats:**
- `TODO(#123)` - References GitHub issue #123
- `FIXME(@alice)` - References person @alice
- `TODO(Trisert/codr#45)` - References repository issue/PR
- `FIXME(username/repo#123)` - References external repository issue

**Why track TODO/FIXME comments?**
- **Accountability**: Links technical debt to specific issues or people
- **Traceability**: Provides full context via GitHub issue/PR links
- **Prioritization**: Issue numbers indicate priority and scheduling
- **Documentation**: GitHub issues provide detailed context and discussion
- **Visibility**: Tech debt is visible alongside regular features

**CI/CD Integration:**
The check runs automatically in GitHub Actions (`.github/workflows/code-quality.yml`) on every push and pull request.

**Best Practices:**
- Create a GitHub issue for significant technical debt before marking it in code
- Update TODO comments when work is in progress or completed
- Use issue numbers for actionable items with GitHub issues
- Be specific in TODO comments about what needs to be done

## Pre-commit Hooks

### `install-hooks.sh`

Installs pre-commit hooks for automated code quality checks.

**Usage:**
```bash
./scripts/install-hooks.sh
```

**What it does:**
- Installs git pre-commit hooks
- Hooks run `cargo fmt` and `cargo clippy` before commits
- Warns about unused dependencies (via cargo-udeps)
- Blocks commits with formatting or linting issues
