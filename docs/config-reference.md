# Configuration reference

> Status: template (M0/M1).

- **Files:** `.tzlintrc.jsonc` → `.json` → `.yaml` → `.yml` → `.tzlintrc` (JSONC).
  Discovery walks upward; first found wins; extras warn.
- **Validation:** serde `deny_unknown_fields` + a published JSON Schema (unknown keys are
  an error).
- **Rules:** map of `rule-id` → `false` | `{ severity, options }`. **Presets**
  (`ja-technical-writing`, `ja-basic`) expand to rule sets; user config overrides by id.
  `extends` reserved for later.
- **Language:** `Config.language` (+ per-file overrides); `message_language` selects
  diagnostic locale (independent of document language).
- **Cache key:** `blake3(content)` + config + rule versions + parser/engine version +
  morphology dictionary fingerprint.
