# Rule Development Guide

Create custom lint rules for Texide in any language that compiles to WebAssembly.

## Quick Start (Rust)

### 1. Create a New Rule

```bash
# From the rules/ directory
cd rules

# Create rule directory
mkdir my-rule && cd my-rule

# Initialize Cargo.toml
cat > Cargo.toml << 'EOF'
[package]
name = "texide-rule-my-rule"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
texide-rule-common = { path = "../common" }
extism-pdk = "1.3"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

[dev-dependencies]
pretty_assertions = "1.4"
EOF

# Create src directory
mkdir src
```

### 2. Implement the Rule

Create `src/lib.rs`:

```rust
use extism_pdk::*;
use serde::Deserialize;
use texide_rule_common::{
    extract_node_text, is_node_type,
    Diagnostic, LintRequest, LintResponse, RuleManifest, Span,
};

const RULE_ID: &str = "my-rule";
const VERSION: &str = "0.1.0";

/// Rule configuration (optional)
#[derive(Debug, Deserialize, Default)]
struct Config {
    // Add your config options here
}

/// Returns rule metadata
#[plugin_fn]
pub fn get_manifest() -> FnResult<String> {
    let manifest = RuleManifest::new(RULE_ID, VERSION)
        .with_description("My custom rule description")
        .with_fixable(false)
        .with_node_types(vec!["Str".to_string()]);
    Ok(serde_json::to_string(&manifest)?)
}

/// Lints a single AST node
#[plugin_fn]
pub fn lint(input: String) -> FnResult<String> {
    let request: LintRequest = serde_json::from_str(&input)?;
    let mut diagnostics = Vec::new();

    // Only process nodes we care about
    if !is_node_type(&request.node, "Str") {
        return Ok(serde_json::to_string(&LintResponse { diagnostics })?);
    }

    // Parse configuration
    let _config: Config = serde_json::from_value(request.config.clone())
        .unwrap_or_default();

    // Extract text from node
    if let Some((start, end, text)) = extract_node_text(&request.node, &request.source) {
        // Your lint logic here
        if text.contains("BAD_PATTERN") {
            diagnostics.push(Diagnostic::warning(
                RULE_ID,
                "Found bad pattern in text",
                Span::new(start as u32, end as u32),
            ));
        }
    }

    Ok(serde_json::to_string(&LintResponse { diagnostics })?)
}
```

### 3. Build and Test

```bash
# Install WASM target (one-time)
rustup target add wasm32-wasip1

# Build the rule
cargo build --target wasm32-wasip1 --release

# Output: target/wasm32-wasip1/release/texide_rule_my_rule.wasm

# Run unit tests
cargo test
```

### 4. Use the Rule

```bash
# Copy to your project
cp target/wasm32-wasip1/release/texide_rule_my_rule.wasm ~/.texide/rules/

# Configure in .texide.jsonc
cat > .texide.jsonc << 'EOF'
{
  "plugins": ["~/.texide/rules/texide_rule_my_rule.wasm"],
  "rules": {
    "my-rule": true
  }
}
EOF

# Run linting
texide lint .
```

---

## Rule Interface

Every rule must implement two functions. See [WASM Interface Specification](./wasm-interface.md) for details.

### `get_manifest() -> String`

Returns JSON metadata about the rule:

```json
{
  "name": "my-rule",
  "version": "1.0.0",
  "description": "What this rule checks",
  "fixable": false,
  "node_types": ["Str", "Paragraph"]
}
```

### `lint(input: String) -> String`

Receives a lint request:

```json
{
  "node": { "type": "Str", "range": [0, 50] },
  "config": { "maxLength": 100 },
  "source": "Full file content...",
  "file_path": "path/to/file.md"
}
```

Returns diagnostics:

```json
{
  "diagnostics": [
    {
      "rule_id": "my-rule",
      "message": "Error description",
      "span": { "start": 10, "end": 20 },
      "severity": "warning"
    }
  ]
}
```

---

## Helper Functions

The `texide-rule-common` crate provides utilities:

### `extract_node_text(node, source)`

Extracts text content from an AST node:

```rust
if let Some((start, end, text)) = extract_node_text(&request.node, &request.source) {
    // start: byte offset (usize)
    // end: byte offset (usize)
    // text: &str slice
}
```

