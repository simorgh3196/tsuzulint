# TsuzuLint

> **Status: research / WIP — clean redesign in progress.** This repository is the
> ground-up redesign of TsuzuLint. See [`docs/roadmap.md`](docs/roadmap.md) for milestones.

**TsuzuLint** is a high-performance natural-language linter written in Rust, inspired by
textlint and specialized for CJK (Japanese first; Korean/Chinese planned).

- **Brand:** TsuzuLint. **Command / crates:** `tzlint` (`tzlint_*`). A short command name
  behind a longer brand is a common convention (e.g. Visual Studio Code → `code`).
- **Goals:** execution speed (index-based AST, zero-copy plugin reads, single-traversal
  scheduler), portability (native + `wasm32`, browser as a first-class design target),
  easy rule extension (Rust PDK now; layered ABI for future TS/AssemblyScript), safe
  breaking changes (frozen AST core + additive tables + `bytecheck`), strong tests & docs.

## Workspace layout

```
crates/
  tzlint_ast    frozen ABI types (index-based AST, Span)
  tzlint_core   parser + lint engine + config + cache + io
  tzlint_rules  built-in native rules
  tzlint_pdk    rule-author SDK
  tzlint_abi    shared-memory plugin ABI
  tzlint_cli    the `tzlint` binary
  tzlint_lsp    LSP server (scaffold in v1)
```

## Build

```sh
cargo build          # or: just build
cargo test           # or: just test
just check           # rustfmt + clippy + tests (CI-equivalent)
```

MSRV: Rust **1.94** — rolling policy: **latest stable − 2** (tracking wasmtime's "last 3
stable releases"); bumped each Rust release. Development uses the latest stable.
License: **Apache-2.0**.

Documentation lives under [`docs/`](docs/) and is written in English.
