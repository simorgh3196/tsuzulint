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
- **Validation:** serde `deny_unknown_fields` — an unknown top-level key is an error. Keys in
  the dynamic `rules` map that are not built-in rule ids are not an error but are **warned** by
  the CLI (`note: config references unknown rule '…'`), so a typo is surfaced rather than
  silently ignored.
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

## Planned

- Per-file **`overrides`** (glob-scoped `language`/rule settings), evolving the schema to a
  `v2` `$id`.
