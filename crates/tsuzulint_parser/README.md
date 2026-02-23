# tsuzulint_parser

A crate that provides a parser abstraction layer. Converts formats such as Markdown and plain text to TxtAST.

## Overview

`tsuzulint_parser` provides a **parser abstraction layer** for the TsuzuLint project. This crate's responsibilities are:

1. **Conversion to Text Abstract Syntax Tree (TxtAST)**: Parses source text and converts it to a unified AST format
2. **Foundation for custom parser implementation**: Through the `Parser` trait, makes it easy to support new file formats
3. **Separation of format-dependent components**: Decouples parser implementations from core linter logic, ensuring extensibility

## Architecture

```text
┌─────────────────────────────────────────────────────────────┐
│                     tsuzulint_parser                        │
│  ┌─────────────────┐  ┌─────────────────┐                   │
│  │   Parser trait  │  │   ParseError    │                   │
│  │  - name()       │  │  - InvalidSource│                   │
│  │  - extensions() │  │  - Unsupported  │                   │
│  │  - parse()      │  │  - Internal     │                   │
│  │  - can_parse()  │  └─────────────────┘                   │
│  └────────┬────────┘                                        │
│           │ implements                                      │
│  ┌────────┴────────┐                                        │
│  │                 │                                        │
│  ▼                 ▼                                        │
│ ┌───────────────┐ ┌───────────────┐                         │
│ │MarkdownParser │ │PlainTextParser│                         │
│ │               │ │               │                         │
│ │ - markdown-rs │ │ - Blank line  │                         │
│ │ - GFM support │ │ - Paragraph   │                         │
│ └───────┬───────┘ └───────┬───────┘                         │
└─────────┼─────────────────┼─────────────────────────────────┘
          │                 │
          ▼                 ▼
┌─────────────────────────────────────────────────────────────┐
│                      tsuzulint_ast                          │
│  ┌───────────┐  ┌──────────┐  ┌───────────┐  ┌───────────┐ │
│  │ AstArena  │  │ TxtNode  │  │ NodeType  │  │ NodeData  │ │
│  │ (bumpalo) │  │          │  │           │  │           │ │
│  └───────────┘  └──────────┘  └───────────┘  └───────────┘ │
└─────────────────────────────────────────────────────────────┘
```

## Parser Trait

```rust
pub trait Parser {
    /// Returns the parser name (e.g., "markdown", "text")
    fn name(&self) -> &str;

    /// Returns supported file extensions (without dots, e.g., ["md", "markdown"])
    fn extensions(&self) -> &[&str];

    /// Converts source text to TxtAST
    fn parse<'a>(&self, arena: &'a AstArena, source: &str) -> Result<TxtNode<'a>, ParseError>;

    /// Determines if the specified extension can be processed (default implementation, case-insensitive)
    fn can_parse(&self, extension: &str) -> bool {
        self.extensions().iter().any(|ext| ext.eq_ignore_ascii_case(extension))
    }
}
```

**Design Points:**

- The `parse` method takes an Arena allocator (`AstArena`) as an argument and allocates AST nodes on that arena
- This allows O(1) memory deallocation after parsing is complete
- `can_parse` provides a default implementation with case-insensitive matching

## Built-in Parsers

### MarkdownParser

**Library Used**: `markdown-rs` (wooorm/markdown-rs)

**Supported Extensions**: `["md", "markdown", "mdown", "mkdn", "mkd"]`

**Parse Options**: GFM (GitHub Flavored Markdown) by default

**Mapping between supported mdast nodes and TxtAST NodeType:**

| mdast Node | TxtAST NodeType | Description |
| ---------- | --------------- | ----------- |
| Root | Document | Document root |
| Paragraph | Paragraph | Paragraph |
| Heading | Header | Heading (with depth info) |
| Text | Str | Text |
| Emphasis | Emphasis | Italic |
| Strong | Strong | Bold |
| InlineCode | Code | Inline code |
| Code | CodeBlock | Code block (with language info) |
| Link | Link | Link (with URL/title) |
| Image | Image | Image (with URL/title) |
| List | List | List (ordered/unordered) |
| ListItem | ListItem | List item |
| Blockquote | BlockQuote | Blockquote |
| ThematicBreak | HorizontalRule | Horizontal rule |
| Break | Break | Line break |
| Html | Html | HTML element |
| Delete | Delete | Strikethrough (GFM) |
| Table | Table | Table (GFM) |
| TableRow | TableRow | Table row |
| TableCell | TableCell | Table cell |
| FootnoteDefinition | FootnoteDefinition | Footnote definition (GFM) |
| FootnoteReference | FootnoteReference | Footnote reference (GFM) |
| LinkReference | LinkReference | Link reference |
| ImageReference | ImageReference | Image reference |
| Definition | Definition | Definition |

### PlainTextParser

**Supported Extensions**: `["txt", "text"]`

**Parsing Logic:**

- Splits text into paragraphs by blank lines
- Each paragraph is represented as a `Paragraph` node
- Text within paragraphs is represented as a `Str` node

**Output AST Structure:**

```text
Document
└── Paragraph (each paragraph)
    └── Str (text)
```

## Usage Examples

### Basic Usage

```rust
use tsuzulint_parser::{MarkdownParser, Parser};
use tsuzulint_ast::AstArena;

let parser = MarkdownParser::new();
let arena = AstArena::new();

let source = "# Hello\n\nThis is **bold** text.";
let ast = parser.parse(&arena, source)?;

// Traverse the AST
for child in ast.children {
    println!("Node type: {:?}", child.node_type);
}
```

### Parser Selection by Extension

```rust
use tsuzulint_parser::{MarkdownParser, PlainTextParser, Parser};

fn get_parser(extension: &str) -> Box<dyn Parser> {
    let md = MarkdownParser::new();
    let txt = PlainTextParser::new();
    
    if md.can_parse(extension) {
        Box::new(md)
    } else {
        Box::new(txt)
    }
}
```

### Implementing a Custom Parser

```rust
use tsuzulint_parser::{Parser, ParseError};
use tsuzulint_ast::{AstArena, TxtNode, NodeType, Span, NodeData};

struct YamlParser;

impl Parser for YamlParser {
    fn name(&self) -> &str {
        "yaml"
    }

    fn extensions(&self) -> &[&str] {
        &["yaml", "yml"]
    }

    fn parse<'a>(
        &self,
        arena: &'a AstArena,
        source: &str,
    ) -> Result<TxtNode<'a>, ParseError> {
        // Implement YAML parsing logic
        // ...
    }
}
```

## Dependencies

| Crate | Purpose |
| ----- | ------- |
| `tsuzulint_ast` | AST type definitions, Arena allocator, NodeType, etc. |
| `markdown` | Markdown parsing (mdast-compatible output) |
| `thiserror` | Error type definition (derive macro) |

## Error Handling

```rust
pub enum ParseError {
    /// Invalid source text
    InvalidSource { message: String, offset: Option<usize> },
    /// Unsupported feature
    Unsupported(String),
    /// Internal parser error
    Internal(String),
}
```

- Error definitions using the `thiserror` crate
- Provides detailed error information including error location (byte offset)

## Module Structure

```text
src/
├── lib.rs        # Public API, crate documentation
├── traits.rs     # Parser trait definition
├── error.rs      # ParseError type
├── markdown.rs   # MarkdownParser implementation
└── text.rs       # PlainTextParser implementation
```
