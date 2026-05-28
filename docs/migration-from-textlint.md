# Migration from textlint

> Status: template (M0).

**Compatibility scope (v1 intends to mirror):** the node-kind vocabulary (mdast/TxtAST-
derived), config concepts (rules map, per-rule options, presets, include/exclude), and
preset naming close to `textlint-rule-preset-ja-technical-writing`.

**Intentional divergence:** config filename `.tzlintrc.*` (closer to `.textlintrc`); rules
are authored against the Rust PDK (no binary rule-API compatibility).

Migration notes (config keys, rule-name mapping) filled in as rules land.
