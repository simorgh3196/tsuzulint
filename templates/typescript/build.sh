#!/bin/bash
# Build script for TypeScript Texide rules
# Requires: Node.js 18+, Javy CLI (https://github.com/bytecodealliance/javy)

set -e

RULE_NAME="${1:-my-rule}"

echo "Building TypeScript rule: $RULE_NAME"

# Install dependencies
if [ ! -d "node_modules" ]; then
  echo "Installing dependencies..."
  npm install
fi

# Compile TypeScript to JavaScript
echo "Compiling TypeScript..."
npx tsc

# Compile JavaScript to WASM using Javy
echo "Compiling to WASM with Javy..."
if ! command -v javy &> /dev/null; then
  echo "Error: Javy is not installed. Install it from https://github.com/bytecodealliance/javy"
  echo "  brew install aspect-cli/javy/javy   # macOS with Homebrew"
  echo "  or download from GitHub releases"
  exit 1
fi

javy build dist/index.js -o "${RULE_NAME}.wasm"

echo "✅ Build complete: ${RULE_NAME}.wasm"
echo ""
echo "To use this rule, add it to your .texide.json:"
echo "  texide add-rule ./${RULE_NAME}.wasm"
