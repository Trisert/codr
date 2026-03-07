#!/bin/bash
# Script to audit technical debt markers
# Finds and reports TODO/FIXME comments in the codebase

set -e

echo "🔍 Checking for technical debt markers..."
echo ""

# Use ripgrep for better pattern matching
if command -v rg &> /dev/null; then
    TODO_COUNT=$(rg "TODO" src/ --type rust --type toml -c || echo "0")
    FIXME_COUNT=$(rg "FIXME" src/ --type rust --type toml -c || echo "0")
    
    echo "TODO comments:"
    echo "============"
    rg "TODO" src/ --type rust --type toml -n || echo "  No TODO comments found"
    
    echo ""
    echo "FIXME comments:"
    echo "=============="
    rg "FIXME" src/ --type rust --type toml -n || echo "  No FIXME comments found"
    
    echo ""
    echo "TODO/FIXME with issue references:"
    echo "=================================="
    TODO_ISSUE=$(rg "TODO\(#\d+\)" src/ --type rust --type toml -c || echo "0")
    FIXME_ASSIGNEE=$(rg "FIXME\([^)]+\)" src/ --type rust --type toml -c || echo "0")
    rg "TODO|FIXME" src/ --type rust --type toml -n | grep -E "#[0-9]+|\([^)]+\)" || echo "  No TODO/FIXME with references found"
    
    echo ""
    echo "📊 Summary:"
    echo "==========="
    echo "  Total TODO comments: $TODO_COUNT"
    echo "  Total FIXME comments: $FIXME_COUNT"
    echo "  TODO with issue references: $TODO_ISSUE"
    echo "  FIXME with assignee: $FIXME_ASSIGNEE"
else
    echo "⚠️  ripgrep not found, using grep"
    echo ""
    TODO_COUNT=$(grep -r "TODO" src/ 2>/dev/null | wc -l || echo "0")
    FIXME_COUNT=$(grep -r "FIXME" src/ 2>/dev/null | wc -l || echo "0")
    
    echo "  Total TODO comments: $TODO_COUNT"
    echo "  Total FIXME comments: $FIXME_COUNT"
fi

echo ""
echo "💡 Tips:"
echo "  - Use TODO(#123) to reference GitHub issues"
echo "  - Use FIXME(@user) to assign specific owners"
echo "  - Keep technical debt items minimal and actionable"
echo "  - Consider creating issues for complex items"
