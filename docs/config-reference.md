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
  top-level keys are: `language`, `message-language`, `rules`, `extends`, `formats`, and
  `morphology` (see below). Keys in the dynamic `rules` map that are not built-in rule ids are not an error but
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
  - `ja-technical-writing` — full parity with the 23-rule
    [`textlint-rule-preset-ja-technical-writing`](https://github.com/textlint-ja/textlint-rule-preset-ja-technical-writing):
    the upstream thresholds copied verbatim (`sentence-length` `max: 100`, `max-comma` `max: 3`,
    `max-ten` `max: 3`, `max-kanji-continuous-len` `max: 6`) plus every other preset rule enabled —
    the surface rules (`arabic-kanji-numbers`, `ja-no-mixed-period`, `no-nfd`,
    `no-invalid-control-character`, `no-zero-width-spaces`, `no-exclamation-question-mark`,
    `no-hankaku-kana`, `ja-no-weak-phrase`, `ja-no-abusage`, `ja-unnatural-alphabet`,
    `no-unmatched-pair`) and the morphology-backed style rules (`no-doubled-joshi`,
    `no-mix-dearu-desumasu`, `no-doubled-conjunction`, `no-doubled-conjunctive-particle-ga`,
    `no-double-negative-ja`, `no-dropping-the-ra`, `ja-no-redundant-expression`,
    `ja-no-successive-word`). It also bundles the tsuzulint-original `no-mixed-zenkaku-hankaku-alphabet`
    (no upstream counterpart). `ja-prh` is **not** bundled — its term list is project-specific, so
    configure it explicitly. See [`docs/migration-from-textlint.md`](migration-from-textlint.md) for
    the full rule-name mapping.

  The `ja-*` presets imply `language: ja` (your own `language` overrides it), so their Japanese
  rules run out of the box. Because activation is opt-out, a preset does **not** restrict *which*
  rules run — every built-in rule is already on, so a preset effectively supplies **options and
  severities** for the rules it names. To run a narrower set, disable the unwanted rules
  explicitly. The morphology-backed style rules `ja-technical-writing` enables are no-ops until a
  [`morphology`](#morphology--dictionary-for-morphology-dependent-rules) dictionary is configured
  (the engine skips them), so a preset never *silently requires* one.
- **Language:** `language` (e.g. `ja`) and `message-language` (the diagnostic locale,
  independent of the document language). Config keys are kebab-case (`message-language`).
  `language` also **scopes** which rules run: a JA-only rule (e.g. `sentence-length`, `max-ten`,
  `no-doubled-joshi`) runs only on Japanese documents, and when `language` is unset only the
  language-neutral rules run — so set `language: ja` (or extend a `ja-*` preset) to lint Japanese.
- **`ja-prh` terminology dictionaries:** the `ja-prh` rule reads terms two ways, which combine. An
  inline list under `options.terms` (`{ expected, pattern?, patterns?, regexPatterns? }`), and a
  list of external [`prh`](https://github.com/prh/prh) `.prh.yml` files under `options.dictionaries`
  (paths resolved relative to the config file):

  ```json
  { "rules": { "ja-prh": { "options": {
      "dictionaries": ["./prh/web.yml", "./prh/tech.yml"],
      "terms": [{ "expected": "JavaScript", "patterns": ["Javascript"] }]
  } } } }
  ```

  Each dictionary's `version` / `imports` / `rules` are honored: a rule's `expected` is the
  replacement (a `$1`-style template for a regex match), and a `pattern` written `/source/flags` is
  a regex. Regex matching uses a small, ReDoS-free engine; a pattern it cannot compile (e.g.
  lookaround) is skipped, and `specs` / `regexpMustEmpty` / `options.wordBoundary` are not yet
  applied. A dictionary that cannot be read or parsed is reported on stderr and skipped (lint still
  runs). Dictionaries are loaded at `lint`/`fix` time, not for `tzlint rules`.
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
  dictionary fingerprint (the pins of the dictionaries active for the run), so any change that
  could alter diagnostics — including a dictionary upgrade — invalidates the entry. A cache
  read/write failure only warns; results are unaffected.

## Built-in rules

Every rule below is **on by default** (opt-out); disable one with `rule-id: false`. The
**Lang** column reflects rule scoping (see *Language* above): language-neutral rules always run,
`ja` rules run only when the document language is Japanese. Morphology-backed rules are marked
**(morph)** — they are inert until a [`morphology`](#morphology--dictionary-for-morphology-dependent-rules)
dictionary is configured. Use `tzlint rules list` for the resolved on/off + severity and
`tzlint rules explain <id>` for one rule's effective options.

### Language-neutral

| Rule | What it flags |
| --- | --- |
| `no-nfd` | NFD (decomposed) Unicode; prefer NFC (autofix) |
| `no-zero-width-spaces` | zero-width space characters (autofix) |
| `no-mixed-zenkaku-hankaku-alphabet` | mixing full-width and half-width Latin letters in one run |
| `no-exclamation-question-mark` | `!` / `?` (and full-width `！` / `？`) in prose |
| `max-kanji-continuous-len` | runs longer than `max` (default 6) continuous kanji |
| `no-invalid-control-character` | invalid C0 / DEL control characters |
| `no-todo` | `TODO` / `FIXME` markers left in prose |

### Japanese — surface (no morphology)

| Rule | What it flags |
| --- | --- |
| `no-hankaku-kana` | half-width katakana; prefer full-width (autofix) |
| `ja-no-mixed-period` | mixed sentence-ending punctuation (`。` vs `.`) |
| `sentence-length` | sentences longer than `max` (default 100) |
| `max-ten` | more than `max` (default 3) 読点「、」 per sentence |
| `max-comma` | more than `max` (default 3) ASCII commas per sentence |
| `arabic-kanji-numbers` | inconsistent Arabic vs. kanji numeral usage (JTF 2.2.2) |
| `ja-unnatural-alphabet` | a stray single Latin letter between Japanese characters (likely IME error) |
| `no-unmatched-pair` | unmatched brackets / quotes (`detectOrphanedClosers` opts in to orphaned closers) |
| `ja-no-weak-phrase` | weak / hedging expressions (the 「かも」 family) |
| `ja-no-abusage` | common Japanese misuses (誤用) |
| `ja-prh` | 表記ゆれ / terminology from term lists + `.prh.yml` (autofix; see below) |

### Japanese — morphology-backed (need a dictionary)

| Rule | What it flags |
| --- | --- |
| `no-doubled-joshi` | the same 助詞 repeated within one sentence |
| `no-mix-dearu-desumasu` | mixing である (plain) and ですます (polite) styles in a document |
| `no-doubled-conjunction` | the same 接続詞 opening consecutive sentences |
| `no-doubled-conjunctive-particle-ga` | the 逆接の接続助詞「が」 used more than once in one sentence |
| `no-double-negative-ja` | a rhetorical double negative (ないことはない / なくはない) |
| `no-dropping-the-ra` | ら抜き言葉 (見れる → 見られる) |
| `ja-no-redundant-expression` | the redundant「〜することができる」family (reads as「〜できる」) |
| `ja-no-successive-word` | a word repeated in immediate succession |

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

## `morphology` — dictionary for morphology-dependent rules

Some rules (e.g. `no-doubled-joshi`) need a tokenized, part-of-speech-tagged view of the text,
which comes from a **dynamic, hash-pinned dictionary** — never embedded in the binary. The
`morphology` key points at one. It is provisioned at runtime **only when a rule active for the
run needs its language**, so configuring it is free for runs that don't exercise such a rule. See
[`docs/morphology.md`](morphology.md) for the full model and per-dictionary licenses.

- **`path`** / **`url`** (exactly one, required): the compressed container (`.dict.zst`). `path`
  is resolved relative to the working directory; `url` must be `https` (SSRF-guarded, fetched on
  a cache miss).
- **`pin`** (required): a BLAKE3 hash over the **compressed** container, 64 hexadecimal
  characters. The bytes are verified against it before decompression, and it is the cache key —
  so a dictionary upgrade is simply a new pin.
- **`lang`** (optional; default `"ja"`): the language the dictionary serves. **Only `"ja"` is
  supported today**; any other value is a config error.

```jsonc
{
  // `no-doubled-joshi` is on by default (opt-out); it stays inert until this is set.
  "morphology": {
    "url": "https://example.com/ipadic.dict.zst",
    "pin": "0000000000000000000000000000000000000000000000000000000000000000",
    "lang": "ja"
  }
}
```

The verified, decompressed dictionary is cached under `.tzlint/dict/` (native CLI). In the
browser the wasm build receives the bytes via `registerDictionary(...)` and the JS host owns the
cache. Absent a `morphology` key, morphology rules are inert and the cache key is byte-identical
to a pre-morphology run.

## Planned

- Per-file **`overrides`** (glob-scoped `language`/rule settings), evolving the schema to a
  `v2` `$id`.
- General format-neutral `overrides` key (glob + region selector); only
  `formats.<csv|tsv>.columns` is wired today.
