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

Detects `TODO:`, `FIXME:`, `XXX:`, `HACK:` markers in prose. Skips `CodeBlock`
and `Code` nodes.

Options:

| Option | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `patterns`         | `string[]` | `["TODO:", "FIXME:", "XXX:", "HACK:", …]` | Replace the default pattern list. |
| `ignore_patterns`  | `string[]` | `[]`    | Substrings that should *not* fire the rule. |
| `case_sensitive`   | `boolean`  | `false` | Match case-sensitively. |

#### `sentence-length`

Flags sentences longer than `max` characters. URLs are collapsed to one
character for counting.

| Option | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `max`       | `number`  | `100` | Maximum number of characters in a sentence. |
| `skip_urls` | `boolean` | `true` | Collapse URL runs before counting. |

#### `max-ten`

Limits the number of `、` per sentence. A sentence is any text between
`touten` boundaries (default `、`), terminated by `kuten` (default `。`).

| Option | Type | Default |
| :--- | :--- | :--- |
| `max`    | `number` | `3` |
| `touten` | `string` | `"、"` |
| `kuten`  | `string` | `"。"` |

#### `max-kanji-continuous-len`

Flags runs of consecutive kanji characters longer than `max`.

| Option | Type | Default |
| :--- | :--- | :--- |
| `max` | `number` | `5` |

#### `no-doubled-joshi`

Detects repeated Japanese particles (助詞) within a single sentence. Uses
Lindera morphological analysis (requires `needs_morphology` → auto-enables
the tokenizer when this rule is configured).

| Option | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `min_interval`          | `number`   | `1`                     | Minimum distance (in commas) between repeated particles. |
| `strict`                | `boolean`  | `false`                 | Disable exemptions for `の`, `を`, `て`, 並立助詞, `かどうか`. |
| `allow`                 | `string[]` | `[]`                    | Particles to never flag even when repeated. |
| `separator_characters`  | `string[]` | `[".", "。", "!", "?", …]` | Sentence terminators that reset counting. |
| `comma_characters`      | `string[]` | `["、", "，"]`          | Comma characters that increase the interval. |

#### `no-exclamation-question-mark`

Disallows `!`, `?`, `！`, `？` in prose. Each can be individually whitelisted.

| Option | Default |
| :--- | :--- |
| `allow_halfwidth_exclamation` | `false` |
| `allow_fullwidth_exclamation` | `false` |
| `allow_halfwidth_question`    | `false` |
| `allow_fullwidth_question`    | `false` |

#### `no-hankaku-kana`

Flags half-width katakana (U+FF61..U+FF9F).

#### `no-mixed-zenkaku-hankaku-alphabet`

Flags paragraphs that mix half-width (ASCII) and full-width (U+FF21..U+FF5A)
alphabet characters.

#### `no-zero-width-spaces`

Flags zero-width-ish invisible codepoints (ZWSP, ZWNJ, ZWJ, WJ, BOM).

#### `no-nfd`

Flags decomposed Unicode codepoints (combining marks) that should be
normalized to NFC.

#### `ja-no-mixed-period`

When a document uses both `。` and `.` as sentence terminators, flags
occurrences of the minority style (so the author can pick one).

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
