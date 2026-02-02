# AssemblyScript Rule Template

This is a template for creating Texide rules in AssemblyScript.

> **Note**: AssemblyScript support is experimental. Rust is recommended for production rules.

## Prerequisites

- Node.js 18+
- npm or pnpm

## Setup

### 1. Copy the Template

```bash
cp -r templates/assemblyscript my-rule
cd my-rule
```

### 2. Replace Placeholders

Replace these placeholders in all files:

| Placeholder | Replace With |
| :--- | :--- |
| `{{RULE_NAME}}` | Your rule ID (e.g., `no-todo`) |
| `{{RULE_DESCRIPTION}}` | Short description |

### 3. Install Dependencies

```bash
npm install
```

### 4. Implement Your Logic

Edit `assembly/index.ts`:

1. Update `Config` class with your options
2. Implement lint logic in the `lint` function

## Build

```bash
# Production build
npm run build

# Debug build (with source maps)
npm run build:debug

# Output: build/release.wasm
```

## Test

```bash
# Run tests
npm test

# Test with Texide
texide lint --plugin ./build/release.wasm test.md
```

## Files

```text
my-rule/
├── assembly/
│   └── index.ts    # Rule implementation
├── asconfig.json   # AssemblyScript config
├── package.json    # npm package manifest
└── README.md       # This file
```

## Type Definitions

Types are defined inline in `assembly/index.ts`. For more complex rules, consider generating types from the JSON Schema:

```bash
# Generate TypeScript types (requires manual adaptation for AS)
npx quicktype ../schemas/rule-types.json \
  --src-lang schema \
  --lang typescript \
  --out assembly/types.ts
```

## Limitations

AssemblyScript has some limitations compared to Rust:

- No regex support (use string methods instead)
- Limited standard library
- Manual memory management in some cases

## Resources

- [AssemblyScript Documentation](https://www.assemblyscript.org/)
- [Extism AS PDK](https://github.com/aspect-build/as-pdk)
- [Rule Development Guide](../../docs/rule-development.md)
