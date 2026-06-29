# TsuzuLint

**TsuzuLint** is a fast, embeddable natural-language linter for Japanese prose, written in
Rust — a `textlint`-style writing checker for Markdown (and CSV/TSV), with morphology-aware
Japanese rules that typography-only tools cannot do.

> **0.1.0 — first release.** Usable today for Japanese technical writing. The project is early
> (`0.x`): the public API and rule behavior may still change between minor releases. See
> [`CHANGELOG.md`](CHANGELOG.md) for what shipped and [`docs/roadmap.md`](docs/roadmap.md) for
> what is next (Korean/Chinese, plugin ABI, browser, LSP).

- **Brand:** TsuzuLint. **Command / crates:** `tzlint` (`tzlint_*`). A short command behind a
  longer brand is a common convention (e.g. Visual Studio Code → `code`).
- **Why:** speed (index-based AST, zero-copy plugin reads, single-traversal scheduler),
  portability (native + `wasm32`, browser as a first-class target), easy rule extension
  (Rust PDK now; layered ABI for future TS/AssemblyScript), and safe evolution (frozen AST
  core + additive tables + `bytecheck`).

## Install

TsuzuLint is built from source (no published binary yet). With a recent Rust toolchain
(MSRV **1.94**):

```sh
# Option A — build in place; the binary lands at target/release/tzlint.
git clone https://github.com/simorgh3196/tsuzulint && cd tsuzulint
cargo build --release

# Option B — install `tzlint` onto your PATH (~/.cargo/bin).
cargo install --path crates/tzlint_cli
```

## Quick start

```sh
# 1. Write a starter .tzlintrc.json in the working directory.
tzlint init

# 2. Opt into the Japanese technical-writing preset by editing .tzlintrc.json:
#      { "extends": "ja-technical-writing" }

# 3. Lint your Markdown.
tzlint lint README.md docs/
```

```text
$ printf 'これはﾊﾛｰという文です。\n' | tzlint lint -
<stdin>:1:4: warning: 半角カタカナは推奨されません。全角カタカナを使ってください。 [no-hankaku-kana]
1 file(s) checked, 1 issue(s) found
```

Two presets ship, selected with the config `extends` key (see
[`docs/config-reference.md`](docs/config-reference.md)):

- **`ja-basic`** — the surface Japanese rules, no dictionary required.
- **`ja-technical-writing`** — `ja-basic` plus length/読点 thresholds and the morphology-backed
  style rules, mirroring `textlint-rule-preset-ja-technical-writing`.

## What it checks

0.1.0 ships **17 built-in rules**. Eleven are *surface* rules that work out of the box; six are
*morphology-backed* and stay inert until a Japanese dictionary is configured (see
[Morphology](#morphology)).

**Surface rules** (no dictionary needed):

| Rule | Checks |
|------|--------|
| `sentence-length` | Sentences longer than a character limit |
| `max-ten` | Too many 読点 (`、`) in one sentence |
| `max-kanji-continuous-len` | Runs of consecutive kanji over a limit |
| `no-hankaku-kana` | Half-width katakana (prefer full-width) |
| `no-mixed-zenkaku-hankaku-alphabet` | Mixing half-/full-width Latin letters |
| `no-nfd` | Decomposed (NFD) combining marks |
| `no-zero-width-spaces` | Invisible / zero-width code points |
| `no-exclamation-question-mark` | `!` / `?` (half- or full-width) in technical prose |
| `ja-no-mixed-period` | Mixing `。` and ASCII `.` sentence terminators |
| `no-todo` | Leftover task markers (TODO/FIXME/XXX/HACK) |
| `ja-prh` | Terminology / 表記ゆれ, with autofix (the `prh` counterpart) |

**Morphology-backed rules** (need a Japanese dictionary; bundled in `ja-technical-writing`):

| Rule | Checks |
|------|--------|
| `no-doubled-joshi` | The same 助詞 repeated within one sentence |
| `no-mix-dearu-desumasu` | Mixing である / ですます sentence styles in a document |
| `no-doubled-conjunctive-particle-ga` | The 逆接の接続助詞「が」 used more than once in a sentence |
| `ja-no-redundant-expression` | Redundant 「〜することができる」-family phrasing |
| `no-dropping-the-ra` | ら抜き言葉 (e.g. 見れる → 見られる) |
| `no-double-negative-ja` | Rhetorical 二重否定 (ないことはない / なくはない) |

`tzlint rules list` prints the resolved set for your config; `tzlint rules explain <id>`
describes one.

## Usage

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
start column.

See [`docs/config-reference.md`](docs/config-reference.md) for configuration,
[`docs/json-output.md`](docs/json-output.md) for the `--format json` contract, and
[`docs/processors.md`](docs/processors.md) for CSV/TSV column linting.

## Morphology

The six morphology-backed rules tokenize Japanese text, so they need a dictionary. They are
*false-negative-safe*: with no dictionary configured they simply report nothing (never a false
positive), so `ja-technical-writing` is safe to enable before you provision one. To turn them
on, add a top-level `morphology` section pointing at a hash-pinned dictionary container:

```jsonc
{
  "extends": "ja-technical-writing",
  "morphology": {
    "path": "dict/ipadic.dict.zst",   // or "url": "https://…/ipadic.dict.zst"
    "pin": "…64-hex BLAKE3 over the compressed container…",
    "lang": "ja"
  }
}
```

The container is verified against `pin` before use and folded into the cache key. There is no
official hosted dictionary yet, so you build one from lindera's embedded IPADIC — see
[`docs/morphology.md`](docs/morphology.md) for how to pack and pin it, and
[`docs/config-reference.md`](docs/config-reference.md) for the full config surface.

## Coming from textlint

TsuzuLint targets the Japanese textlint workflow: the `ja-technical-writing` preset mirrors
`textlint-rule-preset-ja-technical-writing`, and existing [`prh`](https://github.com/prh/prh)
`.prh.yml` term dictionaries drop in directly via `rules.ja-prh.options.dictionaries` (literal
and `/source/flags` regex patterns, with `$1`-style replacement templates). See
[`docs/migration-from-textlint.md`](docs/migration-from-textlint.md).

## Embedding

TsuzuLint is designed to embed. `tzlint_wasm` exposes a `TsuzuLint` binding (`lint`, plus
`registerDictionary` for host-supplied morphology), shipped as lean (no tokenizer) and full
artifacts. See [`docs/embedding.md`](docs/embedding.md).

## Workspace layout

```
crates/
  tzlint_ast    frozen ABI types (index-based AST, Span)
  tzlint_core   parser + lint engine + config + cache + io
  tzlint_rules  built-in native rules
  tzlint_pdk    rule-author SDK
  tzlint_abi    shared-memory plugin ABI
  tzlint_cli    the `tzlint` binary
  tzlint_lsp    LSP server (scaffold)
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

Documentation lives under [`docs/`](docs/) and is written in English. Contributions: see
[`docs/CONTRIBUTING.md`](docs/CONTRIBUTING.md) and [`AGENTS.md`](AGENTS.md).
