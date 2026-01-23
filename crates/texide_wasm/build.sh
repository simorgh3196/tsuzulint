#!/bin/bash
# Build script for texide-wasm npm package
#
# Prerequisites:
#   - wasm-pack: cargo install wasm-pack
#   - rustup target add wasm32-unknown-unknown
#
# Usage:
#   ./build.sh        # Build for web (default)
#   ./build.sh web    # Build for web browsers
#   ./build.sh nodejs # Build for Node.js
#   ./build.sh bundler # Build for bundlers (webpack, etc.)

set -e

TARGET=${1:-web}

echo "Building texide-wasm for target: $TARGET"

# Ensure the browser feature is used (not native)
# This is important because workspace builds may enable native feature
export CARGO_FEATURE_BROWSER=1

case $TARGET in
  web)
    wasm-pack build --target web --out-dir pkg --release -- --no-default-features --features browser
    ;;
  nodejs)
    wasm-pack build --target nodejs --out-dir pkg --release -- --no-default-features --features browser
    ;;
  bundler)
    wasm-pack build --target bundler --out-dir pkg --release -- --no-default-features --features browser
    ;;
  *)
    echo "Unknown target: $TARGET"
    echo "Valid targets: web, nodejs, bundler"
    exit 1
    ;;
esac

echo "Build complete! Output in ./pkg/"
echo ""
echo "To publish to npm:"
echo "  cd pkg && npm publish"
