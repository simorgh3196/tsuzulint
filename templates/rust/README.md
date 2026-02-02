# Rust Rule Template

This is a template for creating Texide rules in Rust.

## Setup

### 1. Copy the Template

```bash
# From the rules/ directory
cp -r ../templates/rust my-rule
cd my-rule
```

### 2. Replace Placeholders

Replace these placeholders in all files:

| Placeholder | Replace With |
| :--- | :--- |
| `{{RULE_NAME}}` | Your rule ID (e.g., `no-todo`, `max-length`) |
| `{{RULE_DESCRIPTION}}` | Short description of what the rule checks |

```bash
# Example using sed (macOS)
sed -i '' 's/{{RULE_NAME}}/my-rule/g' Cargo.toml src/lib.rs
sed -i '' 's/{{RULE_DESCRIPTION}}/Check for my pattern/g' Cargo.toml src/lib.rs

# Example using sed (Linux)
sed -i 's/{{RULE_NAME}}/my-rule/g' Cargo.toml src/lib.rs
sed -i 's/{{RULE_DESCRIPTION}}/Check for my pattern/g' Cargo.toml src/lib.rs
```

### 3. Update Cargo.toml

If creating a rule outside the rules workspace:

```toml
[dependencies]
texide-rule-pdk = { git = "https://github.com/simorgh3196/texide", branch = "main" }
```

### 4. Implement Your Logic

Edit `src/lib.rs`:

1. Update `Config` struct with your configuration options
2. Implement lint logic in the `lint` function
3. Add tests

## Build

```bash
# Install target (one-time)
rustup target add wasm32-wasip1

# Build
cargo build --target wasm32-wasip1 --release

# Output: target/wasm32-wasip1/release/texide_rule_my_rule.wasm
```

## Test

```bash
# Run unit tests
cargo test

# Test with Texide
texide lint --plugin ./target/wasm32-wasip1/release/texide_rule_my_rule.wasm test.md
```

## Files

```text
my-rule/
├── Cargo.toml      # Package manifest
├── README.md       # This file
└── src/
    └── lib.rs      # Rule implementation
```

## Resources

- [Rule Development Guide](../../docs/rule-development.md)
- [WASM Interface Specification](../../docs/wasm-interface.md)
- [JSON Schema](../../schemas/rule-types.json)
- [Sample Rules](../../rules/)
