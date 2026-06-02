---
name: tzlint-processors
description: How to add an input-format processor (e.g. CSV, Re:VIEW) to TsuzuLint. Read this before adding support for a new file type or changing the processor seam.
---

# Adding an input-format processor

TsuzuLint lints natural-language text in any format via a **processor seam**. A `Processor`
turns source into either lintable **regions** (the easy path) or a full **AST** (the escape
hatch for structure-aware rules). The core handles parsing, span-rebasing, per-region rule
resolution, caching, and fix. See `docs/design/input-format-processors.md` for the full contract.

## Decide: Regions or Ast?

- **Return `Parsed::Regions`** (preferred) when you only need to lint the format's prose. You
  return the **byte ranges** of the natural-language text plus a `RegionTag` and a `ParseMode`.
  The core does the rest. This is the lowest-friction path — no AST construction, no span math.
- **Return `Parsed::Ast`** only when the format's own structure must be visible to rules
  (headings, lists, code blocks of the source format). You build the frozen `Ast` yourself
  (text = whole source, absolute spans). Markdown uses this path.

## Steps

1. **Create the module** `crates/tzlint_core/src/processor/<format>.rs`. Implement
   `Processor`: `extensions()` (dot-less, lowercase) and `parse(source, cfg) -> Result<Parsed, ParseError>`.
   - For prose extraction, build `Region { slices, tag, parse_mode }`. Each `slice` must be a
     **contiguous** byte range of `source`; the core parses it independently and rebases spans.
   - Use `RegionTag { kind: Some("<your-kind>"), index, name }` so config can target rules at
     regions. Never invent a CSV-specific concept in shared code — `kind` is your namespace.
   - Never `unwrap`/`expect`/`panic!`/`unreachable!` (clippy-denied). Slice with
     `source.get(a..b).unwrap_or("")`.
2. **Register it** in `Registry::with_builtins` (`processor/mod.rs`) — the single wiring point.
   Add a guard-test extension entry (Task 4.3) so the registry list and the test stay in sync.
3. **Config (if needed):** if your format needs options (delimiter, columns, …), add a resolved
   shape under `crates/tzlint_core/src/config/` and a `RawFormat`-style serde model in
   `config/model.rs`, then build a `ProcessorConfig` for it in `crates/tzlint_cli/src/rules.rs`
   (`processor_config_for`). Per-region rules flow through `region_rules_for` automatically when
   your regions carry a `kind`/`name`/`index` the config can match.
4. **Tests (TDD):** a unit test for your extraction (assert absolute spans of the regions), and
   an `app.rs` integration test that lints a fixture through the CLI.
5. **Docs:** add a short section to `docs/processors.md` describing the format's config and any
   caveats.

## Invariants you must keep

- Spans are absolute byte offsets into the original source; `Ast.text` (for the Ast path) is the
  whole source.
- The frozen `AstCoreV1` is not extended — region identity lives in `RegionTag`, outside the AST.
- All file I/O stays in the CLI/`Host`; a processor only ever sees an in-memory `&str`.
- One dispatch path: everything goes through `lint_document`; don't add a parallel lint route.
