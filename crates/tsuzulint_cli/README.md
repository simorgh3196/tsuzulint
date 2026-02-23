# tsuzulint_cli

Command-line interface (CLI) application for TsuzuLint. The binary name is `tzlint`.

## Overview

`tsuzulint_cli` is the **command-line interface (CLI) application** for the TsuzuLint project.

**Main responsibilities:**

- Provide the primary interaction point for users
- Wrap the `tsuzulint_core` crate, handling command-line argument parsing and result output
- Support developer workflows including configuration file management, plugin installation, and LSP server startup

## Installation

```bash
cargo install tsuzulint_cli
```

Or build from source:

```bash
git clone https://github.com/simorgh3196/tsuzulint
cd tsuzulint
cargo build --release --bin tzlint
```

## Usage

### Global Options

| Option | Short | Description |
| ------ | ----- | ----------- |
| `--config <PATH>` | `-c` | Specify configuration file path |
| `--verbose` | `-v` | Enable verbose debug output |
| `--no-cache` | | Disable caching |

### Subcommands

#### `lint` - Run linting on files

```bash
tzlint lint [OPTIONS] <PATTERNS>...
```

| Option | Description |
| ------ | ----------- |
| `--format <FORMAT>` | Output format (`text` / `json` / `sarif`). Default: `text` |
| `--fix` | Apply automatic fixes |
| `--dry-run` | Preview fixes only (requires `--fix`) |
| `--timings` | Display performance measurements |
| `--fail-on-resolve-error` | Exit with error on rule resolution failure |

**Examples:**

```bash
# Basic linting
tzlint lint src/**/*.md

# Output in JSON format
tzlint lint --format json src/**/*.md

# Apply automatic fixes
tzlint lint --fix src/**/*.md

# Preview fixes
tzlint lint --fix --dry-run src/**/*.md

# With performance timing
tzlint lint --timings src/**/*.md

# Multiple patterns
tzlint lint "src/**/*.md" "docs/**/*.md"
```

#### `init` - Initialize configuration file

```bash
tzlint init [--force]
```

- Creates a `.tsuzulint.jsonc` file
- `--force`: Overwrite existing file

**Generated file:**

```jsonc
{
  // TsuzuLint configuration file
  "rules": [
    // Add rules here
    // "owner/repo",
    // { "github": "owner/repo@v1.0", "as": "alias" }
  ],
  "options": {
    // Per-rule options
  }
}
```

#### `rules` - Rule management

```bash
tzlint rules create <NAME>   # Create new rule project
tzlint rules add <PATH>      # Add WASM rule
```

**Creating a rule project:**

```bash
tzlint rules create my-rule
cd my-rule
cargo build --release --target wasm32-wasip1
```

Generated structure:

```text
my-rule/
├── Cargo.toml       # Configuration for wasm32-wasip1 target
└── src/
    └── lib.rs       # Rule template
```

#### `lsp` - Start LSP server

```bash
tzlint lsp
```

Starts a Language Server Protocol server for editor integration.

#### `plugin` - Plugin management

```bash
tzlint plugin cache clean                          # Clear cache
tzlint plugin install [SPEC] [--url <URL>] [--as <ALIAS>]  # Install plugin
```

**Installation examples:**

```bash
# From GitHub
tzlint plugin install owner/repo

# Specific version
tzlint plugin install owner/repo@v1.0.0

# With alias
tzlint plugin install owner/repo --as my-rule

# From URL
tzlint plugin install --url https://example.com/rule.wasm --as external-rule
```

## Output Formats

### Text format (default)

```text
/path/to/file.md:
  0:10 error [rule-id]: Error message

Checked 5 files (2 from cache), found 3 issues
```

With `--timings` option enabled:

```text
Performance Timings:
Rule                            | Duration        | %
--------------------------------+-----------------+----------
no-todo                         | 15ms            | 45.0%
spell-check                     | 10ms            | 30.0%
--------------------------------+-----------------+----------
Total                           | 33ms
```

### JSON format

```json
[
  {
    "path": "/path/to/file.md",
    "diagnostics": [
      {
        "rule_id": "no-todo",
        "message": "Found TODO",
        "severity": "error",
        "span": { "start": 0, "end": 4 }
      }
    ]
  }
]
```

### SARIF format

SARIF (Static Analysis Results Interchange Format) is a standard format used by GitHub Advanced Security and other tools.

```bash
tzlint lint --format sarif src/**/*.md > results.sarif
```

## Exit Codes

| Code | Meaning |
| ---- | ------- |
| `0` | Success (no errors) |
| `1` | Linting errors found |
| `2` | Internal error (configuration error, invalid glob, etc.) |

## Editor Integration

### VS Code

1. Create an extension that starts the LSP server
2. Use the `tzlint lsp` command

### Neovim

```lua
local lspconfig = require('lspconfig')
lspconfig.tsuzulint.setup {
  cmd = { "tzlint", "lsp" },
  filetypes = { "markdown" },
}
```

## Integration with tsuzulint_core

The CLI uses the following components from `tsuzulint_core`:

| Component | Usage |
| --------- | ----- |
| `Linter` | Main linting engine |
| `LinterConfig` | Configuration file loading and management |
| `LintResult` | Lint result storage |
| `Severity` | Diagnostic severity |
| `apply_fixes_to_file` | Apply automatic fixes |
| `generate_sarif` | Generate SARIF format output |

## Security Features

- **Symbolic link protection**: Detects and refuses to write to symbolic links during `init` command and configuration file updates during plugin installation
- **Unix system protection**: Uses `O_NOFOLLOW` flag to prevent unintended file overwrites via symbolic links

## Dependencies

| Dependency Crate | Usage |
| ---------------- | ----- |
| `clap` | Command-line argument parsing |
| `miette` | User-friendly error display |
| `tracing` | Structured logging |
| `serde_json` | JSON output and configuration file processing |
| `jsonc-parser` | JSONC configuration file parsing |
| `tokio` | Async runtime |
| `tsuzulint_core` | Core linter functionality |
| `tsuzulint_lsp` | LSP server implementation |
| `tsuzulint_registry` | Plugin registry |
