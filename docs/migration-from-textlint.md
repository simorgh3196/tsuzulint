# Migration from textlint

> Status: template (M0).

**Compatibility scope (v1 intends to mirror):** the node-kind vocabulary (mdast/TxtAST-
derived), config concepts (rules map, per-rule options, presets, include/exclude), and
preset naming close to `textlint-rule-preset-ja-technical-writing`.

**Intentional divergence:** config filename `.tzlintrc.*` (closer to `.textlintrc`); rules
are authored against the Rust PDK (no binary rule-API compatibility).

## `prh` terminology dictionaries

Existing `prh` `.prh.yml` dictionaries (as used by `textlint-rule-prh`) load directly into the
`ja-prh` rule — point at them from `rules.ja-prh.options.dictionaries` (see the
[config reference](config-reference.md)). The dictionary's `version`, `imports`, and `rules`
(`expected` + `pattern`/`patterns`) are read; `expected` is the replacement and may use `$1`-style
capture references for a regex (`/source/flags`) pattern.

Limitations to be aware of when migrating: matching uses a smaller regex engine
(`regex-lite`), so Unicode-property classes (`\p{…}`) and lookaround/backreferences are
unsupported — a pattern using them is skipped rather than failing the load (Japanese matched as
literals is unaffected). The prh `specs`, `regexpMustEmpty`, and `options.wordBoundary` fields are
parsed but not yet applied.

Migration notes (config keys, rule-name mapping) filled in as rules land.
