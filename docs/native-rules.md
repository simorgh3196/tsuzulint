# Native Rules

TsuzuLint ships a growing set of *native* rules compiled directly into the
binary. They run without a WASM boundary, share the parser's AST and
morphological tokens, and have sub-microsecond per-file overhead.

> [!TIP]
> Native rules are the fastest execution path. Community-specific or
> project-custom rules are a better fit for the WASM plugin system.

## Using built-in rules

Enable a rule by name in the `options` section of `.tsuzulint.jsonc`:

```jsonc
{
  "options": {
    "no-todo": true,
    "sentence-length": { "max": 90 },
    "max-ten": { "max": 3 }
  }
}
```

No entry in `rules:` is needed — native rules do not load a plugin.

## Presets

Instead of wiring each rule individually, enable a curated bundle:

```jsonc
{
  "presets": ["ja-technical-writing"],
  "options": {
    "sentence-length": { "max": 90 }  // overrides the preset default
  }
}
```

Preset rules can be individually disabled by setting their entry to `false`:

```jsonc
{
  "presets": ["ja-technical-writing"],
  "options": {
    "no-exclamation-question-mark": false
  }
}
```

## Choosing which rules to enable

Every rule is optional. The short version:

- **Opinion-free "almost always" rules** — invisible-char / Unicode hygiene
  checks that fire only when something is genuinely wrong and have no
  stylistic bias. Safe to leave on for any project.
- **Style rules** — things like sentence length, comma count, mixed
  alphabet widths. These encode a style guide. Enable them if your team
  agrees on the convention; disable or override thresholds otherwise.
- **Project-specific rules** — markers like `TODO:`. Useful as a
  pre-publish check, noisy during day-to-day development. Many teams
  enable them only in CI.

The table below summarizes which bucket each rule falls into. "Recommended
default" means the rule is on in `ja-technical-writing`; the last column
gives the typical reason to turn it off.

| Rule | Category | Recommended default | Typical reason to disable |
| :--- | :--- | :---: | :--- |
| `no-zero-width-spaces`               | Hygiene       | ✅ on  | Your source intentionally contains ZWSP / BOM (rare). |
| `no-nfd`                             | Hygiene       | ✅ on  | Linguistic / test fixtures that must preserve NFD. |
| `no-hankaku-kana`                    | Hygiene       | ✅ on  | Legacy Shift-JIS output; data with half-width kana by spec. |
| `no-mixed-zenkaku-hankaku-alphabet`  | Hygiene       | ✅ on  | You deliberately mix half/full-width for domain terms. |
| `ja-no-mixed-period`                 | Hygiene       | ✅ on  | Docs that intentionally use English `.` and Japanese `。` by structure. |
| `no-exclamation-question-mark`       | Style         | ✅ on  | Marketing / conversational prose where `！` / `？` are expected. |
| `sentence-length`                    | Style         | ✅ on (max 100) | Tutorials / narrative prose where long sentences are intended. |
| `max-ten`                            | Style         | ✅ on (max 3)   | Legal / academic text with deliberately long clauses. |
| `max-kanji-continuous-len`           | Style         | ✅ on (max 6)   | Proper nouns / legal names that unavoidably run long (use `allow`-style overrides via ignore patterns, or raise `max`). |
| `no-doubled-joshi`                   | Morphological | ⚠️ opt-in on big docs | Rule uses Lindera morphology — adds ~50 ms of startup per run. Turn off for tiny scripts where the fixed cost matters, or in tests where forcing particle repetition is intentional. |
| `no-todo`                            | Project       | ❌ opt-in       | Too noisy mid-development. Most teams enable only in CI or the pre-push hook, not in local save-on-lint. |

**Disabling a rule** — set its entry to `false` in `options`:

```jsonc
{
  "presets": ["ja-technical-writing"],
  "options": {
    "no-todo": false,              // off entirely
    "sentence-length": { "max": 140 }, // override threshold instead of disabling
    "no-exclamation-question-mark": "off" // equivalent: severity "off"
  }
}
```

**Enable-only a handful** — skip presets and just list them:

```jsonc
{
  "options": {
    "sentence-length": { "max": 90 },
    "no-doubled-joshi": true
  }
}
```

## Available rules

