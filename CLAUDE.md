# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Texide is a high-performance natural language linter written in Rust, inspired by textlint. It uses WASM-based rules for extensibility and supports parallel processing with caching.

## Common Commands

```bash
# Build all crates
cargo build --workspace

# Run tests
cargo test --workspace

# Run specific crate tests
cargo test -p texide_ast

# Format code
cargo fmt --all

# Run clippy
cargo clippy --workspace --all-targets -- -D warnings

# Run the CLI
cargo run --bin texide_cli -- lint tests/fixtures/

# Run with debug logging
RUST_LOG=debug cargo run --bin texide_cli -- lint tests/fixtures/

# Build WASM rule (requires wasm32-wasip1 target)
rustup target add wasm32-wasip1
cargo build --target wasm32-wasip1 --release
```

## Architecture

### Crate Structure

```
texide_cli          # CLI application (binary)
    └── texide_core     # Linter orchestration, config, parallel processing
            ├── texide_parser   # Parser trait + Markdown/PlainText parsers
            │       └── texide_ast      # TxtAST types, Arena allocator (bumpalo)
            ├── texide_plugin   # WASM plugin system (Extism/wasmi)
            └── texide_cache    # File-level caching with BLAKE3
texide_lsp          # LSP server (placeholder)
texide_wasm         # Browser WASM bindings
```

### Data Flow

1. CLI receives file patterns → Linter discovers files via glob
2. For each file (parallel via rayon):
   - Check cache validity (content hash + config + rule versions)
   - If invalid: parse file → convert AST to JSON → run WASM rules → cache result
3. Aggregate diagnostics → output

### Key Design Decisions

- **Arena Allocation**: All AST nodes use bumpalo for cache-friendly allocation. No `Box`, `Rc`, `Arc` for child nodes.
- **WASM Sandbox**: Rules compile to `wasm32-wasip1` and run via Extism (native) or wasmi (browser).
- **Feature Flags**: `texide_plugin` and `texide_core` use `native` (default) and `browser` features for different WASM runtimes.

## Code Style

- **Comments**: All code comments must be in English
- **Error Handling**: Use `thiserror` for definitions, `miette` for user-facing display. Avoid `.unwrap()` in library code.
- **Formatting**: rustfmt with `edition = "2024"`, `imports_granularity = "Module"`, `group_imports = "StdExternalCrate"`

## WASM Rule Interface

Rules implement two functions:
- `get_manifest()` → Returns rule metadata JSON
- `lint(ast_json, context, source, config)` → Returns diagnostics JSON
