# tsuzulint_wasm

A crate that provides WebAssembly bindings for browsers, enabling the Rust-based linter to run in browser environments.

## Overview

**tsuzulint_wasm** is a crate that provides WebAssembly bindings for TsuzuLint in browsers. It enables the feature-rich text linter implemented in Rust to run in browser environments.

**Position in the project:**

- Located at the top layer of the TsuzuLint architecture
- Provides a separate build for browser/Node.js environments, distinct from the CLI (native) version
- Structured to be publishable as an npm package

## Architecture

```text
┌─────────────────────────────────────────────────────────────┐
│                      TextLinter (WASM)                      │
│  ┌─────────────────────────────────────────────────────┐    │
│  │  wasm-bindgen exports                               │    │
│  │  - new()                                            │    │
│  │  - loadRule(wasm_bytes)                             │    │
│  │  - configureRule(name, config)                      │    │
│  │  - lint(content, file_type)                         │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│                   tsuzulint_plugin (browser)                │
│                   WasmiExecutor (WASM-in-WASM)              │
└─────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│                   tsuzulint_parser / tsuzulint_text         │
└─────────────────────────────────────────────────────────────┘
```

## Build

### Build Targets

`build.sh` supports three types of targets:

- `web`: Direct browser usage (ES Modules)
- `nodejs`: Node.js environment
- `bundler`: For bundlers like Webpack/Vite

```bash
# For browsers
./build.sh web

# For Node.js
./build.sh nodejs

# For bundlers
./build.sh bundler
```

### Crate Type

```toml
[lib]
crate-type = ["cdylib", "rlib"]
```

- `cdylib`: Exported as a WebAssembly binary
- `rlib`: Can also be built as a regular library for Rust testing

## API

### Constructor

```javascript
const linter = new TextLinter();
```

### Methods

| Rust Method | JS Name | Description |
| ------------ | ------ | ------ |
| `load_rule(&mut self, wasm_bytes: &[u8])` | `loadRule` | Load a WASM rule from byte array |
| `configure_rule(&mut self, name: &str, config_json: JsValue)` | `configureRule` | Configure a rule |
| `loaded_rules(&self)` | `getLoadedRules` | Get list of loaded rules |
| `lint(&mut self, content: &str, file_type: &str)` | `lint` | Run linting (returns JS object) |
| `lint_json(&mut self, content: &str, file_type: &str)` | `lintJson` | Run linting (returns JSON string) |

## Usage Examples

### Browser

```html
<!DOCTYPE html>
<html>
<head>
  <script type="module">
    import init, { TextLinter } from './pkg/tsuzulint_wasm.js';

    async function main() {
      await init();

      const linter = new TextLinter();

      // Load a rule (fetch WASM bytes)
      const ruleResponse = await fetch('./rules/no-todo.wasm');
      const ruleBytes = await ruleResponse.arrayBuffer();
      linter.loadRule(new Uint8Array(ruleBytes));

      // Run linting
      const content = '# Hello\n\nThis is a TODO item.';
      const diagnostics = linter.lint(content, 'markdown');

      console.log(diagnostics);
      // [
      //   {
      //     ruleId: "no-todo",
      //     message: "Found TODO keyword",
      //     start: 19,
      //     end: 23,
      //     severity: "warning",
      //     fix: { start: 19, end: 23, text: "DONE" }
      //   }
      // ]
    }

    main();
  </script>
</head>
<body>
  <textarea id="editor"></textarea>
  <div id="diagnostics"></div>
</body>
</html>
```

### Node.js

```javascript
const { TextLinter } = require('./pkg/tsuzulint_wasm.js');
const fs = require('fs');

const linter = new TextLinter();

// Load a rule from a local file
const ruleBytes = fs.readFileSync('./rules/no-todo.wasm');
linter.loadRule(ruleBytes);

// Run linting
const content = '# Hello\n\nThis is a TODO item.';
const diagnostics = linter.lint(content, 'markdown');

console.log(JSON.stringify(diagnostics, null, 2));
```

## Diagnostic Format

### JsDiagnostic

```typescript
interface JsDiagnostic {
  ruleId: string;
  message: string;
  start: number;
  end: number;
  startLine?: number;
  startColumn?: number;
  endLine?: number;
  endColumn?: number;
  severity: "error" | "warning" | "info";
  fix?: {
    start: number;
    end: number;
    text: string;
  };
}
```

## Internal Pipeline

```text
1. Input
   ├── Arguments: content: &str, file_type: &str
   
2. Parser Selection
   ├── "markdown" | "md" → MarkdownParser
   └── Other → PlainTextParser
   
3. Parsing
   ├── Build AST with AstArena
   ├── Convert to JSON
   └── prepare_text_analysis():
       ├── Tokenize with Tokenizer
       └── Split sentences with SentenceSplitter
   
4. Rule Execution
   └── host.run_all_rules_with_parts()
       └── Execute each WASM rule with wasmi
   
5. Output Conversion
   ├── Diagnostic → JsDiagnostic
   └── serde_wasm_bindgen::to_value()
```

## WASM-in-WASM Design

In browser environments, the `browser` feature of `tsuzulint_plugin` is used, executing rules with wasmi (a pure Rust WASM interpreter):

- The native Extism/wasmtime does not work in browser environments
- wasmi can be compiled to WASM itself → **WASM-in-WASM** execution is possible

## Dependencies

| Crate | Purpose |
| ------------ | ------ |
| `tsuzulint_ast` | AST data structures |
| `tsuzulint_parser` | Markdown/PlainText parsers |
| `tsuzulint_plugin` (`browser` feature) | WASM rule execution engine |
| `tsuzulint_text` | Tokenizer and sentence splitting |
| `wasm-bindgen` | FFI bindings between Rust and JavaScript |
| `serde-wasm-bindgen` | Convert Serde types to JsValue |
| `serde / serde_json` | Serialization |
| `js-sys` | Access to JavaScript standard objects |
| `console_error_panic_hook` | Output stack trace to console on panic |

## npm Package Structure

```json
{
  "name": "tsuzulint-wasm",
  "files": [
    "tsuzulint_wasm_bg.wasm",
    "tsuzulint_wasm_bg.wasm.d.ts",
    "tsuzulint_wasm.js",
    "tsuzulint_wasm.d.ts"
  ]
}
```

TypeScript type definitions (`.d.ts`) are auto-generated, enabling type-safe usage.

## Testing

```rust
#[wasm_bindgen_test]
fn test_lint_basic() {
    let mut linter = TextLinter::new().unwrap();
    // ... test logic
}
```

- `wasm-bindgen-test`: Run tests in WASM environment
- `build.rs`: Automatically builds simple_rule WASM fixtures for testing

## Limitations

- wasmi is an interpreter, so it is slower than the native version
- Processing large files may take longer
