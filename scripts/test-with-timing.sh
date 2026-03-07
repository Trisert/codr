#!/bin/bash
# Script to run tests with performance tracking
# Displays test execution times to help identify slow tests

set -e

echo "🧪 Running tests with performance tracking..."
echo ""

# Check if we want parallel or serial execution
if [ "$1" = "--serial" ]; then
    echo "📊 Running tests serially for accurate timing..."
    echo ""
    cargo test -- --test-threads=1 --nocapture --format=pretty
else
    echo "📊 Running tests in parallel (default)..."
    echo "Use '$0 --serial' for individual test timing"
    echo ""
    cargo test -- --format=pretty
fi

echo ""
echo "✅ Tests complete!"
echo ""
echo "💡 Tip: Test execution times are shown above. Look for tests taking >100ms."
