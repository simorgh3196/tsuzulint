# Pluggable Input-Format Processors

> Design spec. Status: approved design, pending implementation plan.
> Scope: introduce a processor layer so TsuzuLint can lint natural-language text in
> formats beyond Markdown — starting with CSV/TSV (lint specific columns, with
> per-column rules), and built so new formats (e.g. Re:VIEW) are easy to add in-tree.

## 1. Summary

Today TsuzuLint parses every input as Markdown (`tzlint_core::parse`) and runs the
single-traversal engine over the resulting frozen AST. This design inserts a **processor
layer** in front of that step: a file's extension selects a `Processor`, the processor
locates the lintable natural-language regions of the source (or returns a full AST), and
the core parses each region, rebases its spans onto the original file, and lints each
region with the rule set configured for it.

The first concrete formats are **CSV and TSV**, where the user names the columns to lint
and may assign different rules per column. The processor abstraction is format-neutral so
adding a new format (Re:VIEW, AsciiDoc, plain text, …) is a small, well-bounded in-tree
contribution.

## 2. Goals and non-goals

**Goals**

- Lint specific **columns** of CSV/TSV, selectable by **header name** and by **1-based
  column number**.
- Apply **different rules per column** (a column-scoped overlay over the base rules).
- A format-neutral `Processor` abstraction; "column" is *not* a core concept.
- Make adding a new format a **low-friction in-tree change** (implement a trait, register
  one line, add tests/docs). This is the top-priority requirement.
- Preserve the frozen `AstCoreV1` ABI (no `Node` field changes), the single-traversal
  engine, `Host`-mediated I/O, and the single dispatch path (CLI/LSP parity).

**Non-goals (this milestone)**

- Runtime/loadable processors (WASM). Extension is by PR into the binary; the trait is
  designed so a future WASM processor ABI (M3) can layer on, but no WASM work is in scope.
- A config DSL for describing extraction declaratively.
- Structure-aware rules across non-Markdown formats beyond what the optional full-AST path
  already enables (see §5).

## 3. Background (current state)

- `tzlint_core::parse(source) -> Result<Ast, ParseError>` runs markdown-rs and flattens
  mdast into the frozen index-AST. `Ast.text` is the (BOM-stripped) source; every `Span`
  is an absolute byte offset into it.
- `Engine::lint(&ArchivedAst, &[&dyn Rule]) -> Vec<Diagnostic>` walks the tree once and
  invokes each rule on the node kinds it registered for; diagnostics are sorted into a
  stable total order.
- The CLI pipeline (`tzlint_cli::app`) is: `expand` (globs/dirs/stdin, **Markdown
  extensions only**) → read via `Host` → `lint_cached`/`lint_direct` (`parse` → archive →
  `access` → `Engine::lint`) → render. `fix` mirrors this. Config is resolved once and
  `resolve_rules(&config)` builds a flat `Vec<Box<dyn Rule>>` via `tzlint_rules::build_rule`.
- Config (`RawConfig`): kebab-case, `deny_unknown_fields`; keys `language`,
  `message-language`, `rules` (`false | true | { severity?, options? }`), `extends`
  (preset layering, precedence low→high `extends[0] < … < user`).

## 4. Architecture overview

```
file (path, bytes)
      │  ext → Registry selects a Processor (Markdown is the default/fallback)
      ▼
┌──────────────── Processor (format-specific; the extension point) ───────────────┐
│  parse(source, cfg) -> Parsed                                                    │
│    Parsed::Regions(Vec<Region>)  ← easy path: prose ranges + tag + parse_mode    │
│    Parsed::Ast(Ast)              ← full path: a complete AST (structure-aware)   │
└──────────────────────────────────────────────────────────────────────────────────┘
      │
      ├─ Regions: core parses each slice with its parse_mode, offsets spans by slice.start
      │           (Ast.text = whole original source; spans stay absolute)
      └─ Ast:     used as-is (Markdown takes this path; region = Whole)
      ▼
per-region rule resolution: base rules ⊕ overrides matching the region's tag
      ▼
Engine::lint(region_ast, rules_for_region)   ── once per region (independent)
      ▼
merge all diagnostics, sort by the existing stable order
      ▼
position mapper → (line, column) → existing renderers (text / JSON)
```