| Rule | Category | Textlint equivalent |
| :--- | :--- | :--- |
| `no-todo`                            | Marker        | `textlint-rule-no-todo` |
| `sentence-length`                    | Style         | `textlint-rule-sentence-length` |
| `max-ten`                            | Style         | `textlint-rule-max-ten` |
| `max-kanji-continuous-len`           | Style         | `textlint-rule-max-kanji-continuous-len` |
| `no-doubled-joshi`                   | Morphological | `textlint-rule-no-doubled-joshi` |
| `no-exclamation-question-mark`       | Punctuation   | `textlint-rule-no-exclamation-question-mark` |
| `no-hankaku-kana`                    | Character     | `textlint-rule-no-hankaku-kana` |
| `no-mixed-zenkaku-hankaku-alphabet`  | Character     | `textlint-rule-no-mixed-zenkaku-and-hankaku-alphabet` |
| `no-zero-width-spaces`               | Character     | `textlint-rule-no-zero-width-spaces` |
| `no-nfd`                             | Unicode       | `textlint-rule-no-nfd` |
| `ja-no-mixed-period`                 | Punctuation   | `textlint-rule-ja-no-mixed-period` |

### Rule details

#### `no-todo`

Detects `TODO:`, `FIXME:`, `XXX:`, `HACK:` markers in prose. Skips
`CodeBlock` and `Code` nodes so markers inside code samples don't fire.

- **Enable when**: you want a pre-publish / CI gate that catches forgotten
  markers, or your style guide forbids them in merged docs.
- **Disable when**: you're writing WIP docs locally, or markers are part
  of the content (e.g. a tutorial *about* the TODO convention). A common
  pattern is to enable this rule only in CI, not in the editor.

Options:

| Option | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `patterns`         | `string[]` | `["TODO:", "FIXME:", "XXX:", "HACK:", …]` | Replace the default pattern list. |
| `ignore_patterns`  | `string[]` | `[]`    | Substrings that should *not* fire the rule. |
| `case_sensitive`   | `boolean`  | `false` | Match case-sensitively. |

#### `sentence-length`

Flags sentences longer than `max` characters. URLs are collapsed to one
character before counting so a long share URL doesn't explode the count.

- **Enable when**: you write reference docs, API guides, or release
  notes — audiences skim and shorter sentences help.
- **Disable when**: the content is narrative / literary / conversational,
  or when legal/academic conventions require long clauses. In those cases
  override the threshold instead of turning the rule off:
  `"sentence-length": { "max": 160 }`.

| Option | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `max`       | `number`  | `100` | Maximum number of characters in a sentence. |
| `skip_urls` | `boolean` | `true` | Collapse URL runs before counting. |

#### `max-ten`

Limits the number of `、` per sentence. A sentence is any text between
`touten` boundaries (default `、`), terminated by `kuten` (default `。`).

- **Enable when**: you want to encourage splitting comma-heavy sentences
  — four `、` in one sentence is usually easier to read as two sentences.
- **Disable when**: your prose has structured enumerations that legit need
  many commas, or you follow a style guide like 法令 / 条文 where comma
  counts are dictated by convention. Consider raising `max` (5–6) before
  disabling outright.

| Option | Type | Default |
| :--- | :--- | :--- |
| `max`    | `number` | `3` |
| `touten` | `string` | `"、"` |
| `kuten`  | `string` | `"。"` |

#### `max-kanji-continuous-len`

Flags runs of consecutive kanji characters longer than `max`.

- **Enable when**: you target a general / mixed-audience reader. Long
  kanji runs hurt scannability; inserting 送り仮名 breaks them up.
- **Disable when**: the domain is full of multi-kanji proper nouns (e.g.
  医学 / 法律 / 古典) where splitting is impossible. Raise `max` to 8–10
  as a middle ground before disabling.

| Option | Type | Default |
| :--- | :--- | :--- |
| `max` | `number` | `5` |

#### `no-doubled-joshi`

Detects repeated Japanese particles (助詞) within a single sentence. Uses
Lindera morphological analysis (declares `needs_morphology` → the linter
auto-loads the tokenizer when this rule is enabled).

- **Enable when**: you edit technical docs / blog posts / marketing copy —
  duplicated は / が / を are one of the most common Japanese writing
  mistakes and very hard to spot manually.
- **Disable when**: the source is literary (fiction often repeats
  particles intentionally), or you need the ~50 ms of tokenizer startup
  back (it matters for very short files in hot tooling loops). Consider
  setting `strict: false` (the default) rather than disabling entirely
  so exempted particles like `の` / `を` stay quiet.

| Option | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `min_interval`          | `number`   | `1`                     | Minimum distance (in commas) between repeated particles. |
| `strict`                | `boolean`  | `false`                 | Disable exemptions for `の`, `を`, `て`, 並立助詞, `かどうか`. |
| `allow`                 | `string[]` | `[]`                    | Particles to never flag even when repeated. |
| `separator_characters`  | `string[]` | `[".", "。", "!", "?", …]` | Sentence terminators that reset counting. |
| `comma_characters`      | `string[]` | `["、", "，"]`          | Comma characters that increase the interval. |

