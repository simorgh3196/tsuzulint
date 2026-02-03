# WASM Rule Interface Specification

This document defines the language-agnostic interface that all TsuzuLint WASM rules must implement.

## Overview

TsuzuLint rules are compiled to WebAssembly (WASM) and run in a sandboxed environment. This specification enables rule development in any language that compiles to WASM (Rust, AssemblyScript, Go, etc.).

## Target

Rules must compile to `wasm32-wasip1` (WASI Preview 1).

## Required Exports

Every rule WASM module must export these two functions:

### `get_manifest`

Returns metadata about the rule.

```text
Signature: () -> i32 (pointer to JSON string)
```

**Response**: JSON string matching [RuleManifest schema](#rulemanifest)

### `lint`

Performs linting on AST nodes. Nodes are passed as a batch (array) for efficiency.

```text
Signature: (input_ptr: i32, input_len: i32) -> i32 (pointer to JSON string)
```

**Input**: JSON string matching [LintRequest schema](#lintrequest)
**Response**: JSON string matching [LintResponse schema](#lintresponse)

## Memory Management

### For Extism-based Runtimes (Recommended)

When using Extism PDK, memory management is handled automatically:

- **Rust**: Use `extism-pdk` crate with `#[plugin_fn]` macro
- **AssemblyScript**: Use `@aspect/as-pdk` package
- **Go**: Use `github.com/extism/go-pdk` package

### For Custom Runtimes

If implementing without Extism PDK, export these functions:

```text
alloc(size: i32) -> i32    // Allocate memory, return pointer
dealloc(ptr: i32, size: i32)  // Free memory (optional)
```

## Data Schemas

### RuleManifest

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "type": "object",
  "required": ["name", "version"],
  "properties": {
    "name": {
      "type": "string",
      "description": "Unique rule identifier (e.g., 'no-todo')",
      "pattern": "^[a-z][a-z0-9-]*$"
    },
    "version": {
      "type": "string",
      "description": "Semantic version (e.g., '1.0.0')",
      "pattern": "^\\d+\\.\\d+\\.\\d+(-[a-zA-Z0-9.]+)?$"
    },
    "description": {
      "type": "string",
      "description": "Human-readable description"
    },
    "fixable": {
      "type": "boolean",
      "default": false,
      "description": "Whether this rule provides auto-fixes"
    },
    "node_types": {
      "type": "array",
      "items": { "type": "string" },
      "default": [],
      "description": "Node types to receive (empty = all nodes)"
    },
    "cache_scope": {
      "type": "string",
      "enum": ["node", "node_type", "document"],
      "default": "node",
      "description": "Cache granularity for incremental linting (not yet implemented)"
    },
    "exclude_contexts": {
      "type": "array",
      "items": { "type": "string" },
      "default": [],
      "description": "Parent node types to exclude (e.g., ['CodeBlock'] to skip code blocks) (not yet implemented)"
    },
    "schema": {
      "type": "object",
      "description": "JSON Schema for rule configuration options"
    }
  }
}
```

#### cache_scope Values

> **Note**: `cache_scope` and `exclude_contexts` are not yet implemented. They are reserved for future incremental linting optimization.

| Value | Description | Use Case |
| :--- | :--- | :--- |
| `"node"` | Each node can be cached independently | Rules that only look at individual nodes (e.g., `sentence-length`) |
| `"node_type"` | All nodes of the same type are re-linted together | Rules that compare nodes of the same type (e.g., `no-duplicate-headers`) |
| `"document"` | Entire document is re-linted on any change | Rules that need full document context (e.g., `consistent-terminology`) |

### LintRequest

All matching nodes are passed as a batch (array) in a single `lint` call. This design:
- Reduces WASM call overhead
- Enables efficient caching (source is passed only once)
- Simplifies stateful rules (all nodes available at once)

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "type": "object",
  "required": ["nodes", "config", "source"],
  "properties": {
    "nodes": {
      "type": "array",
      "description": "Array of AST nodes matching node_types",
      "items": {
        "type": "object",
        "properties": {
          "type": { "type": "string" },
          "range": {
            "type": "array",
            "items": { "type": "integer" },
            "minItems": 2,
            "maxItems": 2,
            "description": "[start, end] byte offsets"
          },
          "children": {
            "type": "array",
            "items": { "$ref": "#/properties/nodes/items" }
          }
        }
      }
    },
    "config": {
      "type": "object",
      "description": "Rule-specific configuration from .tsuzulint.json"
    },
    "source": {
      "type": "string",
      "description": "Full source text of the file"
    },
    "file_path": {
      "type": ["string", "null"],
      "description": "File path (if available)"
    }
  }
}
```

### LintResponse

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "type": "object",
  "required": ["diagnostics"],
  "properties": {
    "diagnostics": {
      "type": "array",
      "items": { "$ref": "#/$defs/Diagnostic" }
    }
  },
  "$defs": {
    "Diagnostic": {
      "type": "object",
      "required": ["rule_id", "message", "span"],
      "properties": {
        "rule_id": {
          "type": "string",
          "description": "Rule that generated this diagnostic"
        },
        "message": {
          "type": "string",
          "description": "Human-readable message"
        },
        "span": { "$ref": "#/$defs/Span" },
        "severity": {
          "type": "string",
          "enum": ["error", "warning", "info"],
          "default": "error"
        },
        "fix": { "$ref": "#/$defs/Fix" }
      }
    },
    "Span": {
      "type": "object",
      "required": ["start", "end"],
      "properties": {
        "start": { "type": "integer", "minimum": 0 },
        "end": { "type": "integer", "minimum": 0 }
      },
      "description": "Byte range [start, end)"
    },
    "Fix": {
      "type": "object",
      "required": ["span", "text"],
      "properties": {
        "span": { "$ref": "#/$defs/Span" },
        "text": {
          "type": "string",
          "description": "Replacement text (empty for deletion)"
        }
      }
    }
  }
}
```

## AST Node Types

Rules receive AST nodes as a batch based on their `node_types` manifest field.

### Block Elements

| Type | Description | Has Children |
| :--- | :--- | :--- |
| `Document` | Root node | Yes |
| `Paragraph` | Text paragraph | Yes |
| `Header` | Heading (h1-h6) | Yes |
| `BlockQuote` | Quote block | Yes |
| `List` | Ordered/unordered list | Yes |
| `ListItem` | List item | Yes |
| `CodeBlock` | Fenced code block | No |
| `HorizontalRule` | Thematic break | No |
| `Html` | Raw HTML block | No |
| `Table` | Table | Yes |
| `TableRow` | Table row | Yes |
| `TableCell` | Table cell | Yes |

### Inline Elements

| Type | Description | Has Children |
| :--- | :--- | :--- |
| `Str` | Plain text | No |
| `Break` | Line break | No |
| `Emphasis` | Italic text | Yes |
| `Strong` | Bold text | Yes |
| `Delete` | Strikethrough | Yes |
| `Code` | Inline code | No |
| `Link` | Hyperlink | Yes |
| `Image` | Image | No |
| `LinkReference` | Reference link | Yes |
| `ImageReference` | Reference image | No |
| `FootnoteReference` | Footnote ref | No |

## Example Implementations

### Rust (Extism PDK)

```rust
use extism_pdk::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct Manifest {
    name: &'static str,
    version: &'static str,
    description: &'static str,
    fixable: bool,
    node_types: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_scope: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exclude_contexts: Option<Vec<&'static str>>,
}

#[derive(Deserialize)]
struct LintRequest {
    nodes: Vec<Node>,
    config: serde_json::Value,
    source: String,
    file_path: Option<String>,
}

#[derive(Deserialize)]
struct Node {
    #[serde(rename = "type")]
    node_type: String,
    range: [u32; 2],
    #[serde(default)]
    children: Vec<Node>,
}

#[derive(Serialize)]
struct LintResponse {
    diagnostics: Vec<Diagnostic>,
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
pub fn get_manifest() -> FnResult<String> {
    let manifest = Manifest {
        name: "my-rule",
        version: "1.0.0",
        description: "My custom rule",
        fixable: false,
        node_types: vec!["Str"],
        cache_scope: Some("node"),           // Optional: "node", "node_type", or "document"
        exclude_contexts: Some(vec!["CodeBlock"]), // Optional: skip nodes inside CodeBlock
    };
    Ok(serde_json::to_string(&manifest)?)
}

#[plugin_fn]
pub fn lint(input: String) -> FnResult<String> {
    let request: LintRequest = serde_json::from_str(&input)?;
    let mut diagnostics: Vec<Diagnostic> = vec![];

    // All matching nodes are passed as a batch
    for node in &request.nodes {
        let text = &request.source[node.range[0] as usize..node.range[1] as usize];
        // Your lint logic here
    }

    Ok(serde_json::to_string(&LintResponse { diagnostics })?)
}
```

### AssemblyScript (Extism PDK)

```typescript
import { JSON } from "json-as";
import { Host, Output } from "@aspect/as-pdk";

@json
class Manifest {
  name: string = "my-rule";
  version: string = "1.0.0";
  description: string = "My custom rule";
  fixable: boolean = false;
  node_types: string[] = ["Str"];
  cache_scope: string = "node";           // Optional: "node", "node_type", or "document"
  exclude_contexts: string[] = ["CodeBlock"]; // Optional: skip nodes inside CodeBlock
}

@json
class Node {
  type: string = "";
  range: u32[] = [];
  children: Node[] = [];
}

@json
class LintRequest {
  nodes: Node[] = [];
  config: string = "";  // JSON string
  source: string = "";
  file_path: string = "";
}

@json
class Span {
  start: u32 = 0;
  end: u32 = 0;
}

@json
class Diagnostic {
  rule_id: string = "";
  message: string = "";
  span: Span = new Span();
  severity: string = "error";
}

@json
class LintResponse {
  diagnostics: Diagnostic[] = [];
}

export function get_manifest(): i32 {
  const manifest = new Manifest();
  Output.setString(JSON.stringify(manifest));
  return 0;
}

export function lint(): i32 {
  const input = Host.inputString();
  const request = JSON.parse<LintRequest>(input);
  const response = new LintResponse();

  // All matching nodes are passed as a batch
  for (let i = 0; i < request.nodes.length; i++) {
    const node = request.nodes[i];
    const text = request.source.slice(node.range[0], node.range[1]);
    // Your lint logic here
  }

  Output.setString(JSON.stringify(response));
  return 0;
}
```

## Security Considerations

- Rules run in a WASI sandbox with no filesystem or network access
- Rules cannot access host memory outside allocated regions
- Rules have execution time limits (configurable)
- All communication uses JSON serialization (no shared memory)

## Versioning

This specification follows semantic versioning. Breaking changes increment the major version.

| Spec Version | Changes |
| :--- | :--- |
| 1.0.0 | Initial specification |