The frozen AST and the single-traversal engine are **unchanged**. Per-region linting is
just the existing engine applied independently to each region.

### Crate layout

- `tzlint_core::processor` (new): the `Processor` trait, `Parsed`, `Region`, `RegionTag`,
  `ParseMode`, and the built-in `Registry`.
- `tzlint_core::processor::markdown`: the current `parse.rs` logic, re-expressed as
  `MarkdownProcessor` returning `Parsed::Ast`. The existing `parse()` stays as a thin
  wrapper for compatibility.
- `tzlint_core::processor::delimited`: the CSV/TSV processor (parameterized by delimiter).
- `tzlint_core`: a new single entry point `lint_document(...)` that selects the processor,
  resolves per-region rules, lints each region, and merges diagnostics. CLI/LSP/cache/fix
  all go through it (dispatch parity).

## 5. The Processor abstraction

A single-method trait. The return type unifies the easy path (regions) and the optional
full path (AST). Types below are illustrative, not final signatures.

```rust
/// A format-specific parser. Adding a new format = implement this + register it.
pub trait Processor {
    /// Handled extensions, dot-less and lowercase, e.g. ["csv"], ["md", "markdown"].
    fn extensions(&self) -> &[&str];

    /// Parse `source`. Return Regions for the common (prose-extraction) path, or a full
    /// Ast when the format's own structure must be visible to rules.
    fn parse(&self, source: &str, cfg: &ProcessorConfig) -> Result<Parsed, ParseError>;
}

/// Parse result. Both paths' spans are ABSOLUTE byte offsets into the original `source`.
pub enum Parsed {
    /// Easy path: lintable regions; the core parses each slice and rebases spans.
    Regions(Vec<Region>),
    /// Full path: a complete AST (text = whole source, spans absolute). Escape hatch for
    /// structure-aware linting.
    Ast(Ast),
}

/// One lintable region: the unit that shares a rule set and a parse mode.
pub struct Region {
    /// Source slices making up this region (e.g. all cells of one column). Each slice is
    /// a CONTIGUOUS substring of `source` and is parsed independently as a mini-document.
    pub slices: Vec<Span>,
    /// What this region is, so config can target rules at it.
    pub tag: RegionTag,
    /// How to interpret each slice before linting.
    pub parse_mode: ParseMode,
}

/// Format-neutral region identity. "column" is just one `kind`, defined by the delimited
/// processor — it is NOT a core concept.
pub struct RegionTag {
    /// Region kind, a processor-defined &'static str. CSV/TSV use Some("column"); a
    /// future Re:VIEW processor might use Some("footnote")/Some("caption"); Markdown and
    /// plain text use None (single document → base rules only).
    pub kind: Option<&'static str>,
    /// 0-based ordinal within the kind (e.g. column index). None when not applicable.
    pub index: Option<u32>,
    /// A name (e.g. a column header). None when not applicable.
    pub name: Option<String>,
}

/// How a region's slices are parsed before linting.
pub enum ParseMode {
    /// Parse as Markdown (CommonMark + GFM), reusing all Markdown rules. Default for cells.
    Markdown,
    /// Treat each slice as one plain paragraph (no Markdown constructs). For prose in
    /// formats where `*`/`_`/`#` are literal (LaTeX, HTML text, …).
    PlainText,
}
```

`&'static str` for `kind` is sufficient because extension is compile-time (in-tree PRs),
not runtime; this matches the "extend by PR" model and avoids allocation.

### Coverage vs textlint

textlint plugins return a full TxtAST per format, so structure-aware rules work across
formats. The **easy path here covers the majority case** — natural-language linting of
prose text, which is what most textlint rules (Str-node rules) do — for *any* format. The
gap is rules that inspect the *source format's own structure*; those formats opt into the
**full path** (`Parsed::Ast`) on the same trait. We do not force every format to build an
AST upfront (YAGNI); we keep the door open.

