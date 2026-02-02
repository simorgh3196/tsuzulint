# AGENTS.md

This file provides guidance for AI agents working with the Texide codebase.

## Project Overview

Texide is a high-performance natural language linter written in Rust, inspired by textlint. It uses WASM-based rules for extensibility and supports parallel processing with caching.

> [!WARNING]
> **This project is currently Research-only / WIP.**
> Users should not expect stability. All major changes must go through Pull Requests.

## Development Rules

1. **Do NOT commit directly to `main` branch.**
2. **Use Git Worktrees**: Use the `using-git-worktrees` skill to create isolated environments for each task.
3. **Confirm before Push**: After committing changes, ALWAYS ask the user for permission before pushing to the remote repository.
4. **Verify before Commit**: Always run `make lint`, `make fmt-check` and `make test` before committing to ensure there are no errors.
5. **Verify Markdown**: If you modify any `.md` file, you MUST run `markdownlint-cli2 "**/*.md" "#node_modules"` to ensure compliance with the documentation standard.
6. **Always create a Pull Request.**
    - Branch naming: `feat/name`, `fix/name`, `docs/name`
    - PR description must be clear.
7. **Tests must pass** before requesting review.

## Common Commands

```bash
# Build all crates
make build

# Run all tests
make test

# Run specific crate tests
cargo test -p texide_ast

# Format code
make fmt

# Run clippy
make lint

# Run the CLI
cargo run --bin texide -- lint tests/fixtures/

# Run with debug logging
RUST_LOG=debug cargo run --bin texide -- lint tests/fixtures/

# Build WASM rules
make wasm
```

## Resources

- **Rust Implementation**: Refer to your available skills for necessary knowledge and patterns when implementing Rust code.

## Architecture

### Crate Structure

```text
texide_cli                              # CLI application (binary)
    └── texide_core                     # Linter orchestration, config, parallel processing
            ├── texide_parser           # Parser trait + Markdown/PlainText parsers
            │       └── texide_ast      # TxtAST types, Arena allocator (bumpalo)
            ├── texide_plugin           # WASM plugin system (Extism/wasmi)
            └── texide_cache            # File-level caching with BLAKE3
texide_lsp                              # LSP server (basic implementation using tower-lsp)
texide_registry                         # Rule registry and package management
texide_wasm                             # Browser WASM bindings
```

### Data Flow

1. CLI receives file patterns → Linter discovers files via glob
2. For each file (parallel via rayon):
    - Check cache validity (content hash + config + rule versions)
    - If invalid: parse file → convert AST to JSON → run WASM rules → cache result
3. Aggregate diagnostics → output

### Key Design Decisions

- **Arena Allocation**: All AST nodes use bumpalo for cache-friendly allocation.
- **WASM Sandbox**: Rules compile to `wasm32-wasip1` and run via Extism (native) or wasmi (browser).
- **Feature Flags**: `texide_plugin` and `texide_core` use `native` (default) and `browser` features.

## Code Style

- **Comments**: All code comments must be in English.
- **Error Handling**: Use `thiserror` for definitions, `miette` for user-facing display. Avoid `.unwrap()` in library code.
- **Formatting**: rustfmt with `edition = "2024"`.

## WASM Rule Interface

Rules implement two functions:

- `get_manifest()` → Returns rule metadata JSON
- `lint(ast_json, context, source, config)` → Returns diagnostics JSON

## TDD Workflow

Refer to the `test-driven-development` skill for detailed instructions.

1. **Test behavior, not implementation**
2. **Red-Green-Refactor**
3. **Descriptive test names**
