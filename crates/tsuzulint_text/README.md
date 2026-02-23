# tsuzulint_text

Text analysis component. Provides morphological analysis (tokenization) and sentence splitting functionality.

## Overview

`tsuzulint_text` is the **text analysis component of the TsuzuLint project**. It provides two core functionalities essential for natural language linting:

- **Morphological Analysis**: Tokenization of Japanese text
- **Sentence Splitting**: Sentence boundary detection based on UAX #29 + Japanese-specific heuristics

These serve as the foundation for linting rules (e.g., "check sentence-final particles", "limit sentence length").

## Module Structure

```text
tsuzulint_text/
├── Cargo.toml
├── README.md
├── examples/
│   └── uax29_test.rs      # Example for UAX #29 behavior verification
└── src/
    ├── lib.rs             # Public API definitions
    ├── tokenizer.rs       # Morphological analyzer
    └── splitter.rs        # Sentence splitter
```

## Tokenizer (Morphological Analysis)

Performs tokenization of Japanese text using Lindera (a Rust morphological analysis library based on MeCab).

### Token Structure

```rust
pub struct Token {
    pub surface: String,       // Surface form (the text itself)
    pub pos: Vec<String>,      // Part of speech (e.g., ["noun", "general"])
    pub detail: Vec<String>,   // Detailed part of speech information
    pub span: Range<usize>,    // Byte range within the original text
}
```

### Tokenizer

```rust
pub struct Tokenizer { /* ... */ }

impl Tokenizer {
    /// Create an instance using IPADIC dictionary (embedded version)
    pub fn new() -> Result<Self, TextError>;
    
    /// Convert text to a sequence of tokens
    pub fn tokenize(&self, text: &str) -> Result<Vec<Token>, TextError>;
}
```

### Processing Logic

1. Load IPADIC dictionary (`embedded://ipadic`)
2. Perform segmentation with Lindera's `Normal` mode
3. Extract the following from each token:
   - `surface`: Surface form
   - `pos`: Part of speech information (up to 4 elements, excluding `*`)
   - `detail`: Detailed information (5th element onwards)
   - `span`: Byte position range

### Usage Example

```rust
use tsuzulint-text::Tokenizer;

let tokenizer = Tokenizer::new()?;
let tokens = tokenizer.tokenize("こんにちは世界");

for token in tokens {
    println!("{}: {:?}", token.surface, token.pos);
}
// Output:
// こんにちは: ["interjection"]
// 世界: ["noun", "general"]
```

## SentenceSplitter (Sentence Splitting)

A hybrid approach combining Unicode Standard Annex #29 (UAX #29) based sentence splitting with **Japanese-specific heuristics**.

### Why Hybrid?

Standard UAX #29 rules tend to over-split Japanese text:

- `すごい！！本当に！？` → UAX #29 splits after "！！"
- In Japanese emphasis expressions, we want to treat this as a single sentence

### Sentence Structure

```rust
pub struct Sentence {
    pub text: String,          // Sentence text content
    pub span: Range<usize>,    // Byte range within the original text
}
```

### SentenceSplitter

```rust
impl SentenceSplitter {
    /// Split text into sentences
    pub fn split(text: &str, ignore_ranges: &[Range<usize>]) -> Vec<Sentence>;
}
```

- `ignore_ranges`: Specifies ranges where splitting is prohibited, such as inline code or URLs

### Splitting Rules

| Condition | Action |
| --------- | ------ |
| **Japanese period `。`** | Always split |
| **Exclamation/Question marks (`！？!?`)** | If followed by whitespace/newline → split<br>If followed by non-whitespace → suppress split |
| **Single newline `\n`** | Suppress split (treat as soft wrap) |
| **Double newline `\n\n`** | Always split (treat as paragraph break) |
| **Within ignore ranges** | Do not split |

### Implementation Highlights

1. Get UAX #29-based boundaries via `unicode_sentences()`
2. Calculate byte offsets using pointer arithmetic
3. Apply Japanese heuristics in `should_split()`
4. Gaps (characters between segments) are merged into the preceding sentence

### Sentence Splitting Usage Example

```rust
use tsuzulint_text::SentenceSplitter;

let text = "こんにちは。世界。";
let sentences = SentenceSplitter::split(text, &[]);

for sentence in sentences {
    println!("{}: {:?}", sentence.text, sentence.span);
}
// Output:
// こんにちは。: 0..15
// 世界。: 15..24
```

### Code Block Protection

```rust
let text = "これは `code.` です。次の文。";
let ignore_ranges = vec![10..17]; // Range of `code.`
let sentences = SentenceSplitter::split(text, &ignore_ranges);
// Result: ["これは `code.` です。", "次の文。"]
```

## Japanese Heuristics Details

### Exclamation/Question Mark Handling

```text
Input: "すごい！！本当に！？"

UAX #29 only:
  ["すごい！！", "本当に！？"]  // Over-splitting

With Japanese heuristics:
  ["すごい！！本当に！？"]     // Properly merged
```

### Splitting After Whitespace

```text
Input: "すごい！！ 本当に！？"

Result:
  ["すごい！！", "本当に！？"]  // Split after whitespace
```

## Position in the Overall Project

```text
tsuzulint_core
    └── tsuzulint_parser     # Parser (Markdown/PlainText)
            └── tsuzulint_text  ← This crate
                    ├── Tokenization (for rules)
                    └── Sentence splitting (for sentence-level rules)
```

Text analysis results are used by:

- **Tokens**: Rules like "duplicate particles", "part of speech pattern checking"
- **Sentences**: Rules like "maximum sentence length", "sentence enumeration"

## Dependencies

| Dependency Crate | Purpose |
| ---------------- | ------- |
| **lindera** | Japanese morphological analysis. Embeds IPADIC dictionary with `embed-ipadic` feature |
| **unicode-segmentation** | UAX #29 compliant sentence boundary detection |
| **serde** | Serialize/deserialize `Token`, `Sentence` |
| **thiserror** | Custom error type definition |
| **miette** | User-friendly error display |

### Why These Dependencies?

- **lindera**: High-precision Japanese morphological analysis compatible with MeCab, implemented in Rust. IPADIC embedding eliminates the need for external dictionaries
- **unicode-segmentation**: UAX #29 compliant segmentation. `unicode_sentences()` provides sentence boundary detection
- **serde**: JSON serialization is required when passing AST nodes to WASM rules

## Error Type

```rust
pub enum TextError {
    TokenizeError(String),    // Tokenization error
}
```

## Public API

```rust
pub use splitter::{Sentence, SentenceSplitter};
pub use tokenizer::{TextError, Token, Tokenizer};
```
