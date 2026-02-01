.PHONY: all build release wasm build-all release-all test test-verbose lint fmt fmt-check clean

# =============================================================================
# Development (frequently used)
# =============================================================================

# Default: format, lint, and test
all: fmt lint test

# Build host (debug)
build:
	cargo build --workspace

# Run all tests (host + PDK)
TARGET ?=
test:
	cargo test --workspace
	cd rules && cargo test $(if $(TARGET),--target $(TARGET),) -p texide-rule-pdk

# Run tests with output
test-verbose:
	cargo test --workspace -- --nocapture

# Run clippy
lint:
	cargo clippy --workspace --all-targets -- -D warnings

# Format code
fmt:
	cargo fmt --all

# Format check (for CI)
fmt-check:
	cargo fmt --all -- --check

# =============================================================================
# Release
# =============================================================================

# Build host (release)
release:
	cargo build --workspace --release

# Build WASM rules (release)
wasm:
	cd rules && cargo build --target wasm32-wasip1 --release

# Build everything (debug host + release wasm)
build-all: build wasm

# Build everything (release)
release-all: release wasm

# =============================================================================
# Maintenance
# =============================================================================

# Clean all build artifacts
clean:
	cargo clean
	cd rules && cargo clean

# =============================================================================
# Git Configuration
# =============================================================================

# Create a new git worktree
# Usage: make worktree name=my-branch [base=main]
BASE ?= main
worktree:
	@if [ -z "$(name)" ]; then echo "Error: name is required. Usage: make worktree name=<branch-name>"; exit 1; fi
	git worktree add -b $(name) .worktrees/$(name) $(BASE)
