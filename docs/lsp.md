# Texide Language Server (LSP)

The Texide Language Server provides real-time linting, auto-fixes, and document structure analysis for editors (such as VS Code) using the Language Server Protocol (LSP).

## Features

The following features are currently supported:

### 1. Real-time Diagnostics

Provides feedback as you type or save a document based on Texide rules.
- Supports Error, Warning, and Info levels.
- Powered by `markdown-rs` and built-in plain text parsers.

### 2. Code Actions

Offers automated fixes based on diagnostic results.
- **Quick Fix**: Individual fixes for specific issues.
- **Fix All (`source.fixAll`)**: Automatically fixes all fixable issues in the entire file. This can be integrated with editor features like "Fix on Save."

### 3. Document Symbols

Analyzes document structure to provide the "Outline" view and "Go to Symbol" functionality.
- **Headers**: Structured navigation through document headings.
- **Code Blocks**: Identification of code regions within documents.

### 4. Configuration Hot-Reloading

Automatically reloads configuration when `.texide.json` or `.texide.jsonc` is modified. The server does not need to be restarted for changes to take effect.

## Architecture

`texide_lsp` is built using the `tower-lsp` crate. It utilizes `tokio` for asynchronous task management and communicates via standard input/output (stdin/stdout).

### Technical Highlights

- **Arena Allocation**: Leverages `AstArena` from `texide_ast` to optimize memory allocation during document parsing.
- **Sequential Processing**: Currently processes requests sequentially to handle internal non-thread-safe dependencies (planned for thread-safe optimization in the future).

## Usage

### Starting the Server

The LSP server is integrated into the main `texide` CLI. It can be started using the `lsp` subcommand:

```bash
texide lsp
```

### VS Code Configuration Example

To enable `source.fixAll` on save, add the following to your `settings.json`:

```json
{
  "[markdown]": {
    "editor.codeActionsOnSave": {
      "source.fixAll": "explicit"
    }
  }
}
```

## Future Roadmap

- [ ] Support for hierarchical document symbols (currently providing a flat list).
- [ ] `textDocument/formatting` support.
- [ ] Enhanced rule documentation via Hover.
- [ ] Further optimization for incremental document synchronization.
