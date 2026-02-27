# tsuzulint_manifest

A shared library for defining and validating external rule manifest files (`tsuzulint-rule.json`).

## Overview

`tsuzulint_manifest` is a shared library in the TsuzuLint project for **defining and validating external rule manifest files**.

**Key Responsibilities:**

- Providing type definitions that represent distribution metadata for external WASM rules
- Providing manifest validation functionality based on JSON Schema
- Providing integrity verification via SHA256 hash computation and validation
- Aggregating types shared across other crates (`tsuzulint_registry`, `tsuzulint_plugin`, etc.)

This crate is used during rule distribution and installation, centrally managing information such as rule names, versions, WASM file locations, integrity check hashes, and permission settings.

## Manifest Structure

### ExternalRuleManifest (Root Structure)

| Field | Type | Required | Description |
| ---------- | -- | ---- | ---- |
| `rule` | `RuleMetadata` | Yes | Rule metadata |
| `wasm` | `Vec<Wasm>` | Yes | Downloadable WASM artifact information |
| `permissions` | `Option<Permissions>` | No | Required permissions |
| `tsuzulint` | `Option<TsuzuLintCompatibility>` | No | TsuzuLint compatibility information |
| `options` | `Option<Value>` | No | JSON Schema for rule configuration options |

### RuleMetadata

| Field | Type | Required | Description |
| ---------- | - | ---- | ---- |
| `name` | `String` | Yes | Rule identifier. Pattern: `^[a-z][a-z0-9-]*$` |
| `version` | `String` | Yes | Semantic version |
| `description` | `Option<String>` | No | Rule description |
| `repository` | `Option<String>` | No | GitHub repository URL |
| `license` | `Option<String>` | No | License identifier (SPDX format recommended) |
| `authors` | `Vec<String>` | No | List of authors |
| `keywords` | `Vec<String>` | No | Keywords for search |
| `fixable` | `bool` | No | Whether auto-fix is available (default: `false`) |
| `node_types` | `Vec<String>` | No | AST node types to process (empty = all nodes) |
| `isolation_level` | `IsolationLevel` | No | Rule isolation level (default: `Global`) |

### IsolationLevel

```rust
pub enum IsolationLevel {
    Global,  // Full document required
    Block,   // Can run on individual blocks
}
```

### Wasm

| Field | Type | Required | Description |
| ---------- | - | ---- | ---- |
| `url` or `path` | `String` | Yes | Download URL or path for WASM file |
| `hash` | `String` | Yes | SHA256 hash of WASM file (64-character hex string) |

### Permissions

| Field | Type | Description |
| ---------- | - | ---- |
| `filesystem` | `Vec<FilesystemPermission>` | Filesystem access permissions |
| `network` | `Vec<NetworkPermission>` | Network access permissions |

## Example Manifest File

```json
{
  "rule": {
    "name": "no-todo",
    "version": "1.0.0",
    "description": "Disallow TODO comments in text",
    "repository": "https://github.com/owner/tsuzulint-rule-no-todo",
    "license": "MIT",
    "authors": ["Author Name"],
    "keywords": ["todo", "comments"],
    "fixable": true,
    "node_types": ["Str"],
    "isolation_level": "Block"
  },
  "wasm": [{
    "url": "https://github.com/owner/tsuzulint-rule-no-todo/releases/download/v{version}/rule.wasm",
    "hash": "abc123def456...64chars"
  }],
  "permissions": {
    "filesystem": [],
    "network": []
  },
  "tsuzulint": {
    "min_version": "0.1.0"
  },
  "options": {
    "type": "object",
    "properties": {
      "allow": {
        "type": "array",
        "items": { "type": "string" },
        "description": "List of allowed TODO patterns"
      }
    }
  }
}
```

## JSON Schema Validation

### Embedded Schema

```rust
const RULE_SCHEMA_JSON: &str = include_str!("../../../schemas/v1/rule.json");
```

- JSON Schema files are **embedded at compile time**
- Using the `include_str!` macro eliminates runtime file I/O
- Schema is included in the binary, simplifying deployment

### Lazy Initialization Pattern

