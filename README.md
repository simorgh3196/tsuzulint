# TsuzuLint

> **Status: pre-release (0.1.0 in progress).** The Japanese linting core is complete — full
> `textlint-rule-preset-ja-technical-writing` parity (26 built-in rules), morphology-driven style
> rules, and a `prh`-equivalent terminology rule. The VSCode extension and release packaging are
> the remaining 0.1.0 work. See [`docs/roadmap.md`](docs/roadmap.md) for milestones.

**TsuzuLint** is a high-performance natural-language linter written in Rust — a fast, embeddable
**Japanese `textlint` replacement** (Korean/Chinese planned). On the full `ja-technical-writing`
preset it runs roughly **15× faster** than Node `textlint` at a fraction of the memory
(see [benchmarks](docs/benchmarks.md)), and is **migration-friendly**: its rules deliberately mirror
textlint's names and it imports existing `prh` `.prh.yml` dictionaries
(see [migrating from textlint](docs/migration-from-textlint.md)).

- **Brand:** TsuzuLint. **Command / crates:** `tzlint` (`tzlint_*`). A short command name
  behind a longer brand is a common convention (e.g. Visual Studio Code → `code`).
- **Goals:** execution speed (index-based AST, zero-copy plugin reads, single-traversal
  scheduler), portability (native + `wasm32`, browser as a first-class design target),
  easy rule extension (Rust PDK now; layered ABI for future TS/AssemblyScript), safe
  breaking changes (frozen AST core + additive tables + `bytecheck`), strong tests & docs.

## Usage

[Install `tzlint`](docs/install.md) — from source today (`cargo install --git …` or
`cargo build --release` → `target/release/tzlint`); prebuilt binaries, an npm wrapper, and the
editor extension ship with 0.1.0. Then:

```sh
# Lint files, directories (recursed for Markdown), or globs; `-` reads stdin.
tzlint lint README.md docs/ 'src/**/*.md'
cat draft.md | tzlint lint -

# Pick an output format (text | json | sarif).
tzlint lint --format json docs/
tzlint lint --format sarif docs/ > results.sarif   # e.g. GitHub code scanning

# Apply autofixes in place (preview with --dry-run); `fix -` filters stdin to stdout.
tzlint fix docs/
tzlint fix --dry-run docs/
cat draft.md | tzlint fix - > fixed.md

# Write a starter .tzlintrc.json in the working directory.
tzlint init

# Inspect the resolved rule set (honors --config / discovery).
tzlint rules list
tzlint rules explain max-ten
```

A directory argument is searched recursively for `.md`/`.markdown` files (hidden entries and
symlinks are skipped); globs (`*`, `?`, `[...]`, `**`) match exactly, so quote them to keep the
shell from expanding them first.

Global options: `-c/--config <PATH>` (use a specific config instead of upward discovery),
`-v/--verbose` (extra notes to stderr), `--no-cache` (skip the document cache).

`lint` exits `0` when clean, `1` when it reports one or more diagnostics, and `2` on an
operational error (bad config, unreadable file, …) — the conventional codes for CI. The text
format is `path:line:col: severity: message [rule]`, where `col` is the diagnostic's 1-based
start column:

```text
$ printf 'これはﾊﾛｰという文です。\n' | tzlint lint -
<stdin>:1:4: warning: 半角カタカナは推奨されません。全角カタカナを使ってください。 [no-hankaku-kana]
1 file(s) checked, 1 issue(s) found
```

See [`docs/install.md`](docs/install.md) for install methods,
[`docs/config-reference.md`](docs/config-reference.md) for configuration (and the full rule list),
[`docs/migration-from-textlint.md`](docs/migration-from-textlint.md) for switching from textlint,
[`docs/json-output.md`](docs/json-output.md) for the `--format json` contract,
[`docs/processors.md`](docs/processors.md) for CSV/TSV column linting, and
[`docs/benchmarks.md`](docs/benchmarks.md) for the textlint performance comparison.

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