### `is_node_type(node, type_str)`

Checks if a node matches the expected type:

```rust
if is_node_type(&request.node, "Str") {
    // Process text node
}
```

### `get_node_type(node)`

Gets the node type as a string:

```rust
match get_node_type(&request.node) {
    Some("Str") => { /* ... */ }
    Some("Paragraph") => { /* ... */ }
    _ => {}
}
```

---

## Node Types

Specify which nodes your rule receives in `node_types`:

### Block Elements

| Type | Use Case |
|------|----------|
| `Document` | Process entire document |
| `Paragraph` | Check paragraph structure |
| `Header` | Validate headings |
| `BlockQuote` | Check quoted content |
| `List` | Validate list structure |
| `ListItem` | Check list items |
| `CodeBlock` | Analyze code blocks |
| `Table` | Check table content |

### Inline Elements

| Type | Use Case |
|------|----------|
| `Str` | **Most common** - plain text analysis |
| `Emphasis` | Check emphasized text |
| `Strong` | Check bold text |
| `Code` | Validate inline code |
| `Link` | Check URLs and link text |
| `Image` | Validate image references |

**Tip**: Start with `["Str"]` for text-focused rules.

---

## Configuration

### Defining Config Options

```rust
#[derive(Debug, Deserialize, Default)]
struct Config {
    /// Maximum allowed length (default: 100)
    #[serde(default = "default_max")]
    max: usize,

    /// Patterns to ignore
    #[serde(default)]
    ignore_patterns: Vec<String>,

    /// Enable strict mode
    #[serde(default)]
    strict: bool,
}

fn default_max() -> usize { 100 }
```

### User Configuration

In `.texide.jsonc`:

```json
{
  "rules": {
    "my-rule": true,
    "my-rule": "error",
    "my-rule": {
      "max": 80,
      "strict": true
    }
  }
}
```

---

## Auto-Fix Support

To provide auto-fixes:

1. Set `fixable: true` in manifest
2. Include `fix` in diagnostics

```rust
use texide_rule_common::Fix;

#[plugin_fn]
pub fn get_manifest() -> FnResult<String> {
    let manifest = RuleManifest::new(RULE_ID, VERSION)
        .with_fixable(true);  // Enable fixes
    // ...
}

#[plugin_fn]
pub fn lint(input: String) -> FnResult<String> {
    // ...
    diagnostics.push(
        Diagnostic::warning(RULE_ID, "Issue found", span)
            .with_fix(Fix::new(span, "replacement text"))
    );
    // ...
}
```

### Fix Types

```rust
// Replace text
Fix::new(Span::new(10, 20), "new text")

// Insert at position
Fix::insert(15, "inserted text")

// Delete range
Fix::delete(Span::new(10, 20))
```

---

## Testing

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn create_request(text: &str, config: serde_json::Value) -> String {
        serde_json::json!({
            "node": { "type": "Str", "range": [0, text.len()] },
            "config": config,
            "source": text,
            "file_path": null
        }).to_string()
    }

    #[test]
    fn detects_bad_pattern() {
        let input = create_request("This has BAD_PATTERN here", serde_json::json!({}));
        let output = lint(input).unwrap();
        let response: LintResponse = serde_json::from_str(&output).unwrap();

        assert_eq!(response.diagnostics.len(), 1);
        assert_eq!(response.diagnostics[0].rule_id, "my-rule");
    }

    #[test]
    fn ignores_good_text() {
        let input = create_request("This is clean text", serde_json::json!({}));
        let output = lint(input).unwrap();
        let response: LintResponse = serde_json::from_str(&output).unwrap();

        assert!(response.diagnostics.is_empty());
    }

    #[test]
    fn respects_config() {
        let input = create_request("test", serde_json::json!({ "max": 2 }));
        // ...
    }
}
```

### Integration Testing

```bash
# Create test file
echo "This has BAD_PATTERN in it" > test.md

# Run with your rule
texide lint --config test-config.json test.md
```

---

## Best Practices

### Performance

- Filter early with `is_node_type()`
- Avoid unnecessary string allocations
- Use `&str` references when possible
- Consider caching compiled regexes in config parsing

### Error Messages

- Be specific and actionable
- Include what was found and what was expected
- Suggest how to fix the issue

```rust
// Good
"Sentence has 150 characters, maximum is 100. Consider splitting into shorter sentences."

