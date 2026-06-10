# Migration from textlint

TsuzuLint targets the Japanese `textlint` workflow. If you lint Japanese prose with `textlint`
and `textlint-rule-preset-ja-technical-writing` today, the concepts carry over directly — a
rules map, per-rule options, presets, and `prh` term dictionaries — and most rule IDs are
identical.

## Getting started

The config file is `.tzlintrc.{json,yaml,toml}` (echoing `.textlintrc`). Point it at the
bundled preset:

```json
{ "extends": "ja-technical-writing" }
```

`ja-technical-writing` mirrors `textlint-rule-preset-ja-technical-writing`: it applies the same
length / 読点 thresholds and enables the Japanese style rules below. (`ja-basic` is the
dictionary-free subset.) Run `tzlint rules list` to see the resolved set, and
`tzlint rules explain <id>` for any one rule. See the [config reference](config-reference.md)
for the full surface.

## Rule mapping

Where a TsuzuLint rule corresponds to a textlint one it keeps the same ID (without the
`textlint-rule-` prefix), so a familiar preset reads the same. `ja-technical-writing` enables
these 15 rules in 0.1.0:

| Rule | Needs morphology |
|---|---|
| `sentence-length` | — |
| `max-ten` | — |
| `max-kanji-continuous-len` | — |
| `no-hankaku-kana` | — |
| `no-mixed-zenkaku-hankaku-alphabet` | — |
| `no-nfd` | — |
| `no-zero-width-spaces` | — |
| `ja-no-mixed-period` | — |
| `no-exclamation-question-mark` | — |
| `no-doubled-joshi` | yes |
| `no-mix-dearu-desumasu` | yes |
| `no-doubled-conjunctive-particle-ga` | yes |
| `ja-no-redundant-expression` | yes |
| `no-dropping-the-ra` | yes |
| `no-double-negative-ja` | yes |

The morphology-backed rules tokenize Japanese, so they stay inert until you configure a
dictionary (see [morphology](morphology.md)); until then they report nothing rather than
guessing, so the preset is safe to enable beforehand.

The two remaining built-ins are not bundled in either preset — enable them in your `rules` map:
`no-todo` (leftover TODO/FIXME markers) and `ja-prh` (terminology / 表記ゆれ, below). textlint
rules without a TsuzuLint equivalent are simply out of scope for 0.1.0 — the
[roadmap](roadmap.md) tracks what is next.

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

## Intentional divergence

Config filename `.tzlintrc.*` (closer to `.textlintrc`); rules are authored against the Rust
PDK, so there is no binary rule-API compatibility — custom textlint rules are reimplemented,
not loaded.
