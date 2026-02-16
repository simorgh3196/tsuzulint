# tsuzulint_text

Text analysis components for TsuzuLint.

## Components

### Tokenizer

Based on [Lindera](https://github.com/lindera-morphology/lindera), this module provides morphological analysis for Japanese text.

### Sentence Splitter

The `SentenceSplitter` implements a hybrid approach combining **Unicode Standard Annex #29 (UAX #29)** rules with **Japanese-specific heuristics**.

#### Why Hybrid?

Standard UAX #29 rules are robust but sometimes too aggressive for Japanese text, especially in mixed contexts (e.g., Markdown, technical documentation). We apply custom heuristics to align with common Japanese writing styles.

#### Splitting Rules

1. **Standard UAX #29**: The base segmentation is provided by [`unicode-segmentation`](https://crates.io/crates/unicode-segmentation).
2. **Japanese Period (`。`)**: Always forces a split.
3. **Exclamation/Question Marks (`！`, `？`, `!`, `?`)**:
    * **Trailing Space/Newline**: If followed by whitespace (e.g., `すごい！！ 本当に`), it splits.
    * **No Trailing Space**: If followed immediately by non-whitespace (e.g., `すごい！！本当に`), the split is **suppressed**. This keeps emphatic expressions together.
4. **Newlines (`\n`)**:
    * **Single Newline**: Suppressed (treated as soft wrap) unless it follows a period that forces a split.
    * **Double Newline (`\n\n`)**: Always forces a split (treated as a paragraph break).
5. **Ignored Ranges**:
    * Ranges specified in `ignore_ranges` (e.g., code blocks, URLs) are treated as opaque. No splits occur strictly inside these ranges.

#### Usage

```rust
use tsuzulint_text::splitter::SentenceSplitter;

let text = "Line1.\nLine2.\n\nParagraph2.";
// Standard UAX would split at every period and newline.
// This splitter keeps "Line1.\nLine2." together.
let sentences = SentenceSplitter::split(text, &[]);

assert_eq!(sentences[0].text, "Line1.\nLine2.\n\n");
assert_eq!(sentences[1].text, "Paragraph2.");
```
