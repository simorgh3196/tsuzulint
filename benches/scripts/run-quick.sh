#!/bin/bash
#
# Quick benchmark for PR checks (runs subset of tests)
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BENCH_DIR="$(dirname "$SCRIPT_DIR")"

# Use only 100 files for quick test
QUICK_CORPUS="$BENCH_DIR/corpus/quick_test"

mkdir -p "$QUICK_CORPUS"

# Generate 100 small files instead of 1000
echo "Generating quick test corpus (100 files)..."
for i in $(seq -w 1 100); do
    echo "# Document $i" > "$QUICK_CORPUS/doc_$i.md"
    echo "" >> "$QUICK_CORPUS/doc_$i.md"
    echo "TODO: Review this document." >> "$QUICK_CORPUS/doc_$i.md"
    echo "Some content here." >> "$QUICK_CORPUS/doc_$i.md"
done

echo "Running quick benchmark..."
# Run texide on 100 files
time ./target/release/texide lint "$QUICK_CORPUS" --rules benches/rules/texide/no-todo

echo "Quick benchmark complete!"
