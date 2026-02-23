# tsuzulint_lsp

Language Server Protocol (LSP) server implementation. Provides real-time linting functionality in editors/IDEs.

## Overview

**tsuzulint_lsp** is TsuzuLint's Language Server Protocol (LSP) server implementation. It provides real-time linting functionality in editors/IDEs and handles the following responsibilities:

- Standardized communication with editors
- Real-time document validation and diagnostic reporting
- Auto-fix functionality
- Document symbols (outline) support
- Dynamic configuration file reloading

## Architecture

```text
┌─────────────────────────────────────────────────────┐
│                    Backend                          │
│  (tower-lsp LanguageServer trait implementation)    │
├─────────────────────────────────────────────────────┤
│  BackendState                                       │
│  ├── documents: RwLock<HashMap<Url, DocumentData>>  │
│  ├── linter: RwLock<Option<Linter>>                │
│  └── workspace_root: RwLock<Option<PathBuf>>       │
└─────────────────────────────────────────────────────┘
           │
           ▼
┌─────────────────────────────────────────────────────┐
│              tsuzulint_core::Linter                 │
│              tsuzulint_parser::Parser               │
└─────────────────────────────────────────────────────┘
```

## Implemented LSP Features

### Text Synchronization

| Method | Feature | Description |
| ------- | ------ | ------ |
| `textDocument/didOpen` | Document open | Stores document in cache, immediately runs validation |
| `textDocument/didChange` | Document change | Updates cache with changes, validates after **300ms debounce** |
| `textDocument/didSave` | Document save | Re-validates on save |
| `textDocument/didClose` | Document close | Removes from cache, clears diagnostics |

### Diagnostics

- **Real-time diagnostics**: Automatically runs lint on document open/change
- **Non-blocking execution**: Offloads lint processing via `tokio::task::spawn_blocking`
- **Debounce**: 300ms delay on `didChange` to prevent excessive linting during continuous typing

### Code Actions (Auto-fix)

| Type | Title | Description |
| ------ | --------- | ------ |
| Quick Fix | "Fix: {diagnostic message}" | Single fix for individual diagnostics |
| Source Fix All | "Fix all TsuzuLint issues" | Batch fix all fixable issues at once |

### Document Symbols

Extracts structure from Markdown documents:

- Header → `SymbolKind::STRING`
- CodeBlock → `SymbolKind::FUNCTION`

Viewable in editor outline view.

### File Watching

Monitors configuration file changes via `workspace/didChangeWatchedFiles` and automatically reloads.

## ServerCapabilities

```rust
ServerCapabilities {
    text_document_sync: TextDocumentSyncOptions {
        open_close: true,
        change: TextDocumentSyncKind::FULL,
        save: SaveOptions { include_text: true }
    },
    code_action_provider: CodeActionOptions {
        code_action_kinds: [QUICKFIX, SOURCE_FIX_ALL]
    },
    document_symbol_provider: true
}
```

## Usage

### Starting from CLI

```bash
tzlint lsp
```

### Neovim Configuration Example

```lua
local lspconfig = require('lspconfig')

lspconfig.tsuzulint.setup {
  cmd = { "tzlint", "lsp" },
  filetypes = { "markdown" },
  root_dir = lspconfig.util.root_pattern('.tsuzulint.json', '.tsuzulint.jsonc', '.git'),
  settings = {},
}
```

### VS Code Extension

Extension `package.json`:

```json
{
  "contributes": {
    "languages": [
      {
        "id": "markdown",
        "extensions": [".md", ".markdown"]
      }
    ],
    "grammars": []
  },
  "activationEvents": [
    "onLanguage:markdown"
  ]
}
```

Extension TypeScript:

```typescript
import * as vscode from 'vscode';

export function activate(context: vscode.ExtensionContext) {
  const serverOptions: vscode.ServerOptions = {
    command: 'tzlint',
    args: ['lsp']
  };

  const clientOptions: vscode.LanguageClientOptions = {
    documentSelector: [{ scheme: 'file', language: 'markdown' }],
  };

  const client = new vscode.LanguageClient(
    'tsuzulint',
    'TsuzuLint',
    serverOptions,
    clientOptions
  );

  context.subscriptions.push(client.start());
}
```

## Debounce Behavior

`textDocument/didChange` applies a 300ms debounce to prevent excessive lint execution during continuous typing:

1. Receive document change event
2. Wait 300ms
3. If no new changes during wait → execute lint
4. If new changes occur during wait → reset wait timer

## Error Handling

- **RwLock poisoning**: Log error and return early on lock acquisition failure
- **Linter initialization failure**: Store as `None` and continue operation without linting
- **lint_text failure**: Return empty vector and log error

## Using tower-lsp

```rust
pub async fn run() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    
    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
```

## Dependencies

| Dependency | Purpose |
| ------- | ------ |
| `tsuzulint_core` | Linter core |
| `tsuzulint_parser` | Parser |
| `tsuzulint_ast` | AST types |
| `tower-lsp` | LSP framework |
| `tokio` | Async runtime |
| `serde_json` | JSON serialization |
| `tracing` | Logging |

## Current Status and Limitations

- **Basic implementation**: Currently only basic LSP features are implemented
- **No diagnostic caching**: `codeAction` re-runs lint (optimization opportunity)
- **Flat symbol list**: Nested symbol structures not yet supported
- **Simple debounce**: Fixed 300ms delay

## Testing

- **Unit tests**: Byte offset → LSP Position conversion
- **Integration tests**: Debounce behavior verification