## 6. CSV/TSV processor

### Parsing: a purpose-built scanner that yields content spans

The `csv` crate unescapes quoted fields and does not expose each field's byte range in the
*original* source, which we need for diagnostics. So we use a **small RFC 4180-style
scanner** that, for each cell, computes the **contiguous byte span of its content** in the
original `source`:

- Unquoted cell → the cell's bytes as-is.
- Quoted cell → the bytes **inside** the outer quotes (quotes are not linted). Embedded
  newlines and doubled quotes (`""`) remain in the contiguous slice as raw bytes.
- Records split on unquoted newlines (LF and CRLF); fields split on unquoted delimiters.

Because each `Region.slice` is a contiguous substring of `source`, the core can parse the
slice and add `slice.start` to every resulting span to get absolute offsets (§7).

### Known v1 simplifications (accepted, documented)

- **Escaped quotes**: a quoted cell containing `""` is linted with the raw `""` present
  (not the logical single `"`). Minor for prose linting. A future content-normalization +
  span back-map can fix this if needed (not in scope).
- **TSV quoting**: TSV is treated as tab-delimited CSV with the same quoting rules (matches
  real-world "TSV" files), not the strict IANA TSV escaping. A strict mode is future work.

### Column resolution (config → extraction set)

- `header: true`: the first record is read as header names. `columns` string keys resolve
  by header-name match; bare-integer keys resolve as 1-based positions (→ 0-based index).
  The header row itself is not linted.
- `header: false`: only integer keys are valid; a name key is a config error (§11).
- A configured column name absent from the actual header → a per-file note (§11), not a
  config error (it is data-dependent).

### Edge cases

- `Ast.text` = the whole original `source` (BOM preserved); the scanner skips a leading
  BOM. Offsets match the file the user sees.
- Ragged rows (missing target cell) → that slice is skipped. Extra fields are linted only
  when targeted by index.
- Empty / whitespace-only cells → not linted (nothing to parse).

## 7. Span remapping and position correctness

The key invariant that removes the need for a separate offset table: **`Ast.text` is the
whole original source, and every node span is an absolute offset into it.**

```
slice  = &source[start..end]                  // a cell's contiguous content
mini   = parse_mode.parse(slice)              // Markdown (existing parser) or PlainText
for n in mini.nodes: n.span += start          // shift every span by the slice start
→ collect these nodes into an Ast whose text = the whole original source
```

- Diagnostics' spans are therefore already absolute file offsets; `position.rs`'s
  `LineIndex`, built over the whole source, yields correct `(line, column)`.
- The Markdown full-AST path is byte-for-byte the current behavior (no shift; region =
  Whole).
- Region tags live in **side metadata** (outside the AST, keyed by region/node range),
  used only for rule selection — so there is **no `AstCoreV1` / ABI change**.

### Region granularity

Decision (locked): **lint each region independently — and within a region, each slice
(cell) is an independent mini-document.** A rule's `finish` (cross-node state) does not
carry across cells. This matches the "each cell is a small document" intuition and is the
natural fit since per-column rule sets already require splitting linting by region. A
future "one AST per column" optimization is possible but would change document-level rule
scope, so it is gated separately.

## 8. Region linting and per-region rule resolution

### Single dispatch entry

```
lint_document(ext, source, config, registry, rule_factory) -> Vec<Diagnostic>
  1. processor = registry.for_ext(ext)            // Markdown is the default/fallback
  2. parsed    = processor.parse(source, cfg_for(format))
  3. regions   = match parsed { Ast(a) => [(Whole, a)], Regions(rs) => build_region_asts(rs, source) }
  4. for each region: lint with the rule set resolved for its tag (Engine::lint)
  5. merge diagnostics; sort by the existing stable key
```

This is the **one dispatch function**; CLI, LSP, cache, and fix all call it
(`tzlint-dispatch-parity`).

