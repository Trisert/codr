#!/bin/bash
# Dead code detection script for codr
# Run this locally to check for unused dependencies and dead code

set -e

echo "🔍 Running dead code detection..."
echo ""

# Check if nightly toolchain is installed
if ! rustup toolchain list | grep -q "nightly"; then
    echo "📦 Installing nightly Rust toolchain..."
    rustup toolchain install nightly
fi

# Check if cargo-udeps is installed
if ! cargo +nightly udeps --version &> /dev/null; then
    echo "📦 Installing cargo-udeps..."
    cargo +nightly install cargo-udeps --locked
fi

echo "Checking for unused dependencies..."
echo ""

# Run cargo-udeps to detect unused dependencies
if cargo +nightly udeps --all-targets; then
    echo ""
    echo "✅ No unused dependencies found!"
else
    exit_code=$?
    echo ""
    echo "⚠️  Unused dependencies detected!"
    echo ""
    echo "The following dependencies may be unused:"
    echo "  Review the output above and remove unused dependencies from Cargo.toml"
    echo ""
    echo "To re-run this check:"
    echo "  ./scripts/check-dead-code.sh"
    echo ""
    exit $exit_code
fi

echo ""
echo "💡 Tips for removing unused dependencies:"
echo "  1. Review the output to identify unused dependencies"
echo "  2. Remove them from [dependencies] or [dev-dependencies] in Cargo.toml"
echo "  3. Run 'cargo build' to verify everything still compiles"
echo "  4. Commit the changes"
echo ""
echo "Note: Some dependencies may be indirectly used through re-exports."
echo "Always verify before removing dependencies."
