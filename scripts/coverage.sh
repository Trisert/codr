#!/bin/bash
# Test coverage script for codr
# Run this locally to check test coverage before pushing

set -e

echo "🔍 Running test coverage check..."
echo ""

# Check if cargo-tarpaulin is installed
if ! command -v cargo-tarpaulin &> /dev/null; then
    echo "⚠️  cargo-tarpaulin not found. Installing..."
    cargo install cargo-tarpaulin
fi

# Create coverage directory
mkdir -p coverage

# Run coverage with threshold
echo "Running tests with coverage (threshold: 50%)..."
cargo tarpaulin \
    --verbose \
    --out Xml \
    --output-dir coverage \
    --timeout 300 \
    --fail-under 50 \
    --exclude-files '*/tests/*' \
    --exclude-files '*/test_*.rs' \
    -- --test-threads=1

echo ""
echo "✅ Coverage check passed! (≥50%)"
echo "📊 Coverage reports generated in coverage/ directory"
echo ""
echo "To view HTML coverage report:"
echo "  - Open coverage/index.html in your browser (if --out Html is used)"
echo ""
echo "To view XML coverage report:"
echo "  - Use tools like cobertura-coverage or upload to codecov.io"
