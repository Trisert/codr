#!/bin/bash
# Duplicate code detection script for codr
# Run this locally to check for duplicate code blocks

set -e

echo "🔍 Running duplicate code detection..."
echo ""

# Check if cargo-duplicated is installed
if ! cargo duplicated --version &> /dev/null; then
    echo "📦 Installing cargo-duplicated..."
    cargo install cargo-duplicated --locked
fi

# Create configuration file if it doesn't exist
if [ ! -f "dups.toml" ]; then
    echo "📝 Creating dups.toml configuration..."
    cat > dups.toml << 'EOF'
# Duplicate code detection configuration for codr
# See https://github.com/bircni/cargo-duplicated for details

# Minimum number of tokens in a duplicate block
threshold = 50

# Paths to exclude from duplicate detection
excludes = [
    "target/",
    "*.lock",
]

# Include test files in duplicate detection
include_tests = true

# JSON output for CI integration
output_format = "json"
EOF
fi

echo "Scanning for duplicate code blocks (threshold: 50 tokens)..."
echo ""

# Run cargo-duplicated
if cargo duplicated; then
    echo ""
    echo "✅ No duplicate code found!"
else
    exit_code=$?
    echo ""
    echo "⚠️  Duplicate code detected!"
    echo ""
    echo "Duplicate code makes maintenance harder and indicates opportunities for refactoring."
    echo "Consider extracting common code into shared functions or modules."
    echo ""
    echo "To re-run this check:"
    echo "  ./scripts/check-duplicate-code.sh"
    echo ""
    echo "To customize the threshold, edit dups.toml"
    exit $exit_code
fi

echo ""
echo "💡 Tips for reducing code duplication:"
echo "  1. Extract common patterns into shared functions"
echo "  2. Use macros or generics to reduce repetition"
echo "  3. Create utility modules for reusable code"
echo "  4. Consider trait implementations for shared behavior"