#### `no-exclamation-question-mark`

Disallows `!`, `?`, `！`, `？` in prose. Each can be individually
whitelisted.

- **Enable when**: you write technical / formal documentation where
  these marks are stylistically out of place.
- **Disable when**: the content is marketing, conversational, support
  replies, or onboarding material where `?` questions naturally appear.
  Whitelisting just the full-width or just the ASCII variant is often
  enough: `{"allow_halfwidth_question": true}`.

| Option | Default |
| :--- | :--- |
| `allow_halfwidth_exclamation` | `false` |
| `allow_fullwidth_exclamation` | `false` |
| `allow_halfwidth_question`    | `false` |
| `allow_fullwidth_question`    | `false` |

#### `no-hankaku-kana`

Flags half-width katakana (U+FF61..U+FF9F).

- **Enable when**: essentially always for modern Japanese text. Half-width
  kana breaks search, collation, and copy-paste in most downstream tools.
- **Disable when**: the document is about legacy encoding / showing
  half-width kana as an example, or it's generated output from a
  Shift-JIS system you cannot change.

#### `no-mixed-zenkaku-hankaku-alphabet`

Checks the whole document for mixed half-width (ASCII A–Z, a–z) and
full-width (U+FF21..U+FF5A) alphabet characters. When both styles are
present, every occurrence of the rarer style is flagged so you can
normalise to the dominant one.

- **Enable when**: you want consistent rendering in searches, collations,
  code fences, and grep — a stray `Ｇｉｔ` in an otherwise `git` document
  is a common copy-paste mistake.
- **Disable when**: your docs deliberately demonstrate the difference
  (e.g. an article *about* width normalisation), or proper nouns in your
  domain are always written full-width.

#### `no-zero-width-spaces`

Flags zero-width-ish invisible codepoints (ZWSP `\u{200B}`, ZWNJ `\u{200C}`,
ZWJ `\u{200D}`, WJ `\u{2060}`, BOM `\u{FEFF}`).

- **Enable when**: almost always. Invisible codepoints are almost never
  intentional in technical docs and they break diffs / search / code
  review in subtle ways.
- **Disable when**: you're specifically documenting these characters, or
  your content uses them to control line-breaking in specific languages
  (e.g. Thai/Khmer, though tsuzulint is Japanese-focused).

#### `no-nfd`

Flags decomposed Unicode codepoints (combining marks) that should be
normalised to NFC.

- **Enable when**: almost always. macOS filesystems and some editors
  silently produce NFD, which breaks `grep` / string equality in every
  downstream tool. Catching it at lint time is cheap.
- **Disable when**: your content is Unicode-test data that must preserve
  specific NFD sequences.

#### `ja-no-mixed-period`

When a document uses both `。` and `.` as sentence terminators, flags
occurrences of the minority style (so the author can pick one).

- **Enable when**: you write documents in either style consistently —
  this rule catches accidental mixing. The dominant style is inferred
  from the document itself so you don't have to configure anything.
- **Disable when**: your document legitimately has English sentences
  (ending in `.`) alongside Japanese sentences (ending in `。`). In that
  case the rule's "minority" heuristic is wrong for you.

## Available presets

| Preset | Rules | Intended for |
| :--- | :--- | :--- |
| `ja-technical-writing` | 10 rules, close to textlint-rule-preset-ja-technical-writing | Engineering docs |
| `ja-basic`             | 5 text-quality checks (no length limits) | Blog posts, casual docs |

## Writing a new native rule

Native rules live under [`crates/tsuzulint_core/src/native_rules/rules/`][dir].
Each rule is a single file that implements [`native_rules::Rule`][trait]:

```rust
use tsuzulint_core::native_rules::{Rule, RuleContext};
use tsuzulint_plugin::Diagnostic;

pub struct MyRule;
pub static RULE: MyRule = MyRule;

impl Rule for MyRule {
    fn name(&self) -> &'static str { "my-rule" }
    fn description(&self) -> &'static str { "Describe the rule here." }
    fn lint(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        // Walk ctx.ast, emit Diagnostic::new(...)...
        vec![]
    }
}
```

Register the rule in
`crates/tsuzulint_core/src/native_rules/rules/mod.rs` by adding it to
`pub mod <name>;` and the `all()` slice.

Tests live in the same file under `#[cfg(test)] mod tests` — we recommend
one test per behavioral case (valid input, edge case, specific option
combination). The existing rules are good examples to copy.

[dir]: ../crates/tsuzulint_core/src/native_rules/rules/
[trait]: ../crates/tsuzulint_core/src/native_rules/rule_trait.rs
