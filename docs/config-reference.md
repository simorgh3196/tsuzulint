# Configuration reference

> Status: the loader (`tzlint_core::config`, M1d-2) and the published JSON Schema
> (`tzlint_core::CONFIG_SCHEMA`, M1d-3) are implemented. Per-file `overrides` are planned (see
> *Planned* below).

- **Files:** `.tzlintrc.jsonc` → `.tzlintrc.json` → `.tzlintrc.yaml` → `.tzlintrc.yml` →
  `.tzlintrc` (parsed as JSONC). Discovery walks upward from a start directory; the first
  directory holding any candidate wins, the highest-priority candidate in it is loaded, and
  co-located lower-priority candidates are returned as structured warnings (ignored, not
  merged). A leading UTF-8 BOM is stripped, and an empty/whitespace-only (or comments-only)
  file is the default config — consistently across all formats.
- **Formats:** strict JSON (`.json`); JSONC (`//` and `/* */` comments + trailing commas);
  YAML (`.yaml`/`.yml`). YAML **anchors/aliases are rejected** — they are unnecessary for
  configuration and enable alias-expansion ("billion laughs") memory-exhaustion that the
  config size cap cannot bound.
- **Validation:** serde `deny_unknown_fields` — an unknown top-level key is an error. (The
  dynamic `rules` map keys are not yet checked against a known-rule set; that arrives with the
  rules crate in M1f, after which an unknown rule id can be warned.)
- **Rules:** map of `rule-id` → `false` (off) | `true` (on, defaults) | `{ severity?, options? }`.
  `severity` is one of `error` | `warning` | `info` | `hint` (overrides the rule's default);
  `options` is arbitrary JSON passed through to the rule (must be JSON-representable). In YAML,
  the boolean spellings `yes`/`no`/`on`/`off` are accepted in addition to `true`/`false`.
- **Presets:** `ja-basic`, `ja-technical-writing` provide a base rule set that the user config
  overrides by id (user wins). The concrete rule sets are populated once rules land (M1f). The
  `extends` key is **reserved** and currently rejected with a clear error.
- **Language:** `language` (e.g. `ja`) and `message_language` (the diagnostic locale,
  independent of the document language). Config file keys are kebab-case (`message-language`).
- **JSON Schema:** a Draft 2020-12 schema is published as `tzlint_core::CONFIG_SCHEMA`
  (`$id` `https://tsuzulint.dev/schema/config/v1.json`) for editor completion/validation and
  CLI emission. It describes the **JSON-level** contract and is intentionally stricter than the
  loader on one point: it accepts only real booleans for a rule, whereas the loader also accepts
  the YAML string spellings (`"on"`/`"yes"`/…). A differential test pins schema↔loader agreement
  (and that one deliberate asymmetry) so they cannot drift.
- **Cache key:** `blake3(content)` + config + rule versions + parser/engine version +
  morphology dictionary fingerprint. *(Cache lands in M1e.)*

## Planned

- Per-file **`overrides`** (glob-scoped `language`/rule settings).
- `extends` for composing configs and pulling in presets from config.
- Per-rule **`options`** schemas once rules land (M1f), evolving the schema to a `v2` `$id`.
