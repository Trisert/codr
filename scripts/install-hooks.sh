#!/bin/bash
# Script to install pre-commit hooks for the codr project

set -e

echo "📦 Installing pre-commit hooks..."

# Create hooks directory if it doesn't exist
mkdir -p .git/hooks

# Copy the pre-commit hook
if [ -f ".git/hooks/pre-commit" ]; then
    echo "⚠️  Existing pre-commit hook found. Backing up..."
    cp .git/hooks/pre-commit .git/hooks/pre-commit.backup
fi

# Make sure our hook is executable
chmod +x .git/hooks/pre-commit

echo "✅ Pre-commit hooks installed successfully!"
echo ""
echo "The pre-commit hook will:"
echo "  - Check for large files (> 500KB)"
echo "  - Check for long files (> 1000 lines, warning only)"
echo "  - Check code formatting with rustfmt"
echo "  - Run Clippy linter to catch code quality issues"
echo ""
echo "Hooks will run automatically before each commit."
echo "To skip hooks temporarily, use: git commit --no-verify"
