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
	cd rules && cargo test $(if $(TARGET),--target $(TARGET),) -p tsuzulint-rule-pdk

# Run tests with output
test-verbose:
	cargo test --workspace -- --nocapture

# Run clippy
lint:
	cargo clippy --workspace --all-targets -- -D warnings

# Run markdownlint
lint-md:
	npx markdownlint-cli2 ".github/**/*.md" "README.md" "AGENTS.md" "docs/**/*.md" "editors/vscode/README.md" "rules/**/*.md" "schemas/**/*.md" "templates/**/*.md"

# Format code
fmt:
	cargo fmt --all

# Format check (for CI)
fmt-check:
	cargo fmt --all -- --check

# Format markdown
fmt-md:
	npx markdownlint-cli2 ".github/**/*.md" "README.md" "AGENTS.md" "docs/**/*.md" "editors/vscode/README.md" "rules/**/*.md" "schemas/**/*.md" "templates/**/*.md" --fix

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
