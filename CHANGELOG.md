# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

While the project is in `0.x`, the public API and rule behavior may change between
minor releases.

## [0.1.0] - Unreleased

The first public release: a fast, embeddable Japanese prose linter — the Rust
counterpart to `textlint`. The release date is set when 0.1.0 is cut; entries below
accumulate as the release is built.

### Added

- **Lean core (`tzlint_core`).** A `markdown-rs` parser with an mdast → index-AST
  transform over the frozen `AstCoreV1` (rkyv) layout, a single-traversal lint engine
  with a stable `(span.start, span.end, rule_id, message)` diagnostic order, a
  document-level in-memory cache keyed by every input that can change a result, a
  multi-format configuration loader (TOML / JSON / YAML) with presets, a byte-offset →
  line/column position mapper, and an `io` / `Host` boundary that routes all filesystem
  and network access through a single abstraction.
- **Diagnostic and fix model.** A `Diagnostic` / `Fix` model with a convergent
  autofix engine.
- **Starter rules (`tzlint_rules`).** Eleven rules: `no-hankaku-kana`,
  `no-mixed-zenkaku-hankaku-alphabet`, `no-nfd`, `no-zero-width-spaces`,
  `ja-no-mixed-period`, `no-exclamation-question-mark`, `no-todo`, `sentence-length`,
  `max-ten`, `max-kanji-continuous-len`, and the morphology-backed `no-doubled-joshi`.
- **Japanese morphology style rules.** `no-mix-dearu-desumasu` flags mixing the である
  (plain) and ですます (polite) sentence styles within a document — auto-detecting the
  majority and flagging the minority, or enforcing a configured `prefer`red style.
  `no-doubled-conjunctive-particle-ga` flags the 逆接の接続助詞「が」 used more than once
  in one sentence (the subject-marking 格助詞「が」 is not counted).
  `ja-no-redundant-expression` flags the redundant「〜することができる」family (こと + が +
  できる/可能), which reads more tightly as 「〜できる」.
  `no-dropping-the-ra` flags ら抜き言葉 (a 一段/カ変 verb + the potential 「れる」 where
  「られる」 is standard, e.g. 見れる → 見られる; 五段 passives like 書かれる are not flagged).
  `no-double-negative-ja` flags a rhetorical double negative — a negative closed by
  「は」+ another negative (ないことはない / なくはない); 〜なければならない is not flagged.
  All five ship enabled in the `ja-technical-writing` preset (alongside `no-doubled-joshi`),
  mirroring `textlint-rule-preset-ja-technical-writing`; they stay no-ops until a morphology
  dictionary is configured.
- **Command-line interface (`tzlint`).** `lint`, `fix`, `init`, and `rules`
  subcommands with `text`, `json`, and `sarif` output formats.
- **Japanese morphology.** A language-neutral `MorphologyProvider` seam over the
  frozen `MorphologyV1` token table, a Japanese backend (native, feature-gated), and a
  dynamic, non-embedded dictionary pipeline — hash-pinned provisioning, verification,
  decompression, and caching — folded into the cache key as a morphology fingerprint.
- **WebAssembly bindings (`tzlint_wasm`).** A `TsuzuLint` binding exposing `lint`, plus
  `registerDictionary` for host-supplied morphology dictionaries, shipped as lean
  (no tokenizer) and full (`morphology` feature) artifacts.
- **Frozen ABIs.** `AstCoreV1` and `MorphologyV1` are frozen and extended only via
  additive tables.

[0.1.0]: https://github.com/simorgh3196/tsuzulint/releases/tag/v0.1.0
