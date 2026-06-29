# Migration from textlint

> Status: **shipped (0.1.0).** TsuzuLint implements the full
> [`textlint-rule-preset-ja-technical-writing`](https://github.com/textlint-ja/textlint-rule-preset-ja-technical-writing)
> rule set plus a `prh` counterpart, so a Japanese textlint project can switch with a small
> config translation. This page is the concrete switch guide.

TsuzuLint deliberately names its rules after the textlint preset's, so for most projects the
migration is a config translation rather than a rewrite. The linting still runs through the same
[`tzlint lint`](../README.md#usage) dispatch, so editor and CI results match byte-for-byte.

## At a glance

| | textlint | TsuzuLint |
| --- | --- | --- |
| Runtime | Node.js + npm dependency tree | a single native binary (`tzlint`) |
| Config file | `.textlintrc.{json,yml,…}` | `.tzlintrc.{jsonc,json,yaml,yml}` |
| Preset | `preset-ja-technical-writing` | `extends: "ja-technical-writing"` |
| Terminology | `textlint-rule-prh` (`.prh.yml`) | `ja-prh` rule (imports the same `.prh.yml`) |
| Rule activation | opt-in (rules listed are on) | **opt-out** (every built-in rule is on; set `false` to disable) |

## 1. Install and initialize

```sh
cargo install --git https://github.com/simorgh3196/tsuzulint tzlint_cli   # see docs/install.md
tzlint init                                                               # writes a starter .tzlintrc.json
```

See [`docs/install.md`](install.md) for the other install methods.

## 2. Translate the config

A typical Japanese `.textlintrc`:

```json
{
  "plugins": ["@textlint/markdown"],
  "rules": {
    "preset-ja-technical-writing": {
      "sentence-length": { "max": 100 },
      "no-exclamation-question-mark": false
    },
    "prh": { "rulePaths": ["./prh/tech.yml"] }
  }
}
```

becomes `.tzlintrc.jsonc`:

```jsonc
{
  // `ja-technical-writing` implies `language: ja`, so the Japanese rules run out of the box.
  "extends": "ja-technical-writing",
  "rules": {
    "sentence-length": { "options": { "max": 100 } },
    "no-exclamation-question-mark": false,
    "ja-prh": { "options": { "dictionaries": ["./prh/tech.yml"] } }
  }
}
```

Key differences to keep in mind:

- **Markdown is built in** — there is no `plugins` key; TsuzuLint parses Markdown natively.
- **Per-rule options move under `options`** — textlint's `"rule": { "max": 100 }` becomes
  `"rule": { "options": { "max": 100 } }`. Severity is a sibling key (`{ "severity": "error" }`).
- **Activation is opt-out** — listing a rule is not what turns it on (every built-in rule is on by
  default). Disable a rule with `false`, exactly as textlint does. To run a *narrow* set, disable
  the rest explicitly.
- **`language: ja`** — set it (or extend a `ja-*` preset, which implies it) so the Japanese rules
  run; with `language` unset only the language-neutral rules fire. See the
  [config reference](config-reference.md).

## 3. Rule mapping

Every rule of `textlint-rule-preset-ja-technical-writing` (23 rules) has a same-named TsuzuLint
rule, all bundled in the `ja-technical-writing` preset.

| textlint preset rule | TsuzuLint rule id | Notes |
| --- | --- | --- |
| `sentence-length` | `sentence-length` | |
| `max-comma` | `max-comma` | |
| `max-ten` | `max-ten` | |
| `max-kanji-continuous-len` | `max-kanji-continuous-len` | |
| `arabic-kanji-numbers` | `arabic-kanji-numbers` | |
| `no-mix-dearu-desumasu` | `no-mix-dearu-desumasu` | needs a [morphology dictionary](config-reference.md#morphology--dictionary-for-morphology-dependent-rules) |
| `ja-no-mixed-period` | `ja-no-mixed-period` | |
| `no-double-negative-ja` | `no-double-negative-ja` | needs a morphology dictionary |
| `no-dropping-the-ra` | `no-dropping-the-ra` | needs a morphology dictionary |
| `no-doubled-conjunction` | `no-doubled-conjunction` | needs a morphology dictionary |
| `no-doubled-conjunctive-particle-ga` | `no-doubled-conjunctive-particle-ga` | needs a morphology dictionary |
| `no-doubled-joshi` | `no-doubled-joshi` | needs a morphology dictionary |
| `no-nfd` | `no-nfd` | |
| `no-zero-width-spaces` | `no-zero-width-spaces` | |
| `no-exclamation-question-mark` | `no-exclamation-question-mark` | |
| `no-hankaku-kana` | `no-hankaku-kana` | |
| `ja-no-weak-phrase` | `ja-no-weak-phrase` | |
| `ja-no-successive-word` | `ja-no-successive-word` | needs a morphology dictionary |
| `ja-no-abusage` | `ja-no-abusage` | |
| `ja-no-redundant-expression` | `ja-no-redundant-expression` | needs a morphology dictionary |
| `ja-unnatural-alphabet` | `ja-unnatural-alphabet` | |
| `no-unmatched-pair` | `no-unmatched-pair` | only unclosed openers by default; `detectOrphanedClosers` opts in |
| `no-invalid-control-character` | `no-invalid-control-character` | |
| `textlint-rule-prh` (plugin) | `ja-prh` | imports `.prh.yml` — see below |

The morphology-backed rules are **on by default but inert until a `morphology` dictionary is
configured** (the engine skips them otherwise), so the preset never silently requires one. See
[`docs/morphology.md`](morphology.md) to turn them on.

TsuzuLint also ships two rules with no upstream-preset counterpart: `no-mixed-zenkaku-hankaku-alphabet`
(bundled in `ja-technical-writing`) and `no-todo`.

## 4. `prh` terminology dictionaries

Existing `prh` `.prh.yml` dictionaries (as used by `textlint-rule-prh`) load directly into the
`ja-prh` rule — point at them from `rules.ja-prh.options.dictionaries` (see the
[config reference](config-reference.md)). The dictionary's
`version`, `imports`, and `rules` (`expected` + `pattern`/`patterns`) are read; `expected` is the
replacement and may use `$1`-style capture references for a regex (`/source/flags`) pattern.

Limitations to be aware of when migrating: matching uses a smaller regex engine
(`regex-lite`), so Unicode-property classes (`\p{…}`) and lookaround/backreferences are
unsupported — a pattern using them is skipped rather than failing the load (Japanese matched as
literals is unaffected). The prh `specs`, `regexpMustEmpty`, and `options.wordBoundary` fields are
parsed but not yet applied.

## What's not covered yet

- **Markdown-structural rules** (the `markdownlint`-style MD0xx family) and a bundled Markdown
  preset are deferred to 0.2.0 — keep using `markdownlint`/`textlint` for those for now.
- **Off-preset prose rules** outside `textlint-rule-preset-ja-technical-writing` — e.g. pangu-style
  spacing (`ja-space-between-half-and-full-width` and friends) — are not implemented yet.
- **Custom textlint rules / plugins** authored in JavaScript do not run; TsuzuLint rules are
  authored against the Rust PDK (no binary rule-API compatibility). A WebAssembly plugin ABI is on
  the roadmap (M3).
- **Editor integration** ships first as a CLI-backed VSCode extension; a full LSP server is the
  0.2.0 upgrade (M5).
- **Some `prh` fields** (`specs`, `regexpMustEmpty`, `options.wordBoundary`) and regex features
  (lookaround, `\p{…}`) are parsed-but-skipped, as noted above.

## Compatibility scope and intentional divergence

**Compatibility scope (v1 mirrors):** the node-kind vocabulary (mdast/TxtAST-derived), config
concepts (rules map, per-rule options, presets, include/exclude), and preset naming close to
`textlint-rule-preset-ja-technical-writing`.

**Intentional divergence:** config filename `.tzlintrc.*` (closer to `.textlintrc`); per-rule
options nest under an `options` key; activation is opt-out; rules are authored against the Rust PDK
(no binary rule-API compatibility).
