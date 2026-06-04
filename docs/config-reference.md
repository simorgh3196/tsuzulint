# Configuration reference

> Status: the loader (`tzlint_core::config`), presets, `extends`, per-rule options/severity
> routing, and the published JSON Schema (`tzlint_core::CONFIG_SCHEMA`) are implemented. Per-file
> `overrides` are planned (see *Planned* below).

- **Files:** `.tzlintrc.jsonc` → `.tzlintrc.json` → `.tzlintrc.yaml` → `.tzlintrc.yml` →
  `.tzlintrc` (parsed as JSONC). Discovery walks upward from the working directory; the first
  directory holding any candidate wins, the highest-priority candidate in it is loaded, and
  co-located lower-priority candidates are returned as structured warnings (ignored, not
  merged — pass `--verbose` to see them). Use `-c/--config <PATH>` to load a specific file and
  skip discovery; the format is then inferred from the extension. A leading UTF-8 BOM is
  stripped, and an empty/whitespace-only (or comments-only) file is the default config —
  consistently across all formats. `tzlint init` writes a minimal starter `.tzlintrc.json`.
- **Formats:** strict JSON (`.json`); JSONC (`//` and `/* */` comments + trailing commas);
  YAML (`.yaml`/`.yml`). YAML **anchors/aliases are rejected** — they are unnecessary for
  configuration and enable alias-expansion ("billion laughs") memory-exhaustion that the
  config size cap cannot bound.
- **Validation:** serde `deny_unknown_fields` — an unknown top-level key is an error. Accepted
  top-level keys are: `language`, `message-language`, `rules`, `extends`, and `formats` (see
  below). Keys in the dynamic `rules` map that are not built-in rule ids are not an error but
  are **warned** by the CLI (`note: config references unknown rule '…'`), so a typo is surfaced
  rather than silently ignored.
- **Activation model (opt-out):** every built-in rule is **on by default**. A bare
  `tzlint lint` runs the full built-in set; a `rules` entry set to `false` disables that rule.
  So configuration *narrows* the default set rather than opting rules in.
- **Rules:** map of `rule-id` → `false` (off) | `true` (on, defaults) | `{ severity?, options? }`.
  `severity` is one of `error` | `warning` | `info` | `hint` (overrides the rule's default);
  `options` is arbitrary JSON routed into the rule (each rule reads the keys it understands and
  ignores the rest). In YAML, the boolean spellings `yes`/`no`/`on`/`off` are accepted in
  addition to `true`/`false`. Use `tzlint rules list` to see the resolved on/off + severity for
  every rule, and `tzlint rules explain <id>` for one rule's effective state and options.
- **Presets & `extends`:** `extends` takes a preset id (string) or an array of ids; the
  preset's settings form a base layer that your own `rules` override by id (user wins on a
  collision). Built-in presets:
  - `ja-basic` — `no-hankaku-kana`, `no-mixed-zenkaku-hankaku-alphabet`, `no-nfd`,
    `no-zero-width-spaces`, `ja-no-mixed-period`.
  - `ja-technical-writing` — the above plus `no-exclamation-question-mark`, and thresholds:
    `sentence-length` `max: 100`, `max-ten` `max: 3`, `max-kanji-continuous-len` `max: 6`.

  Because activation is opt-out, a preset does **not** restrict *which* rules run — every
  built-in rule is already on, so a preset effectively supplies **options and severities** for
  the rules it names. To run a narrower set, disable the unwanted rules explicitly. Morphology
  rules (e.g. `no-doubled-joshi`) are intentionally absent from the presets until M2.
- **Language:** `language` (e.g. `ja`) and `message-language` (the diagnostic locale,
  independent of the document language). Config keys are kebab-case (`message-language`).
- **JSON Schema:** a Draft 2020-12 schema is published as `tzlint_core::CONFIG_SCHEMA`
  (`$id` `https://tsuzulint.dev/schema/config/v1.json`) for editor completion/validation and
  CLI emission. It describes the **JSON-level** contract and is intentionally stricter than the
  loader on one point: it accepts only real booleans for a rule, whereas the loader also accepts
  the YAML string spellings (`"on"`/`"yes"`/…). A differential test pins schema↔loader agreement
  (and that one deliberate asymmetry) so they cannot drift.
- **Cache:** an in-memory document cache (skips re-linting unchanged content within a run) plus
  a persistent on-disk cache written to `.tzlintcache` in the working directory, so a repeat run
  over unchanged files skips parse+lint. `--no-cache` disables both. The cache key is
  `blake3(content)` + the full config + rule versions + parser/engine version + a morphology
  dictionary fingerprint (a placeholder until M2), so any change that could alter diagnostics
  invalidates the entry. A cache read/write failure only warns; results are unaffected.

## `formats` — per-format options

The `formats` key is a map from format id (`csv`, `tsv`, …) to format-specific settings. It
is an accepted top-level key (alongside `language`, `message-language`, `rules`, and
`extends`). See [`docs/processors.md`](processors.md) for the user-facing guide.

### `formats.<csv|tsv>`

- **`header`** (bool, optional; defaults to `false`): when `true` the first record is read
  as a header row (not linted; enables column lookup by name). When `false` (the default)
  only integer-key column selectors are valid — a name key is a config error.
- **`delimiter`** (optional): a single ASCII character that overrides the default delimiter
  (`,` for CSV, `\t` for TSV). Non-ASCII characters are rejected with a config error; omit
  the key to use the format default.
- **`columns`** (map): the columns to lint. **Only listed columns are linted** (opt-in
  semantics — unlisted columns such as IDs or timestamps are never linted).

  **Key:** a **header name** (string; requires `header: true`) or a **1-based column
  number** (bare integer string, e.g. `"2"`). When a region matches both a name-keyed and a
  number-keyed target, the **name** target's rules win (resolution is name-then-index,
  independent of key order).

  A **bare-integer key is always read as a column number**, even under `header: true` — so a
  header literally named `"2024"` cannot currently be selected by name (it would resolve to
  column 2024 and, if out of range, be skipped). Use a non-numeric header name, or select that
  column by its position. An explicit name/number disambiguation syntax is a possible future
  addition.

  **Value** (all fields optional):
  - `parse-mode`: `markdown` (default) | `plain`. `markdown` parses the cell as CommonMark
    and applies Markdown-aware rules. `plain` treats the cell as a single paragraph with no
    Markdown constructs (useful when `*`/`_`/`#` are literal).
  - `rules`: a rule overlay in the same shape as the top-level `rules` map
    (`false | true | { severity?, options? }`). **Layering**: the effective rule set is
    `base ⊕ column.rules` — the column overlay wins. To drop a base rule for one column,
    set it `false` in that column's `rules`.

  A configured column name absent from the actual file header produces a per-file note (not
  a hard error, because it is data-dependent).

**Example** — header CSV with two prose columns and different rules:

```yaml
language: ja
rules:
  no-hankaku-kana: true      # base: applies to every linted column unless overridden
formats:
  csv:
    header: true             # row 1 is a header → not linted; enables name lookup
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

**Example** — headerless TSV by 1-based column number:

```yaml
formats:
  tsv:
    header: false
    columns:
      "2": { rules: { no-todo: true } }
      "5": { parse-mode: plain, rules: { max-ten: { options: { max: 0 } } } }
```

## Planned

- Per-file **`overrides`** (glob-scoped `language`/rule settings), evolving the schema to a
  `v2` `$id`.
- General format-neutral `overrides` key (glob + region selector); only
  `formats.<csv|tsv>.columns` is wired today.
