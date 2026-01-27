# Texide JSON Schemas

This directory contains JSON Schema definitions for Texide configuration and rule development.

## Schema Files

### Configuration Schemas (Versioned)

| Schema | Description | Usage |
|--------|-------------|-------|
| [v1/rule.json](v1/rule.json) | Rule manifest schema | `texide-rule.json` in rule repositories |
| [v1/config.json](v1/config.json) | Project configuration schema | `.texide.jsonc` in user projects |

### Type Definitions

| Schema | Description | Usage |
|--------|-------------|-------|
| [rule-types.json](rule-types.json) | WASM rule type definitions | Code generation for rule development |

## Schema Versioning

Schemas follow semantic versioning with the URL format:
```
https://raw.githubusercontent.com/simorgh3196/texide/main/schemas/v{major}/schema.json
```

- **Major version increments** for backward-incompatible changes (adding required fields, removing fields)
- **Backward-compatible changes** (adding optional fields) are updated within the same version
- Old schema versions are maintained for a period after deprecation

## Usage

### Rule Authors

Add `$schema` to your `texide-rule.json` for IDE auto-completion and validation:

```json
{
  "$schema": "https://raw.githubusercontent.com/simorgh3196/texide/main/schemas/v1/rule.json",
  "rule": {
    "name": "my-rule",
    "version": "1.0.0"
  },
  "artifacts": {
    "wasm": "https://github.com/.../releases/download/v{version}/rule.wasm"
  },
  "security": {
    "sha256": "..."
  }
}
```

### Project Configuration

Add `$schema` to your `.texide.jsonc`:

```json
{
  "$schema": "https://raw.githubusercontent.com/simorgh3196/texide/main/schemas/v1/config.json",
  "rules": [
    "simorgh3196/texide-rule-no-doubled-joshi"
  ],
  "options": {
    "no-doubled-joshi": true
  }
}
```

### Rule Development

#### Rust

Use the `texide-rule-foundation` crate which implements these types:

```rust
use texide_rule_common::{
    LintRequest, LintResponse, Diagnostic, Span, Fix, RuleManifest
};
```

#### TypeScript / AssemblyScript

Generate types using [quicktype](https://quicktype.io/):

```bash
# Install quicktype
npm install -g quicktype

# Generate TypeScript types
quicktype schemas/rule-types.json \
  --src-lang schema \
  --lang typescript \
  --out src/types.ts
```

#### Go

```bash
# Using gojsonschema
go install github.com/atombender/go-jsonschema/cmd/gojsonschema@latest

gojsonschema -p types schemas/rule-types.json -o types/rule_types.go
```

#### Other Languages

Use any JSON Schema code generator for your target language:
- Python: `datamodel-code-generator`
- Java: `jsonschema2pojo`
- C#: `NJsonSchema`

## Schema Reference

### v1/rule.json - Rule Manifest

| Section | Required | Description |
|---------|----------|-------------|
| `rule` | Yes | Rule metadata (name, version, description, fixable, node_types, etc.) |
| `artifacts` | Yes | Download URLs (wasm) |
| `security` | Yes | SHA256 hash for verification |
| `permissions` | No | Filesystem/network permissions (future) |
| `texide` | No | Texide version requirements |
| `options` | No | JSON Schema for rule configuration options |

### v1/config.json - Project Configuration

| Field | Required | Description |
|-------|----------|-------------|
| `rules` | No | List of rules to load |
| `options` | No | Rule options |
| `security` | No | Security settings |
| `cache` | No | Cache settings |
| `output` | No | Output formatting |
| `ignore` | No | Files to ignore |
| `include` | No | Files to include |

### rule-types.json - WASM Rule Types

| Type | Description |
|------|-------------|
| `RuleManifest` | Returned by `get_manifest()` function |
| `LintRequest` | Input to `lint()` function |
| `LintResponse` | Output from `lint()` function |
| `Diagnostic` | A single lint warning or error |
| `Span` | Byte range in source text |
| `Fix` | Auto-fix replacement |
| `AstNode` | TxtAST node |

## Validation

Validate your files:

```bash
# Using ajv-cli
npm install -g ajv-cli

# Validate texide-rule.json
ajv validate -s schemas/v1/rule.json -d texide-rule.json

# Validate .texide.jsonc
ajv validate -s schemas/v1/config.json -d .texide.jsonc

# Validate rule manifest output
ajv validate -s schemas/rule-types.json \
  --spec=draft7 \
  -r '#/$defs/RuleManifest' \
  -d manifest.json
```
