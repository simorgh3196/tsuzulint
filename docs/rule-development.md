# Rule Development Guide

This guide explains how to create custom lint rules for Texide.

## Prerequisites

- Rust 1.85+
- WASM target: `rustup target add wasm32-wasip1`

## Quick Start

```bash
# Create a new rule project
texide create-rule my-rule
cd my-rule

# Build the WASM module
cargo build --target wasm32-wasip1 --release

# The WASM file is at:
# target/wasm32-wasip1/release/my_rule.wasm
```

## Rule Structure

Every rule must export two functions:

### `get_manifest() -> String`

Returns the rule metadata as JSON:

```rust
#[plugin_fn]
pub fn get_manifest() -> FnResult<String> {
    let manifest = RuleManifest {
        name: "my-rule".to_string(),
        version: "1.0.0".to_string(),
        description: Some("Rule description".to_string()),
        fixable: false,
        node_types: vec!["Str".to_string()],
    };
    Ok(serde_json::to_string(&manifest)?)
}
```

### `lint(input: String) -> String`

Receives a lint request and returns diagnostics:

```rust
#[plugin_fn]
pub fn lint(input: String) -> FnResult<String> {
    let request: LintRequest = serde_json::from_str(&input)?;
    let mut diagnostics = Vec::new();

    // Your lint logic here
    // request.node - The AST node to check
    // request.config - Rule configuration
    // request.source - Full source text

    let response = LintResponse { diagnostics };
    Ok(serde_json::to_string(&response)?)
}
```

## Node Types

Rules can filter nodes by type. Available types:

| Block Elements | Inline Elements |
|----------------|-----------------|
| Document | Str |
| Paragraph | Break |
| Header | Emphasis |
| BlockQuote | Strong |
| List | Delete |
| ListItem | Code |
| CodeBlock | Link |
| HorizontalRule | Image |
| Html | LinkReference |
| Table | ImageReference |
| TableRow | FootnoteReference |
| TableCell | |

## Example: No TODO Rule

```rust
use extism_pdk::*;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct LintRequest {
    node: serde_json::Value,
    config: serde_json::Value,
    source: String,
}

#[derive(Serialize)]
struct Diagnostic {
    rule_id: String,
    message: String,
    span: Span,
    severity: String,
}

#[derive(Serialize)]
struct Span {
    start: u32,
    end: u32,
}

#[plugin_fn]
pub fn lint(input: String) -> FnResult<String> {
    let request: LintRequest = serde_json::from_str(&input)?;
    let mut diagnostics = Vec::new();

    // Check if node is a text node
    if request.node.get("type").and_then(|t| t.as_str()) == Some("Str") {
        if let Some(range) = request.node.get("range").and_then(|r| r.as_array()) {
            let start = range[0].as_u64().unwrap_or(0) as usize;
            let end = range[1].as_u64().unwrap_or(0) as usize;
            let text = &request.source[start..end];

            if text.contains("TODO") {
                diagnostics.push(Diagnostic {
                    rule_id: "no-todo".to_string(),
                    message: "TODO comments are not allowed".to_string(),
                    span: Span {
                        start: start as u32,
                        end: end as u32,
                    },
                    severity: "error".to_string(),
                });
            }
        }
    }

    let response = serde_json::json!({ "diagnostics": diagnostics });
    Ok(serde_json::to_string(&response)?)
}
```

## Configuration

Rules receive configuration from `.texide.json`:

```json
{
  "rules": {
    "my-rule": {
      "option1": "value1",
      "option2": 42
    }
  }
}
```

Access config in your rule:

```rust
if let Some(max) = request.config.get("maxLength").and_then(|v| v.as_u64()) {
    // Use max value
}
```

## Auto-fix Support

To provide auto-fixes, set `fixable: true` in manifest and include `fix`:

```rust
diagnostics.push(Diagnostic {
    rule_id: "my-rule".to_string(),
    message: "Message".to_string(),
    span: Span { start, end },
    severity: "error".to_string(),
    fix: Some(Fix {
        span: Span { start, end },
        text: "replacement text".to_string(),
    }),
});
```

## Testing

Create test files and run:

```bash
# Build your rule
cargo build --target wasm32-wasip1 --release

# Test with Texide
texide lint --config test-config.json test-files/
```

## Publishing

1. Build the WASM file
2. Publish to a registry (npm, crates.io, or GitHub releases)
3. Users install with: `texide add-rule <path-or-url>`

## Tips

- Keep rules focused on a single concern
- Use descriptive error messages
- Provide auto-fixes when possible
- Test with various input files
- Consider performance for large files
