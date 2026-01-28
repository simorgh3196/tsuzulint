# Migration Guide from textlint

This guide helps you migrate from the original textlint to Texide.

## Overview

Texide is a Rust reimplementation of textlint with:
- No Node.js dependency
- WASM-based rules
- Improved performance
- Compatible configuration format

## Configuration

### Similar Format

Texide uses a similar JSON configuration format, with `options` for rule configuration:

```json
{
  "rules": ["no-todo", "max-lines"],
  "options": {
    "no-todo": true,
    "max-lines": { "max": 300 }
  }
}
```

Supported config files:

| Format | Status |
|--------|--------|
| `.texide.jsonc` | Supported (default, supports comments) |
| `.texide.json` | Supported |

## Rules

### Key Difference

Original textlint uses JavaScript/TypeScript rules.
Texide uses WASM rules (compiled from Rust, Go, etc.).

### Migration Options

1. **Use official WASM ports** (when available)
2. **Rewrite in Rust** (recommended for custom rules)
3. **Use AssemblyScript** (easier for JS developers)

### Example: Migrating no-todo Rule

**Original (TypeScript):**

```typescript
import { TextlintRuleContext } from "@textlint/types";

export default function (context: TextlintRuleContext) {
  const { Syntax, RuleError, report, getSource } = context;
  return {
    [Syntax.Str](node) {
      const text = getSource(node);
      if (/TODO/.test(text)) {
        report(node, new RuleError("Found TODO"));
      }
    },
  };
}
```

**New (Rust):**

```rust
use extism_pdk::*;
use serde::{Deserialize, Serialize};

#[plugin_fn]
pub fn lint(input: String) -> FnResult<String> {
    let request: LintRequest = serde_json::from_str(&input)?;
    let mut diagnostics = Vec::new();

    if request.node.get("type").and_then(|t| t.as_str()) == Some("Str") {
        let range = request.node.get("range").unwrap();
        let start = range[0].as_u64().unwrap() as usize;
        let end = range[1].as_u64().unwrap() as usize;
        let text = &request.source[start..end];

        if text.contains("TODO") {
            diagnostics.push(Diagnostic {
                rule_id: "no-todo".to_string(),
                message: "Found TODO".to_string(),
                span: Span { start: start as u32, end: end as u32 },
                severity: "error".to_string(),
            });
        }
    }

    Ok(serde_json::to_string(&LintResponse { diagnostics })?)
}
```

## CLI Commands

| textlint | Texide | Notes |
|----------|--------|-------|
| `textlint file.md` | `texide lint file.md` | |
| `textlint --fix file.md` | `texide lint --fix file.md` | |
| `textlint --init` | `texide init` | |
| `textlint --format json` | `texide lint --format json` | |

## Package Management

### Before (npm)

```bash
npm install textlint textlint-rule-no-todo
```

### After (Texide)

```bash
# Install Texide binary
cargo install texide

# Add rules (WASM files)
texide add-rule ./rules/no-todo.wasm
```

## Editor Integration

### VS Code

- **textlint**: Uses vscode-textlint extension
- **Texide**: Use LSP (planned) or run CLI on save

### Neovim

```lua
-- Using null-ls or similar
null_ls.builtins.diagnostics.texide.with({
  command = "texide",
  args = { "lint", "--format", "json", "$FILENAME" },
})
```

## Performance Comparison

| Metric | textlint | Texide |
|--------|----------|--------|
| Startup time | ~500ms | ~10ms |
| Memory (100 files) | ~200MB | ~50MB |
| Parallel processing | Limited | Full rayon |
| Caching | Optional | Built-in |

*Note: Actual numbers depend on rules and files.*

## Limitations

### Not Yet Supported

- [ ] JavaScript rule execution
- [ ] Some textlint plugins
- [ ] `.textlintrc.js` config

### Different Behavior

- **Parser**: Uses markdown-rs (may have slight AST differences)
- **Rule API**: Different function signature (see Rule Development Guide)

## Getting Help

- [GitHub Issues](https://github.com/simorgh3196/texide/issues)
- [Discussions](https://github.com/simorgh3196/texide/discussions)

## Contributing

Help us improve migration by:
1. Reporting incompatibilities
2. Porting popular rules to WASM
3. Improving documentation
