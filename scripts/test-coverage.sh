#!/bin/bash
# Script to run tests with coverage tracking
# Generates coverage reports and enforces minimum coverage thresholds

set -e

echo "📊 Running tests with coverage tracking..."
echo ""

# Check if tarpaulin is installed
if ! command -v cargo-tarpaulin &> /dev/null; then
    echo "⚠️  cargo-tarpaulin not found!"
    echo "Install it with: cargo install cargo-tarpaulin"
    echo "Running tests without coverage..."
    cargo test
    exit 0
fi

# Minimum coverage threshold (50%)
COVERAGE_THRESHOLD=50

echo "Generating coverage report..."
echo "Minimum coverage threshold: ${COVERAGE_THRESHOLD}%"
echo ""

# Run tarpaulin with coverage enforcement
if cargo tarpaulin --ignore-panics --ignore-tests --fail-under $COVERAGE_THRESHOLD --verbose; then
    echo ""
    echo "✅ Coverage check passed! (${COVERAGE_THRESHOLD}% minimum met)"
else
    EXIT_CODE=$?
    echo ""
    echo "❌ Coverage check failed! Below ${COVERAGE_THRESHOLD}% threshold"
    echo "To view detailed coverage report:"
    echo "  cargo tarpaulin --out Html"
    echo ""
    exit $EXIT_CODE
fi
