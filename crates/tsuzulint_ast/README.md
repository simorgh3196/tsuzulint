# tsuzulint_ast

A crate providing TxtAST (textlint AST) type definitions and an Arena allocator.

## Overview

`tsuzulint_ast` provides **AST (Abstract Syntax Tree) type definitions** that form the core of the TsuzuLint project. It maintains compatibility with textlint's TxtAST specification while offering an implementation optimized for Rust's memory model.

## Key Features

- **TxtAST Type Definitions**: AST representation for natural language text
- **Arena Allocation**: High-performance memory management using bumpalo
- **Visitor Pattern**: Traits for AST traversal and transformation
- **JSON Serialization**: Data exchange with WASM rules

## Architecture

### Arena Allocation (bumpalo)

Arena allocation inspired by Oxc:

```rust
pub struct AstArena {
    bump: Bump,  // bumpalo wrapper
}
```

**Benefits:**

1. **Minimized Allocation Overhead** - Bump allocation is extremely fast
2. **Improved Cache Locality** - All nodes of a single file are placed in contiguous memory
3. **Bulk Deallocation** - Release all memory at once with `reset()`
4. **Zero-Copy Strings** - Directly reference source text with `alloc_str()`

### Core Types

#### `TxtNode<'a>` - Core AST Node Type

```rust
pub struct TxtNode<'a> {
    pub node_type: NodeType,          // Node type
    pub span: Span,                   // Byte position in source text
    pub children: &'a [TxtNode<'a>],  // Child nodes (arena reference)
    pub value: Option<&'a str>,       // Text value (for Str, Code, CodeBlock)
    pub data: NodeData<'a>,           // Additional data
}
```

- Lifetime `'a` ties the node to the arena allocator
- Implements `Copy` trait (copying is cheap)
- Three constructors: `new_parent()`, `new_text()`, `new_leaf()`

#### `NodeType` - Node Type Enumeration

```rust
pub enum NodeType {
    // Document structure
    Document, Paragraph, Header, BlockQuote, List, ListItem,
    CodeBlock, HorizontalRule, Html,
    // Inline elements
    Str, Break, Emphasis, Strong, Delete, Code, Link, Image,
    // Reference elements (textlint v14.5.0+)
    LinkReference, ImageReference, Definition,
    // GFM extensions
    Table, TableRow, TableCell,
    FootnoteDefinition, FootnoteReference,
}
```

#### `NodeData<'a>` - Node-Specific Data

Achieves **over 30% memory reduction** compared to traditional struct approaches:

```rust
pub enum NodeData<'a> {
    None,                                    // No data
    Header(u8),                              // Heading level (1-6)
    List(bool),                              // ordered flag
    CodeBlock(Option<&'a str>),              // Language specification
    Link(LinkData<'a>),                      // URL + title
    Image(LinkData<'a>),
    Reference(ReferenceData<'a>),            // Identifier + label
    Definition(DefinitionData<'a>),
}
```

### Visitor Pattern

#### `Visitor<'a>` - Read-Only Traversal

```rust
pub trait Visitor<'a>: Sized {
    fn enter_node(&mut self, node: &TxtNode<'a>) -> VisitResult;
    fn exit_node(&mut self, node: &TxtNode<'a>) -> VisitResult;
    fn visit_str(&mut self, node: &TxtNode<'a>) -> VisitResult;
    // ... visit_* methods for each node type
}
```

**Control Flow:**
- `ControlFlow::Continue(())` - Continue traversing child nodes
- `ControlFlow::Break(())` - Early termination (propagated via `?` operator)

#### `MutVisitor<'a>` - AST Transformation

```rust
pub trait MutVisitor<'a>: Sized {
    fn arena(&self) -> &'a AstArena;  // For allocating new nodes
    fn visit_str_mut(&mut self, node: &TxtNode<'a>) -> VisitMutResult<'a>;
    // ... for all other node types
}
```

## Usage Examples

```rust
use tsuzulint_ast::{AstArena, TxtNode, NodeType, Span, NodeData};

// Create arena
let arena = AstArena::new();

// Create text node
let text = arena.alloc(TxtNode::new_text(
    NodeType::Str,
    Span::new(0, 5),
    arena.alloc_str("hello"),
));

// Create child node slice
let children = arena.alloc_slice_copy(&[*text]);

// Create parent node
let paragraph = arena.alloc(TxtNode::new_parent(
    NodeType::Paragraph,
    Span::new(0, 5),
    children,
    NodeData::None,
));
```

### Visitor Usage Example

```rust
use tsuzulint_ast::{Visitor, TxtNode, walk_node};
use std::ops::ControlFlow;

struct TextCollector<'a> {
    texts: Vec<&'a str>,
}

impl<'a> Visitor<'a> for TextCollector<'a> {
    fn visit_str(&mut self, node: &TxtNode<'a>) -> ControlFlow<()> {
        if let Some(text) = node.value {
            self.texts.push(text);
        }
        ControlFlow::Continue(())
    }
}
```

## Dependencies

| Dependency | Purpose |
| ---------- | ------- |
| **bumpalo** | Arena allocation |
| **serde** | Serialization (JSON exchange with WASM rules) |
| **rkyv** (optional) | Zero-copy serialization (for caching) |

## Compatibility with textlint TxtAST

Conforms to TxtAST specification:

- `type` field: Node type (PascalCase)
- `range` field: `[start, end]` byte offsets
- `children` field: Child node array (parent nodes only)
- `value` field: Text value (text nodes only)
- Additional fields: `depth`, `ordered`, `lang`, `url`, `title`, `identifier`, `label`

## Module Structure

```text
src/
├── lib.rs           # Public API, crate documentation
├── arena.rs         # AstArena (bumpalo wrapper)
├── node.rs          # TxtNode, NodeData, LinkData, etc.
├── node_type.rs     # NodeType enumeration
├── span.rs          # Span, Position, Location
└── visitor/
    ├── mod.rs       # Visitor module definition
    ├── visit.rs     # Visitor trait (read-only)
    ├── visit_mut.rs # MutVisitor trait (transformation)
    └── walk.rs      # walk_node, walk_children functions
```