### Resolving rules per region (a factory keeps core rule-agnostic)

`tzlint_core` must not depend on `tzlint_rules` (that would be a dependency cycle). So the
driver takes a **rule factory**:

```rust
rule_factory: &dyn Fn(&EffectiveRuleSettings) -> Vec<Box<dyn Rule>>
```

- Precompute the **distinct** effective settings once: the base (`config.rules`) and, per
  targeted column, `base ⊕ column.rules`. Build each rule set once via the factory and map
  `RegionTag → prebuilt rule set`. Every cell of a column reuses its column's set (no
  per-row rebuild).
- The CLI passes a closure wrapping the existing `build_rule`/`resolve_rules`. Core stays
  rule-agnostic; per-column rules are supported.

### Layering and opt-in semantics

- **Opt-in**: only columns listed in `columns` are linted. Unlisted columns (IDs, dates, …)
  are never linted — directly expressing "lint specific columns".
- **Layering**: a column's effective rules are `base ⊕ column.rules` (column wins),
  mirroring the existing preset/`extends` precedence. To drop a base rule for a column,
  set it `false` in that column.

## 9. Configuration model

New top-level keys (kebab-case, `deny_unknown_fields` extended to the new shapes):

- `formats`: a map keyed by format id (`csv`, `tsv`, …). Each value carries
  format-specific options. For delimited formats: `header` (bool), `delimiter` (optional
  override; csv=`,`, tsv=`\t`), and `columns`.
- `columns`: a map whose key is a header name (string) or a 1-based column number (bare
  integer string). Each value: optional `parse-mode` (`markdown` default | `plain`) and a
  `rules` overlay (same shape as top-level `rules`).
- `overrides` (general, format-neutral): a list of `{ files?: glob[], region: { kind?,
  index?, name? }, rules }`. `formats.<fmt>.columns` desugars to entries of this form; the
  general form is the extension point for other formats.

### Examples

Header CSV, two prose columns with different rules:

```yaml
language: ja
rules:
  no-hankaku-kana: true      # base: applies to every linted region unless overridden
formats:
  csv:
    header: true             # row 1 is a header → not linted
    columns:                 # only these columns are linted (opt-in)
      title:
        parse-mode: plain
        rules:
          max-ten: { options: { max: 1 } }
      body:
        rules:
          no-todo: true
          max-ten: { options: { max: 3 } }
```

Headerless TSV, by 1-based column number:

```yaml
formats:
  tsv:
    header: false
    columns:
      "2": { rules: { no-todo: true } }
      "5": { parse-mode: plain, rules: { max-ten: { options: { max: 0 } } } }
```

General override (the desugared / cross-format form):

```yaml
overrides:
  - files: ["**/*.csv"]
    region: { kind: column, name: body }
    rules: { max-ten: false }
```

### Key disambiguation

String key → header name; bare-integer key → 1-based position. With `header: true`, name
match takes priority. Duplicate header names: warn, and target all columns with that name.

## 10. CLI / discovery / cache / fix integration

- **Discovery (`expand.rs`)**: replace `is_markdown_extension` with a registry-driven
  `is_supported_extension`. **Safety default**: directory/glob walks pick up a non-Markdown
  extension only when the config has that format's section (`formats.csv`/`formats.tsv`);
  Markdown is always discovered. **Explicitly named files always lint.** This avoids
  accidentally linting data CSVs.
- **stdin**: no extension → defaults to Markdown. An optional `--stdin-filename` (or
  `--format`) flag selects a processor for stdin input (optional in v1).
- **Cache (`cache.rs`)**: the cache key must incorporate the processor id and the
  parse/rule-affecting config (columns, header, delimiter, parse-mode, and the effective
  rule config), so changing column targeting or per-column rules invalidates correctly.
- **fix (`fix.rs`)**: goes through the same driver. CSV fixes land only inside disjoint
  cell spans (never delimiters/structure), so fixes across regions compose without
  overlap; iterate to a fixpoint as today.
- **dispatch parity**: a parity test asserts the same content through `lint_document`
  yields identical diagnostics regardless of caller (file vs stdin `--format csv`).