// Bad
"Too long"
```

### Severity Levels

| Level | Use Case |
|-------|----------|
| `error` | Must fix before commit |
| `warning` | Should fix, but not blocking |
| `info` | Suggestion or style preference |

### Configuration Defaults

- Provide sensible defaults
- Document all options
- Use `#[serde(default)]` for optional fields

---

## Publishing

### Option 1: GitHub Releases (Recommended)

> [!WARNING]
> **Not Yet Implemented**: GitHub-based plugin distribution is planned but not yet implemented.
> The specification described here is subject to change.

By publishing on GitHub Releases, users can easily install your plugin using the `owner/repo` format.

```bash
# Build release
cargo build --target wasm32-wasip1 --release

# Calculate hash
HASH=$(shasum -a 256 target/wasm32-wasip1/release/texide_rule_my_rule.wasm | cut -d' ' -f1)

# Create texide-rule.json (required for GitHub distribution)
cat > texide-rule.json << EOF
{
  "\$schema": "https://raw.githubusercontent.com/simorgh3196/texide/main/schemas/v1/rule.json",
  "rule": {
    "name": "my-rule",
    "version": "1.0.0",
    "description": "My custom lint rule",
    "repository": "https://github.com/yourname/texide-rule-my-rule",
    "license": "MIT",
    "fixable": false,
    "node_types": ["Str"]
  },
  "artifacts": {
    "wasm": "https://github.com/yourname/texide-rule-my-rule/releases/download/v{version}/texide_rule_my_rule.wasm"
  },
  "security": {
    "sha256": "$HASH"
  }
}
EOF

# Create GitHub release with .wasm file
gh release create v1.0.0 \
  target/wasm32-wasip1/release/texide_rule_my_rule.wasm \
  texide-rule.json
```

Users can install with:
```bash
texide plugin install yourname/texide-rule-my-rule
```

See [Plugin Distribution Guide](./plugin-distribution.md) for details.

### Option 2: Direct Distribution

Share the `.wasm` file directly. Users add to `.texide.jsonc`:

```json
{
  "plugins": ["./rules/my-rule.wasm"]
}
```

---

## Language Support

### Rust (Recommended)

Use `extism-pdk` and `texide-rule-common`:

```toml
[dependencies]
texide-rule-common = { git = "https://github.com/simorgh3196/texide" }
extism-pdk = "1.3"
```

### AssemblyScript

See [AssemblyScript Template](../templates/assemblyscript/).

```typescript
import { JSON } from "json-as";
import { Host, Output } from "@aspect/as-pdk";

export function get_manifest(): i32 { /* ... */ }
export function lint(): i32 { /* ... */ }
```

### Other Languages

Any language with WASM and Extism PDK support:

- Go: `github.com/extism/go-pdk`
- Zig: `extism/zig-pdk`
- C/C++: `extism/c-pdk`

Generate types from [JSON Schema](../schemas/rule-types.json).

---

## Troubleshooting

### Build Errors

**"wasm32-wasip1 target not found"**
```bash
rustup target add wasm32-wasip1
```

**"crate-type cdylib required"**
```toml
[lib]
crate-type = ["cdylib"]
```

### Runtime Errors

**"get_manifest function not found"**
- Ensure `#[plugin_fn]` attribute is present
- Check function name spelling

**"invalid JSON response"**
- Verify serde serialization
- Check for valid UTF-8 in messages

### Debugging

```bash
# Enable debug logging
RUST_LOG=debug texide lint file.md

# Test with specific input
echo '{"node":{"type":"Str","range":[0,5]},"config":{},"source":"hello","file_path":null}' | \
  texide test-rule my-rule.wasm
```

---

## Examples

See [rules/](../rules/) for complete examples:

- [no-todo](../rules/no-todo/) - Pattern detection
- [sentence-length](../rules/sentence-length/) - Length validation
- [no-doubled-joshi](../rules/no-doubled-joshi/) - Japanese particle detection

---

## Reference

- [WASM Interface Specification](./wasm-interface.md)
- [JSON Schema Definitions](../schemas/)
- [Extism PDK Documentation](https://extism.org/docs/write-a-plugin/rust-pdk)
