#!/bin/bash
# Script to check for large files in the repository
# Can be run manually to audit file sizes

set -e

echo "🔍 Checking for large files in repository..."
echo ""

# Configuration
MAX_FILE_SIZE_KB=500
MAX_LINES=1000

# Find all files in src/ directory
echo "📁 Checking files in src/ directory..."
echo ""

LARGE_FILES_FOUND=0
LONG_FILES_FOUND=0

# Check file sizes
echo "Files larger than ${MAX_FILE_SIZE_KB}KB:"
echo "--------------------------------------"
find src -type f -exec du -k {} \; | awk -v limit="$MAX_FILE_SIZE_KB" '$1 > limit { print $2 " (" $1 "KB)" }' | while read -r line; do
    if [ -n "$line" ]; then
        echo "❌ $line"
        LARGE_FILES_FOUND=1
    fi
done

if [ $LARGE_FILES_FOUND -eq 0 ]; then
    echo "✓ No files larger than ${MAX_FILE_SIZE_KB}KB"
fi

echo ""
echo "Files with more than ${MAX_LINES} lines:"
echo "----------------------------------------"
find src -name "*.rs" -exec wc -l {} \; | awk -v limit="$MAX_LINES" '$1 > limit { print $2 " (" $1 " lines)" }' | while read -r line; do
    if [ -n "$line" ]; then
        echo "⚠️  $line"
        LONG_FILES_FOUND=1
    fi
done

if [ $LONG_FILES_FOUND -eq 0 ]; then
    echo "✓ No files with more than ${MAX_LINES} lines"
fi

echo ""
echo "✅ File size check complete!"
echo ""
echo "Summary:"
find src -type f | wc -l | xargs echo "  Total files checked:"
du -sk src | cut -f1 | xargs echo "  Total size of src/:"
