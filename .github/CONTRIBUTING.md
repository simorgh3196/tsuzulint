# Contributing to Texide

Thank you for your interest in contributing to Texide! This document provides guidelines and information for contributors.

## Getting Started

### Prerequisites

- Rust 1.85 or later
- wasm32-wasip1 target (for building rules)

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Add WASM target
rustup target add wasm32-wasip1
```

### Development Setup

```bash
# Clone the repository
git clone https://github.com/simorgh3196/texide.git
cd texide

# Build all crates
cargo build

# Run tests
cargo test --workspace

# Run with debug logging
RUST_LOG=debug cargo run --bin texide -- lint tests/fixtures/
```

## Project Structure

```shell
texide/
├── crates/
│   ├── texide_ast/      # TxtAST definitions and Arena allocator
│   ├── texide_parser/   # Parser trait and implementations
│   ├── texide_plugin/   # WASM plugin system (Extism host)
│   ├── texide_cache/    # Caching system
│   ├── texide_core/     # Core linter engine
│   ├── texide_cli/      # CLI application
│   └── texide_lsp/      # LSP server
├── rules/                 # Built-in sample rules
├── docs/                  # Documentation
└── tests/                 # Integration tests
```

## Development Workflow

### 1. Create a Feature Branch

> [!IMPORTANT]
> **Direct commits to `main` are NOT allowed.**
> Please always create a Pull Request from your feature branch.

```bash
git checkout -b feature/your-feature-name
# or fix/bug-name, docs/update-readme, etc.
```

### 2. Make Your Changes

- Write code with English comments
- Follow Rust idioms and best practices
- Add tests for new functionality

### 3. Run Tests

```bash
# Run all tests
cargo test --workspace

# Run specific crate tests
cargo test -p texide_ast

# Run with coverage (requires cargo-llvm-cov)
cargo llvm-cov --workspace
```

### 4. Format and Lint

```bash
# Format code
cargo fmt --all

# Run clippy
cargo clippy --workspace -- -D warnings
```

### 5. Submit a Pull Request

- Provide a clear description of changes
- Reference any related issues
- Ensure CI passes

## Code Style

### Comments

All code comments should be in **English**:

```rust
// Good: English comments
/// Parses the input string into a TxtAST.
fn parse(input: &str) -> Result<TxtNode, ParseError> {
    // ...
}

// Bad: Non-English comments
/// 入力文字列をTxtASTにパースする
fn parse(input: &str) -> Result<TxtNode, ParseError> {
    // ...
}
```

### Documentation

- Use `///` for public API documentation
- Include examples where helpful
- Document panics, errors, and safety considerations

### Error Handling

- Use `thiserror` for error definitions
- Use `miette` for user-facing error display
- Avoid `.unwrap()` in library code

## Architecture Guidelines

### AST Nodes

- Use Arena allocation (`bumpalo`) for AST nodes
- Avoid `Box`, `Rc`, or `Arc` for child nodes
- Prefer immutable borrows where possible

### Plugin System

- All rules run in WASM sandbox via Extism
- Host functions provide limited capabilities
- Rules cannot access filesystem or network directly

### Performance

- Use `rayon` for parallel file processing
- Implement caching at file and line level
- Minimize allocations in hot paths

## Resources

- [Rust Book](https://doc.rust-lang.org/book/)
- [Extism Documentation](https://extism.org/docs/)
- [textlint Documentation](https://textlint.github.io/)
- [Oxc Source Code](https://github.com/oxc-project/oxc)
- [Biome Source Code](https://github.com/biomejs/biome)

## Questions?

- Open a [GitHub Issue](https://github.com/simorgh3196/texide/issues)
- Start a [Discussion](https://github.com/simorgh3196/texide/discussions)

Thank you for contributing!
