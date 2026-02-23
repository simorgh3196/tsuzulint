# tsuzulint_plugin

A crate that provides a WASM plugin system. Compiles rules to WASM and executes them safely in a sandboxed environment.

## Overview

`tsuzulint_plugin` is the **WASM plugin system** at the core of TsuzuLint. This crate is responsible for:

- **WASM-based rule execution**: Compiles lint rules to WASM and executes them safely in a sandboxed environment
- **Plugin loading and management**: Loading, configuring, and unloading rules from WASM files or byte arrays
- **Host function provision**: Providing context information required during rule execution
- **Diagnostic collection**: Aggregating diagnostic results returned by rules

## Architecture

```text
┌─────────────────────────────────────────────────────────────┐
│                      PluginHost                              │
│  (High-level API)                                           │
│  - load_rule(), configure_rule(), run_rule()                │
│  - run_all_rules()                                          │
└─────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│                    RuleExecutor trait                        │
│  (Backend abstraction)                                       │
└─────────────────────────────────────────────────────────────┘
            │                           │
            ▼                           ▼
┌─────────────────────┐     ┌─────────────────────┐
│   ExtismExecutor    │     │    WasmiExecutor    │
│   (native feature)  │     │  (browser feature)  │
│                     │     │                     │
│ - wasmtime (JIT)    │     │ - Pure Rust         │
│ - Fast execution    │     │ - WASM-in-WASM      │
│ - CLI/Server use    │     │ - Browser use       │
└─────────────────────┘     └─────────────────────┘
```

## Plugin Interface

Each rule must export the following two functions:

```rust
// 1. Returns rule metadata (JSON)
fn get_manifest() -> String

// 2. Performs linting and returns diagnostic results (Msgpack)
fn lint(input_bytes: &[u8]) -> Vec<u8>
```

### Input Data Structure

```rust
struct LintRequest {
    tokens: Vec<Token>,       // Morphological analysis results
    sentences: Vec<Sentence>, // Sentence boundary information
    node: T,                  // AST node (serialized)
    source: &str,             // Source text
    file_path: Option<&str>,  // File path
}
```

### Output Data Structure

```rust
struct LintResponse {
    diagnostics: Vec<Diagnostic>, // List of diagnostic results
}
```

## Extism vs wasmi Comparison

| Feature | Extism (native) | wasmi (browser) |
| ------- | --------------- | --------------- |
| **Execution method** | JIT compilation | Interpreter |
| **Underlying tech** | wasmtime | Pure Rust |
| **Performance** | Fast | Slow |
| **Use environment** | CLI, Tauri, Server | Browser WASM (WASM-in-WASM) |
| **Security control** | Extism Manifest | wasmi StoreLimiter |

## Feature Flags

```toml
[features]
default = ["native"]
native = ["dep:extism", "dep:extism-manifest"]   # Extism backend
browser = ["dep:wasmi"]                           # wasmi backend
test-utils = ["dep:wat"]                          # Test utilities
rkyv = ["dep:rkyv", "tsuzulint_ast/rkyv"]         # Fast serialization
```

**Important constraints:**

- Either `native` or `browser` must be enabled (compilation error otherwise)
- If both are enabled, `native` takes precedence

## Key Types

### RuleManifest (Rule Metadata)

```rust
pub struct RuleManifest {
    pub name: String,                    // Rule ID
    pub version: String,                 // Semantic version
    pub description: Option<String>,     // Description
    pub fixable: bool,                   // Auto-fixable
    pub node_types: Vec<String>,         // Target node types
    pub isolation_level: IsolationLevel, // Isolation level
    pub schema: Option<Value>,           // Configuration JSON Schema
}

pub enum IsolationLevel {
    Global,  // Rules requiring the entire document
    Block,   // Independently executable at block level
}
```

### Diagnostic (Diagnostic Result)

```rust
pub struct Diagnostic {
    pub rule_id: String,          // Rule ID
    pub message: String,          // Message
    pub span: Span,               // Byte range
    pub loc: Option<Location>,    // Line/column position
    pub severity: Severity,       // Severity
    pub fix: Option<Fix>,         // Auto-fix
}

pub enum Severity {
    Info,
    Warning,
    Error,  // Default
}

pub struct Fix {
    pub span: Span,    // Replacement range
    pub text: String,  // Replacement text
}
```

## Security Features

### Resource Limits

- **Memory**: 128MB limit (DoS prevention)
- **CPU**: Fuel limit (1 billion instructions = infinite loop prevention)
- **Time**: Timeout (5 seconds = unresponsive rule prevention)

### Access Control

- Network access: Completely denied
- Filesystem access: Completely denied
- Environment variables: Cleared

## Usage Example

```rust
use tsuzulint_plugin::PluginHost;

// Create host
let mut host = PluginHost::new();

// Load rule
host.load_rule("./rules/no-todo.wasm")?;

// Configure rule (optional)
host.configure_rule("no-todo", serde_json::json!({
    "allow": ["TODO", "FIXME"]
}))?;

// Execute rule
let diagnostics = host.run_rule(
    "no-todo",
    &ast_node,
    "source content",
    &tokens_json,
    &sentences_json,
    Some("example.md")
)?;

// Execute all rules at once
let all_diagnostics = host.run_all_rules(
    &ast_node,
    "source content",
    &tokens_json,
    &sentences_json,
    Some("example.md")
)?;

// Process results
for diag in diagnostics {
    println!("{}: {} at {:?}", diag.rule_id, diag.message, diag.span);
}

// Unload rule
host.unload_rule("no-todo");
```

## Dependencies

| Dependency | Purpose |
| ---------- | ------- |
| `extism` (optional) | WASM execution in native environment |
| `extism-manifest` (optional) | Extism plugin configuration |
| `wasmi` (optional) | WASM execution in browser environment (interpreter) |
| `serde` / `serde_json` | JSON serialization |
| `rmp-serde` | MessagePack serialization (performance) |
| `thiserror` | Error type definition |
| `tracing` | Logging |
| `tsuzulint_ast` | AST type definitions |
| `tsuzulint_text` | Token/Sentence type definitions |

**Why MessagePack is used:**

- Faster serialization/deserialization than JSON
- More compact binary format
- Optimizes data transfer between host and WASM

## Module Structure

```text
src/
├── lib.rs              # Crate entry point
├── executor.rs         # RuleExecutor trait
├── executor_extism.rs  # Extism backend (native feature)
├── executor_wasmi.rs   # wasmi backend (browser feature)
├── host.rs             # PluginHost (high-level API)
├── manifest.rs         # RuleManifest type
├── diagnostic.rs       # Diagnostic/Severity/Fix types
├── error.rs            # PluginError type
└── test_utils.rs       # Test utilities (test-utils feature)
```
