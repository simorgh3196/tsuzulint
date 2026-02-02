# TsuzuLint Rules

WASM-based lint rules for TsuzuLint.

## Quick Start: Create a New Rule

### Option 1: Copy from Template (Recommended)

```bash
# From repo root
cp -r templates/rust rules/my-rule
cd rules/my-rule

# Replace placeholders
sed -i '' 's/{{RULE_NAME}}/my-rule/g' Cargo.toml src/lib.rs
sed -i '' 's/{{RULE_DESCRIPTION}}/My custom lint rule/g' Cargo.toml src/lib.rs

# Build
cargo build --target wasm32-wasip1 --release
```

### Option 2: Copy from Existing Rule

```bash
cp -r rules/no-todo rules/my-rule
cd rules/my-rule
# Update Cargo.toml and src/lib.rs
```

### Option 3: Manual Setup

```bash
cd rules
mkdir my-rule && cd my-rule

cat > Cargo.toml << 'EOF'
[package]
name = "tsuzulint-rule-my-rule"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
tsuzulint-rule-pdk = { path = "../rules-pdk" }
extism-pdk = "1.3"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
EOF

mkdir src
# Create src/lib.rs (see templates/rust/src/lib.rs)
```

## Building Rules

### Prerequisites

```bash
# Install WASM target (one-time)
rustup target add wasm32-wasip1
```

### Build Commands

```bash
# Build all rules
cd rules
cargo build --target wasm32-wasip1 --release

# Build specific rule
cargo build --target wasm32-wasip1 --release -p tsuzulint-rule-no-todo

# Run tests
cargo test --workspace
```

### Output

Built WASM files are located at:

```text
rules/target/wasm32-wasip1/release/
├── tsuzulint_rule_no_todo.wasm
├── tsuzulint_rule_sentence_length.wasm
└── tsuzulint_rule_no_doubled_joshi.wasm
```

## Available Rules

| Rule | Description | Fixable |
| :--- | :--- | :--- |
| [no-todo](no-todo/) | Disallow TODO/FIXME comments | No |
| [sentence-length](sentence-length/) | Check sentence length | No |
| [no-doubled-joshi](no-doubled-joshi/) | Detect repeated Japanese particles | Yes |

## Rule Configuration

### no-todo

Detects TODO/FIXME/XXX comments.

```json
{
  "rules": {
    "no-todo": {
      "patterns": ["TODO:", "FIXME:", "HACK:"],
      "ignore_patterns": ["TODO-OK:"],
      "case_sensitive": false
    }
  }
}
```

### sentence-length

Checks sentence length limits.

```json
{
  "rules": {
    "sentence-length": {
      "max": 100,
      "skip_code": true
    }
  }
}
```

### no-doubled-joshi

Detects repeated Japanese particles.（助詞の重複を検出）

```json
{
  "rules": {
    "no-doubled-joshi": {
      "particles": ["は", "が", "を", "に", "で", "と", "も", "の"],
      "min_interval": 0,
      "allow": [],
      "suggest_fix": true
    }
  }
}
```

## Rule Interface

Every rule must export two functions:

### `get_manifest() -> String`

Returns JSON metadata:

```json
{
  "name": "rule-id",
  "version": "1.0.0",
  "description": "What this rule checks",
  "fixable": false,
  "node_types": ["Str"]
}
```

### `lint(input: String) -> String`

Receives lint request, returns diagnostics:

```rust
use tsuzulint_rule_pdk::{
    extract_node_text, is_node_type,
    Diagnostic, LintRequest, LintResponse, RuleManifest, Span,
};

#[plugin_fn]
pub fn lint(input: String) -> FnResult<String> {
    let request: LintRequest = serde_json::from_str(&input)?;
    let mut diagnostics = Vec::new();

    if let Some((start, end, text)) = extract_node_text(&request.node, &request.source) {
        // Your lint logic here
    }

    Ok(serde_json::to_string(&LintResponse { diagnostics })?)
}
```

## Directory Structure

```text
rules/
├── Cargo.toml              # Workspace manifest
├── README.md               # This file
├── rules-pdk/             # Shared types library
│   ├── Cargo.toml
│   └── src/lib.rs
├── no-todo/                # Sample rule
│   ├── Cargo.toml
│   └── src/lib.rs
├── sentence-length/        # Sample rule
└── no-doubled-joshi/       # Sample rule
```

## Resources

- [Rule Development Guide](../docs/rule-development.md) - Complete guide
- [WASM Interface Spec](../docs/wasm-interface.md) - Protocol details
- [JSON Schema](../schemas/rule-types.json) - Type definitions
- [Templates](../templates/) - Rust & AssemblyScript templates