```rust
static SCHEMA: OnceLock<Validator> = OnceLock::new();
```

- Uses `OnceLock` to initialize the validator **thread-safely, exactly once**
- Schema is compiled only on the first validation
- Subsequent validations reuse the compiled validator

### Validation Flow

```rust
pub fn validate_manifest(json_str: &str) -> Result<ExternalRuleManifest, ManifestError>
```

1. **JSON Parse**: Convert input string to `serde_json::Value`
2. **Schema Validation**: Validate against JSON Schema Draft-07
3. **Struct Deserialization**: Convert validated JSON to `ExternalRuleManifest`

## Error Handling

```rust
pub enum ManifestError {
    ParseError(#[from] serde_json::Error),  // JSON parse error
    ValidationError(String),                 // Schema validation error
}
```

- Error definitions using `thiserror`
- User-friendly error messages (including path to error location)

## Usage Examples

### Basic Usage

```rust
use tsuzulint_manifest::{validate_manifest, ExternalRuleManifest};

let json = r#"{
    "rule": {
        "name": "no-todo",
        "version": "1.0.0",
        "description": "Disallow TODO comments"
    },
    "wasm": [{
        "url": "https://example.com/rule.wasm",
        "hash": "abc123...64chars"
    }]
}"#;

match validate_manifest(json) {
    Ok(manifest) => {
        println!("Rule: {} v{}", manifest.rule.name, manifest.rule.version);
    }
    Err(e) => {
        eprintln!("Validation failed: {}", e);
    }
}
```

### Creating from Structs Directly

```rust
use tsuzulint_manifest::{ExternalRuleManifest, RuleMetadata, Artifacts};

let manifest = ExternalRuleManifest {
    rule: RuleMetadata {
        name: "no-todo".to_string(),
        version: "1.0.0".to_string(),
        description: Some("Disallow TODO comments".to_string()),
        ..Default::default()
    },
    wasm: vec![extism_manifest::Wasm::Url {
        req: extism_manifest::HttpRequest {
            url: "https://example.com/rule.wasm".to_string(),
            ..Default::default()
        },
        meta: extism_manifest::WasmMetadata {
            hash: Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string()),
            ..Default::default()
        },
    }],
    ..Default::default()
};

// Convert to JSON
let json = serde_json::to_string_pretty(&manifest)?;
```

## Dependencies

| Crate | Purpose |
| ------------ | ---- |
| **`jsonschema`** | JSON Schema Draft-07 compliant validation |
| **`serde`** | Serialization/deserialization framework |
| **`serde_json`** | JSON parsing and manipulation |
| **`thiserror`** | Error type definitions |
| **`sha2`** | SHA256 hash calculation |
| **`hex`** | Hexadecimal encoding of hash values |

## Design Features

1. **Single Responsibility**: Focused solely on manifest type definitions and validation
2. **Schema-Centric**: Treats JSON Schema as the single source of truth
3. **Thread-safe Lazy Initialization**: Schema reuse via `OnceLock` for thread-safe one-time initialization
4. **Extensibility**: Allows arbitrary JSON Schema via the `options` field

## Integration with Other Crates

- **`tsuzulint_registry`**: Validates manifests during plugin download
- **`tsuzulint_plugin`**: Uses manifest information during rule loading
- **`tsuzulint_core`**: Uses for rule configuration validation

## Integrity Verification

The `integrity` module provides SHA256 hash computation and verification for WASM artifacts.

### HashVerifier

```rust
use tsuzulint_manifest::HashVerifier;

// Compute SHA256 hash
let wasm_bytes = include_bytes!("rule.wasm");
let hash = HashVerifier::compute(wasm_bytes);
// Returns: 64-character lowercase hex string

// Verify against expected hash (case-insensitive)
HashVerifier::verify(wasm_bytes, &hash)?;
```

### IntegrityError

```rust
pub enum IntegrityError {
    HashMismatch { expected: String, actual: String },
    InvalidFormat(String),
}
```

### Features

- SHA256 hash computation using `sha2` crate
- Case-insensitive hash comparison
- Format validation (64 hex characters)
- Used by both `tsuzulint_registry` and `tsuzulint_core` for consistent verification
