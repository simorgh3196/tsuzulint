# {{RULE_NAME}}

> {{RULE_DESCRIPTION}}

A Texide rule written in TypeScript and compiled to WASM using [Javy](https://github.com/bytecodealliance/javy).

## Prerequisites

- Node.js 18+
- [Javy CLI](https://github.com/bytecodealliance/javy)

### Installing Javy

```bash
# macOS with Homebrew
brew install aspect-cli/tap/javy

# Or download from GitHub releases
# https://github.com/bytecodealliance/javy/releases
```

## Development

### Install dependencies

```bash
npm install
```

### Build WASM

```bash
# Using the build script
./build.sh {{RULE_NAME}}

# Or manually
npm run build
```

### Test (before WASM compilation)

```bash
npm test
```

## Configuration

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `exampleOption` | string | `"default"` | Example configuration option |

## Example `.texide.json`

```json
{
  "rules": {
    "{{RULE_NAME}}": {
      "exampleOption": "custom_value"
    }
  },
  "plugins": [
    "./{{RULE_NAME}}.wasm"
  ]
}
```

## Project Structure

```
{{RULE_NAME}}/
├── package.json      # Node.js project config
├── tsconfig.json     # TypeScript compiler config
├── build.sh          # Build script
├── src/
│   └── index.ts      # Rule implementation
└── dist/             # Compiled JavaScript (generated)
```

## How It Works

1. TypeScript is compiled to JavaScript using `tsc`
2. JavaScript is compiled to WASM using Javy
3. Texide loads the WASM and calls `main()` for each lint request

## Limitations

- Javy compiles the entire QuickJS runtime into WASM (~1MB overhead)
- No access to Node.js APIs (runs in sandboxed QuickJS environment)
- Performance is slower than native Rust rules

For production rules with high performance requirements, consider using the [Rust template](../rust/).