## 11. Error handling

Library crates forbid `unwrap`/`expect`/`panic!`; everything is `Result` or a diagnostic.

- **Parse errors**: a structural CSV error (e.g. an unterminated quote) → `ParseError`,
  handled like a Markdown `ParseError` (one diagnostic for the file, nothing else linted).
  Ragged rows / extra fields / empty cells are not errors (§6). The scanner must never
  panic on arbitrary bytes and must respect `MAX_FILE` / `MAX_SOURCE_LEN`.
- **Static config errors → `ConfigError`**: an unknown `formats` key (no registered
  processor); a name key under `columns` with `header: false`; an invalid `parse-mode`.
- **Runtime, data-dependent → stderr notes** (like the existing unknown-rule note): a
  configured column name absent from the file header (`note: column 'body' not found in
  header of data.csv`); a `.csv` argument with no `formats.csv` config (`note: no columns
  configured for 'csv'; nothing to lint`); unknown rule ids inside a column's `rules`.

## 12. Testing strategy (TDD; `just check` before pushing)

- **Scanner unit tests** (table-driven): quoting/escapes, embedded newline and delimiter,
  CRLF, BOM, ragged rows; assert each field's absolute span.
- **Rebasing unit tests**: slice parse + offset → diagnostics land at correct absolute
  offsets and `line:column`.
- **Config unit tests**: `formats`/`columns` parsing, `base ⊕ column` layering, name vs
  index resolution, validation errors.
- **Rule-resolution unit tests**: distinct rule sets; opt-in (unlisted columns not linted);
  parse-mode effect.
- **Integration tests** (`app.rs` `MockHost`): a per-column-rules CSV produces the right
  diagnostics with correct path/line/column; JSON output; cache invalidation on column
  config change; fix lands only inside cells; discovery opt-in (csv not auto-walked without
  config).
- **Parity test**: `lint_document` single path; file vs stdin `--format csv` match.

## 13. Extensibility deliverable (the top-priority requirement)

Adding a format must be a small, well-bounded change. Deliverables:

1. A skill `.agents/skills/tzlint-processors/`: the step-by-step recipe — implement
   `Processor` (with a "Regions vs Ast" decision guide), register one line in the built-in
   registry, define a `ProcessorConfig` slice if needed, add tests (scanner unit + a
   fixture integration test), add a `docs/` page.
2. **Wiring confined to one place**: built-in registration is a single registry list plus
   the new module — the only spot a contributor must touch besides their module.
3. `docs/design/input-format-processors.md` (this doc) as the contract reference: trait
   contract, span/offset rules, `parse_mode`, region tags, the Regions-vs-Ast guide, and
   the textlint-plugin correspondence.

## 14. ABI / frozen-AST impact

None. No `Node` field is added; region tags live outside the AST. The Markdown path is
byte-for-byte unchanged. The `golden_archived_layout_is_frozen` test is unaffected.

## 15. Decisions locked

1. **Approach A** (processor returns prose regions; core parses & lints per region), with
   an optional full-AST path on the same trait.
2. **`RegionTag` is format-neutral** (`kind`/`index`/`name`); "column" is the delimited
   processor's `kind`, not a core concept.
3. **Column targeting** by header name and by **1-based** column number.
4. **Per-region linting**, and **each cell is an independent mini-document** (rule `finish`
   does not cross cells).
5. **`parse_mode` default = Markdown** per region, overridable to `PlainText`.
6. **Opt-in columns** (only listed columns are linted) and **`base ⊕ column` layering**.

## 16. Open questions / future work

- Escaped-quote content fidelity (`""` vs `"`) via content normalization + span back-map.
- Strict IANA TSV escaping mode.
- "One AST per column" performance optimization (gated on document-level rule scope).
- Runtime/WASM processors aligned with the M3 plugin ABI (extend the same trait boundary).
- `--stdin-filename` / `--format` ergonomics and `columns: "*"` (lint-all-except) shorthand.
