# tsuzulint_core

The core crate of the linter engine. Integrates file discovery, parsing, rule execution, and caching.

## Overview

`tsuzulint_core` is the **linter engine** at the heart of the TsuzuLint project. As a high-performance natural language linter inspired by textlint, it handles the following key responsibilities:

- **Linter Orchestration**: Integration of file discovery, parsing, rule execution, and caching
- **Configuration Management**: Loading and validation of JSON/JSONC configuration files
- **Parallel Processing (rayon)**: High-speed parallel linting of multiple files
- **Caching**: Incremental caching based on BLAKE3 hashes
- **WASM Plugin Integration**: Rule execution through tsuzulint_plugin

## Architecture

### Data Flow

```text
CLI (receives patterns)
    ↓
Linter::lint_patterns()
    ↓
discover_files() → File discovery via glob/walkdir
    ↓
lint_files_parallel() [rayon parallel processing]
    ↓ (each file)
┌─────────────────────────────────────────┐
│ 1. Cache check (content hash +          │
│    config hash + rule versions)          │
│    - Hit → Return result from cache      │
│    - Miss → Execute parsing              │
│                                          │
│ 2. Parser selection (based on extension) │
│    - .md/.markdown → MarkdownParser      │
│    - Others → PlainTextParser            │
│                                          │
│ 3. AST construction (using AstArena)     │
│                                          │
│ 4. Tokenization & sentence splitting     │
│                                          │
│ 5. Incremental block caching             │
│    - Reuse unchanged blocks              │
│                                          │
│ 6. WASM rule execution                   │
│    - Global isolation: entire document   │
│    - Block isolation: changed blocks only│
│                                          │
│ 7. Save results to cache                 │
└─────────────────────────────────────────┘
    ↓
Aggregate results → Output
```

### Module Structure

| Module | Responsibility |
| ------ | -------------- |
| `linter.rs` | Main `Linter` struct. Linter orchestrator |
| `parallel_linter.rs` | Parallel file linting using rayon |
| `file_linter.rs` | Single file linting logic |
| `config.rs` | Configuration file loading and validation (JSON Schema) |
| `rule_loader.rs` | Plugin/rule loading and PluginHost initialization |
| `pool.rs` | Thread pool for PluginHost |
| `walker.rs` | Parallel file discovery using `ignore` crate |
| `context.rs` | LintContext - document structure caching |
| `fix.rs` / `fixer.rs` | Auto-fix functionality |
| `block_extractor.rs` | Block extraction for incremental caching |
| `manifest_resolver.rs` | Path resolution for rule manifests |
| `formatters/sarif.rs` | SARIF 2.1.0 format output |

## Configuration Processing

### LinterConfig Structure

```rust
pub struct LinterConfig {
    pub rules: Vec<RuleDefinition>,           // Plugins to load
    pub options: HashMap<String, RuleOption>, // Rule configuration
    pub include: Vec<String>,                 // File patterns to include
    pub exclude: Vec<String>,                 // File patterns to exclude
    pub cache: CacheConfig,                   // Cache configuration
    pub timings: bool,                        // Performance measurement
    pub base_dir: Option<PathBuf>,            // Base directory for config file
}
```

### Configuration File Format

- **Supported formats**: `.tsuzulint.json`, `.tsuzulint.jsonc` (comment support)
- **JSON Schema validation**: Validation via embedded schema

### Rule Definition Patterns

```json
{
  "rules": [
    "owner/repo",
    { "github": "owner/repo@v1.0", "as": "alias" },
    { "url": "https://...", "as": "url-rule" },
    { "path": "./local/rule.json", "as": "local" }
  ],
  "options": {
    "no-todo": true,
    "max-lines": { "max": 100 },
    "disabled-rule": false
  }
}
```

## Parallel Processing

### Parallel Linting with rayon

```rust
let results: Vec<Result<LintResult, (PathBuf, LinterError)>> = paths
    .par_iter()
    .map_init(
        || create_plugin_host(config, dynamic_rules),
        |host_result, path| {
            lint_file_internal(path, file_host, ...)
        }
    )
    .collect();
```

**Features:**

- **map_init**: Initialize PluginHost in each thread (avoid WASM reloading)
- **Thread-safe**: Mutex-protected cache access
- **Error separation**: Returns successes and failures separately

## Performance Optimization

### 1. Incremental Block Caching

- Cache files at block granularity
- Reuse diagnostic results for unchanged blocks
- Efficient O(Blocks + Diagnostics) distribution

### 2. PluginHost Pooling

```rust
pub struct PluginHostPool {
    available: Mutex<VecDeque<PluginHost>>,
    initializer: Option<Arc<HostInitializer>>,
}
```

- Reuse hosts with WASM modules already loaded
- LIFO order to maximize CPU cache efficiency

### 3. Early Rule Filtering

```rust
pub struct ContentCharacteristics {
    pub has_headings: bool,
    pub has_links: bool,
    pub has_code_blocks: bool,
}

pub fn should_skip_rule(&self, node_types: &[String]) -> bool
```

Pre-analyze document content to skip unnecessary rules.

### 4. RawValue Serialization Optimization

- Pass AST directly for single rule scenarios
- Serialize once using `RawValue` for multiple rules

## Auto-Fix Functionality

```rust
use tsuzulint_core::apply_fixes_to_file;

for result in &successes {
    if !result.diagnostics.is_empty() {
        apply_fixes_to_file(&result.path, &result.diagnostics)?;
    }
}
```

- Determine fix order via dependency graph
- Safe application via topological sort
- Iterative application to handle fix chains

## Usage Example

```rust
use tsuzulint_core::{Linter, LinterConfig};

// Load from configuration file
let config = LinterConfig::from_file(".tsuzulint.json")?;
let linter = Linter::new(config)?;

// Lint by pattern
let (successes, failures) = linter.lint_patterns(&["src/**/*.md".to_string()])?;

for result in successes {
    println!("{}: {} issues", result.path.display(), result.diagnostics.len());
}

// SARIF output
use tsuzulint_core::generate_sarif;
let sarif = generate_sarif(&successes)?;
println!("{}", sarif);
```

## Feature Flags

```toml
[features]
default = ["native"]
native = ["tsuzulint_plugin/native"]    # Extism (native WASM)
browser = ["tsuzulint_plugin/browser"]   # wasmi (browser WASM)
```

## Dependencies

| Dependency | Purpose |
| ---------- | ------- |
| `rayon` | Data parallel processing |
| `blake3` | Fast content hash calculation |
| `serde` / `serde_json` | JSON serialization |
| `jsonc-parser` | JSONC parsing |
| `jsonschema` | JSON Schema validation |
| `walkdir` / `ignore` | File discovery |
| `globset` | Glob pattern matching |
| `crossbeam-channel` | Multi-producer/consumer channels |
| `parking_lot` | High-performance Mutex |

## Public API

```rust
pub use config::{CacheConfig, LinterConfig, RuleDefinition};
pub use context::{DocumentStructure, LintContext};
pub use error::LinterError;
pub use fix::FixCoordinator;
pub use fixer::apply_fixes_to_file;
pub use formatters::generate_sarif;
pub use linter::Linter;
pub use pool::PluginHostPool;
pub use result::LintResult;
pub use tsuzulint_plugin::{Diagnostic, Fix, Severity};
```

## Security

### Path Traversal Protection

- Manifest paths: Reject absolute paths and `..` components
- WASM paths: Reject paths outside manifest directory
- canonicalize validation: Protection against symlink attacks
