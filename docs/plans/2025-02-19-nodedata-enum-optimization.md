# NodeData Enum Optimization Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Convert `NodeData` struct (88 bytes) to an enum (≈24 bytes) for memory efficiency while maintaining equivalent functionality.

**Architecture:** Replace the current `NodeData` struct with 7 `Option` fields with a tagged union (enum) that stores only the relevant data for each node type. Auxiliary structs (`LinkData`, `ReferenceData`, `DefinitionData`) are inlined in enum variants for zero-allocation overhead.

**Tech Stack:** Rust, serde, bumpalo arena allocator

---

## Background

Current `NodeData` struct size breakdown:
- 7 `Option<&str>` = 7 × 16 bytes = 112 bytes (but with niches, actual ~88 bytes)
- Most nodes only need 0-2 fields

Proposed enum size:
- 1 byte discriminant + 8-16 bytes data = ~24 bytes max
- `None` variant: 0 bytes (niche optimization)

---

## Task 1: Define New Enum and Helper Structs

**Files:**
- Modify: `crates/tsuzulint_ast/src/node.rs:59-89`

**Step 1: Write the failing test for enum size**

Add to `crates/tsuzulint_ast/src/node.rs` tests:

```rust
#[test]
fn test_nodedata_size_optimization() {
    use std::mem::size_of;
    
    let old_size = 7 * size_of::<Option<&str>>();
    let new_size = size_of::<NodeData>();
    
    assert!(new_size <= old_size / 2, 
        "NodeData should be at least 50% smaller: was {} bytes, now {} bytes", 
        old_size, new_size);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p tsuzulint_ast test_nodedata_size_optimization`
Expected: FAIL (NodeData doesn't exist as enum yet)

**Step 3: Define the new enum and helper structs**

Replace `crates/tsuzulint_ast/src/node.rs:59-89` with:

```rust
#[derive(Debug, Clone, Copy, Default)]
pub enum NodeData<'a> {
    #[default]
    None,
    Header(u8),
    List(bool),
    CodeBlock(Option<&'a str>),
    Link(LinkData<'a>),
    Reference(ReferenceData<'a>),
    Definition(DefinitionData<'a>),
}

#[derive(Debug, Clone, Copy)]
pub struct LinkData<'a> {
    pub url: &'a str,
    pub title: Option<&'a str>,
}

#[derive(Debug, Clone, Copy)]
pub struct ReferenceData<'a> {
    pub identifier: &'a str,
    pub label: Option<&'a str>,
}

#[derive(Debug, Clone, Copy)]
pub struct DefinitionData<'a> {
    pub identifier: &'a str,
    pub url: &'a str,
    pub title: Option<&'a str>,
    pub label: Option<&'a str>,
}
```

**Step 4: Run test to verify enum compiles**

Run: `cargo check -p tsuzulint_ast`
Expected: Compilation errors in other files (expected)

**Step 5: Commit**

```bash
git add crates/tsuzulint_ast/src/node.rs
git commit -m "refactor(ast): define NodeData enum and helper structs"
```

---

## Task 2: Implement Serialize for NodeData Enum

**Files:**
- Modify: `crates/tsuzulint_ast/src/node.rs`

**Step 1: Write failing serialization test**

Add to tests:

```rust
#[test]
fn test_nodedata_enum_serialization_header() {
    let data = NodeData::Header(2);
    let json = serde_json::to_value(&data).unwrap();
    assert_eq!(json["depth"], 2);
}

#[test]
fn test_nodedata_enum_serialization_link() {
    let data = NodeData::Link(LinkData {
        url: "https://example.com",
        title: Some("Example"),
    });
    let json = serde_json::to_value(&data).unwrap();
    assert_eq!(json["url"], "https://example.com");
    assert_eq!(json["title"], "Example");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p tsuzulint_ast test_nodedata_enum_serialization`
Expected: FAIL (Serialize not implemented)

**Step 3: Implement custom Serialize**

Add after enum definition:

```rust
impl<'a> Serialize for NodeData<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        
        match self {
            NodeData::None => serializer.serialize_none(),
            NodeData::Header(depth) => {
                let mut s = serializer.serialize_struct("NodeData", 1)?;
                s.serialize_field("depth", depth)?;
                s.end()
            }
            NodeData::List(ordered) => {
                let mut s = serializer.serialize_struct("NodeData", 1)?;
                s.serialize_field("ordered", ordered)?;
                s.end()
            }
            NodeData::CodeBlock(lang) => {
                if let Some(lang) = lang {
                    let mut s = serializer.serialize_struct("NodeData", 1)?;
                    s.serialize_field("lang", lang)?;
                    s.end()
                } else {
                    serializer.serialize_none()
                }
            }
            NodeData::Link(link) => {
                let mut len = 1;
                if link.title.is_some() {
                    len += 1;
                }
                let mut s = serializer.serialize_struct("NodeData", len)?;
                s.serialize_field("url", link.url)?;
                if let Some(title) = link.title {
                    s.serialize_field("title", title)?;
                }
                s.end()
            }
            NodeData::Reference(ref_data) => {
                let mut len = 1;
                if ref_data.label.is_some() {
                    len += 1;
                }
                let mut s = serializer.serialize_struct("NodeData", len)?;
                s.serialize_field("identifier", ref_data.identifier)?;
                if let Some(label) = ref_data.label {
                    s.serialize_field("label", label)?;
                }
                s.end()
            }
            NodeData::Definition(def) => {
                let mut len = 2;
                if def.title.is_some() {
                    len += 1;
                }
                if def.label.is_some() {
                    len += 1;
                }
                let mut s = serializer.serialize_struct("NodeData", len)?;
                s.serialize_field("identifier", def.identifier)?;
                s.serialize_field("url", def.url)?;
                if let Some(title) = def.title {
                    s.serialize_field("title", title)?;
                }
                if let Some(label) = def.label {
                    s.serialize_field("label", label)?;
                }
                s.end()
            }
        }
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p tsuzulint_ast test_nodedata_enum_serialization`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/tsuzulint_ast/src/node.rs
git commit -m "feat(ast): implement Serialize for NodeData enum"
```

---

## Task 3: Implement Constructor Methods for NodeData

**Files:**
- Modify: `crates/tsuzulint_ast/src/node.rs`

**Step 1: Write failing tests for constructors**

```rust
#[test]
fn test_nodedata_enum_header() {
    let data = NodeData::header(2);
    assert!(matches!(data, NodeData::Header(2)));
}

#[test]
fn test_nodedata_enum_link() {
    let data = NodeData::link("https://example.com", Some("Example"));
    match data {
        NodeData::Link(link) => {
            assert_eq!(link.url, "https://example.com");
            assert_eq!(link.title, Some("Example"));
        }
        _ => panic!("Expected Link variant"),
    }
}

#[test]
fn test_nodedata_enum_list() {
    let data = NodeData::list(true);
    assert!(matches!(data, NodeData::List(true)));
}

#[test]
fn test_nodedata_enum_code_block() {
    let data = NodeData::code_block(Some("rust"));
    assert!(matches!(data, NodeData::CodeBlock(Some("rust"))));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p tsuzulint_ast test_nodedata_enum_`
Expected: FAIL (methods not implemented)

**Step 3: Implement constructor methods**

Replace existing `impl<'a> NodeData<'a>` block with:

```rust
impl<'a> NodeData<'a> {
    pub const fn new() -> Self {
        NodeData::None
    }

    pub const fn header(depth: u8) -> Self {
        NodeData::Header(depth)
    }

    pub const fn link(url: &'a str, title: Option<&'a str>) -> Self {
        NodeData::Link(LinkData { url, title })
    }

    pub const fn code_block(lang: Option<&'a str>) -> Self {
        NodeData::CodeBlock(lang)
    }

    pub const fn list(ordered: bool) -> Self {
        NodeData::List(ordered)
    }

    pub const fn reference(identifier: &'a str, label: Option<&'a str>) -> Self {
        NodeData::Reference(ReferenceData { identifier, label })
    }

    pub const fn definition(
        identifier: &'a str,
        url: &'a str,
        title: Option<&'a str>,
        label: Option<&'a str>,
    ) -> Self {
        NodeData::Definition(DefinitionData {
            identifier,
            url,
            title,
            label,
        })
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p tsuzulint_ast test_nodedata_enum_`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/tsuzulint_ast/src/node.rs
git commit -m "feat(ast): add constructor methods for NodeData enum"
```

---

## Task 4: Update TxtNode Serialization

**Files:**
- Modify: `crates/tsuzulint_ast/src/node.rs:91-124`

**Step 1: Write failing test for TxtNode serialization**

```rust
#[test]
fn test_txtnode_with_enum_data_serialization() {
    let mut node = TxtNode::new_parent(NodeType::Header, Span::new(0, 10), &[]);
    node.data = NodeData::header(2);
    
    let json = serde_json::to_value(&node).unwrap();
    assert_eq!(json["type"], "Header");
    assert_eq!(json["depth"], 2);
}
```

**Step 2: Update TxtNode Serialize implementation**

Modify `impl<'a> Serialize for TxtNode<'a>` to handle the new enum:

```rust
impl<'a> Serialize for TxtNode<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let mut len = 2; // type, range
        if self.node_type.is_parent() || !self.children.is_empty() {
            len += 1;
        }
        if self.value.is_some() {
            len += 1;
        }
        if !matches!(self.data, NodeData::None) {
            len += self.data.field_count();
        }

        let mut state = serializer.serialize_struct("TxtNode", len)?;

        state.serialize_field("type", &self.node_type)?;
        state.serialize_field("range", &[self.span.start, self.span.end])?;

        if self.node_type.is_parent() || !self.children.is_empty() {
            state.serialize_field("children", &self.children)?;
        }

        if let Some(value) = &self.value {
            state.serialize_field("value", value)?;
        }

        self.data.serialize_into(&mut state)?;

        state.end()
    }
}
```

**Step 3: Add field_count and serialize_into to NodeData**

```rust
impl<'a> NodeData<'a> {
    fn field_count(&self) -> usize {
        match self {
            NodeData::None => 0,
            NodeData::Header(_) => 1,
            NodeData::List(_) => 1,
            NodeData::CodeBlock(lang) => if lang.is_some() { 1 } else { 0 },
            NodeData::Link(link) => if link.title.is_some() { 2 } else { 1 },
            NodeData::Reference(ref_data) => if ref_data.label.is_some() { 2 } else { 1 },
            NodeData::Definition(def) => {
                let mut count = 2;
                if def.title.is_some() { count += 1; }
                if def.label.is_some() { count += 1; }
                count
            }
        }
    }

    fn serialize_into<S: serde::ser::SerializeStruct>(
        &self,
        state: &mut S,
    ) -> Result<(), S::Error> {
        match self {
            NodeData::None => {}
            NodeData::Header(depth) => {
                state.serialize_field("depth", depth)?;
            }
            NodeData::List(ordered) => {
                state.serialize_field("ordered", ordered)?;
            }
            NodeData::CodeBlock(lang) => {
                if let Some(lang) = lang {
                    state.serialize_field("lang", lang)?;
                }
            }
            NodeData::Link(link) => {
                state.serialize_field("url", link.url)?;
                if let Some(title) = link.title {
                    state.serialize_field("title", title)?;
                }
            }
            NodeData::Reference(ref_data) => {
                state.serialize_field("identifier", ref_data.identifier)?;
                if let Some(label) = ref_data.label {
                    state.serialize_field("label", label)?;
                }
            }
            NodeData::Definition(def) => {
                state.serialize_field("identifier", def.identifier)?;
                state.serialize_field("url", def.url)?;
                if let Some(title) = def.title {
                    state.serialize_field("title", title)?;
                }
                if let Some(label) = def.label {
                    state.serialize_field("label", label)?;
                }
            }
        }
        Ok(())
    }
}
```

**Step 4: Run tests**

Run: `cargo test -p tsuzulint_ast`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/tsuzulint_ast/src/node.rs
git commit -m "refactor(ast): update TxtNode serialization for NodeData enum"
```

---

## Task 5: Update Parser (tsuzulint_parser)

**Files:**
- Modify: `crates/tsuzulint_parser/src/markdown.rs`

**Step 1: Update imports**

Change line 7 from:
```rust
use tsuzulint_ast::{AstArena, NodeData, NodeType, Span, TxtNode};
```
To:
```rust
use tsuzulint_ast::{AstArena, NodeData, NodeType, Span, TxtNode};
```
(No change needed, but verify LinkData, ReferenceData, DefinitionData are available)

**Step 2: Update Header node creation**

In `convert_node`, line 50-59:
```rust
Node::Heading(heading) => {
    let mut node = self.create_parent_node(
        arena,
        node,
        &heading.children,
        source,
        NodeType::Header,
    );
    node.data = NodeData::header(heading.depth);
    node
}
```
(No change needed - constructor already returns correct type)

**Step 3: Update Link node creation**

Lines 87-94:
```rust
Node::Link(link) => {
    let mut node =
        self.create_parent_node(arena, node, &link.children, source, NodeType::Link);
    let url = arena.alloc_str(&link.url);
    let title = link.title.as_ref().map(|t| arena.alloc_str(t));
    node.data = NodeData::link(url, title);
    node
}
```
(No change needed)

**Step 4: Update Image node creation**

Lines 96-102:
```rust
Node::Image(image) => {
    let mut node = self.create_leaf_node(node, source, NodeType::Image);
    let url = arena.alloc_str(&image.url);
    let title = image.title.as_ref().map(|t| arena.alloc_str(t));
    node.data = NodeData::link(url, title);
    node
}
```
(No change needed)

**Step 5: Update CodeBlock node creation**

Lines 78-85:
```rust
Node::Code(code) => {
    let mut node =
        self.create_text_node(arena, node, &code.value, source, NodeType::CodeBlock);
    if let Some(lang) = &code.lang {
        node.data = NodeData::code_block(Some(arena.alloc_str(lang)));
    }
    node
}
```
(No change needed)

**Step 6: Update List node creation**

Lines 104-109:
```rust
Node::List(list) => {
    let mut node =
        self.create_parent_node(arena, node, &list.children, source, NodeType::List);
    node.data = NodeData::list(list.ordered);
    node
}
```
(No change needed)

**Step 7: Update FootnoteDefinition**

Lines 145-158, replace:
```rust
node.data.identifier = Some(arena.alloc_str(&def.identifier));
if let Some(label) = &def.label {
    node.data.label = Some(arena.alloc_str(label));
}
```
With:
```rust
node.data = NodeData::reference(
    arena.alloc_str(&def.identifier),
    def.label.as_ref().map(|l| arena.alloc_str(l)),
);
```

**Step 8: Update FootnoteReference**

Lines 160-167, replace:
```rust
node.data.identifier = Some(arena.alloc_str(&ref_node.identifier));
if let Some(label) = &ref_node.label {
    node.data.label = Some(arena.alloc_str(label));
}
```
With:
```rust
node.data = NodeData::reference(
    arena.alloc_str(&ref_node.identifier),
    ref_node.label.as_ref().map(|l| arena.alloc_str(l)),
);
```

**Step 9: Update LinkReference**

Lines 170-183, replace:
```rust
node.data.identifier = Some(arena.alloc_str(&ref_node.identifier));
if let Some(label) = &ref_node.label {
    node.data.label = Some(arena.alloc_str(label));
}
```
With:
```rust
node.data = NodeData::reference(
    arena.alloc_str(&ref_node.identifier),
    ref_node.label.as_ref().map(|l| arena.alloc_str(l)),
);
```

**Step 10: Update ImageReference**

Lines 185-192, replace:
```rust
node.data.identifier = Some(arena.alloc_str(&ref_node.identifier));
if let Some(label) = &ref_node.label {
    node.data.label = Some(arena.alloc_str(label));
}
```
With:
```rust
node.data = NodeData::reference(
    arena.alloc_str(&ref_node.identifier),
    ref_node.label.as_ref().map(|l| arena.alloc_str(l)),
);
```

**Step 11: Update Definition**

Lines 194-205, replace:
```rust
node.data.identifier = Some(arena.alloc_str(&def.identifier));
node.data.url = Some(arena.alloc_str(&def.url));
if let Some(title) = &def.title {
    node.data.title = Some(arena.alloc_str(title));
}
if let Some(label) = &def.label {
    node.data.label = Some(arena.alloc_str(label));
}
```
With:
```rust
node.data = NodeData::definition(
    arena.alloc_str(&def.identifier),
    arena.alloc_str(&def.url),
    def.title.as_ref().map(|t| arena.alloc_str(t)),
    def.label.as_ref().map(|l| arena.alloc_str(l)),
);
```

**Step 12: Update tests that access data fields**

Update tests like `test_parse_heading` (lines 324-327):
```rust
assert_eq!(ast.children[0].data.depth(), Some(1));
```

Wait - we need accessor methods first. Skip this step for now and update after Task 6.

**Step 13: Run tests**

Run: `cargo test -p tsuzulint_parser`
Expected: Some failures due to test field access

**Step 14: Commit**

```bash
git add crates/tsuzulint_parser/src/markdown.rs
git commit -m "refactor(parser): update NodeData usage to enum"
```

---

## Task 6: Add Accessor Methods to TxtNode

**Files:**
- Modify: `crates/tsuzulint_ast/src/node.rs`

**Step 1: Write failing tests for accessors**

```rust
#[test]
fn test_txtnode_depth_accessor() {
    let mut node = TxtNode::new_parent(NodeType::Header, Span::new(0, 10), &[]);
    node.data = NodeData::header(2);
    assert_eq!(node.depth(), Some(2));
    
    let leaf = TxtNode::new_leaf(NodeType::HorizontalRule, Span::new(0, 3));
    assert_eq!(leaf.depth(), None);
}

#[test]
fn test_txtnode_url_accessor() {
    let mut node = TxtNode::new_parent(NodeType::Link, Span::new(0, 10), &[]);
    node.data = NodeData::link("https://example.com", None);
    assert_eq!(node.url(), Some("https://example.com"));
}

#[test]
fn test_txtnode_ordered_accessor() {
    let mut node = TxtNode::new_parent(NodeType::List, Span::new(0, 10), &[]);
    node.data = NodeData::list(true);
    assert_eq!(node.ordered(), Some(true));
}
```

**Step 2: Add accessor methods to TxtNode**

Add to `impl<'a> TxtNode<'a>`:

```rust
pub fn depth(&self) -> Option<u8> {
    match self.data {
        NodeData::Header(d) => Some(d),
        _ => None,
    }
}

pub fn url(&self) -> Option<&'a str> {
    match &self.data {
        NodeData::Link(link) => Some(link.url),
        NodeData::Definition(def) => Some(def.url),
        _ => None,
    }
}

pub fn title(&self) -> Option<&'a str> {
    match &self.data {
        NodeData::Link(link) => link.title,
        NodeData::Definition(def) => def.title,
        _ => None,
    }
}

pub fn ordered(&self) -> Option<bool> {
    match self.data {
        NodeData::List(o) => Some(o),
        _ => None,
    }
}

pub fn lang(&self) -> Option<&'a str> {
    match self.data {
        NodeData::CodeBlock(lang) => lang,
        _ => None,
    }
}

pub fn identifier(&self) -> Option<&'a str> {
    match &self.data {
        NodeData::Reference(ref_data) => Some(ref_data.identifier),
        NodeData::Definition(def) => Some(def.identifier),
        _ => None,
    }
}

pub fn label(&self) -> Option<&'a str> {
    match &self.data {
        NodeData::Reference(ref_data) => ref_data.label,
        NodeData::Definition(def) => def.label,
        _ => None,
    }
}
```

**Step 3: Run tests**

Run: `cargo test -p tsuzulint_ast test_txtnode_`
Expected: PASS

**Step 4: Commit**

```bash
git add crates/tsuzulint_ast/src/node.rs
git commit -m "feat(ast): add accessor methods to TxtNode for NodeData fields"
```

---

## Task 7: Update Parser Tests

**Files:**
- Modify: `crates/tsuzulint_parser/src/markdown.rs` (tests section)

**Step 1: Update test assertions**

Replace direct field access with accessor methods:

Line 325-327:
```rust
assert_eq!(ast.children[0].data.depth, Some(1));
assert_eq!(ast.children[1].data.depth, Some(2));
```
To:
```rust
assert_eq!(ast.children[0].depth(), Some(1));
assert_eq!(ast.children[1].depth(), Some(2));
```

Line 343:
```rust
assert_eq!(link.data.url, Some("https://example.com"));
```
To:
```rust
assert_eq!(link.url(), Some("https://example.com"));
```

Line 403:
```rust
assert_eq!(code_block.data.lang, Some("rust"));
```
To:
```rust
assert_eq!(code_block.lang(), Some("rust"));
```

Line 417:
```rust
assert!(code_block.data.lang.is_none());
```
To:
```rust
assert!(code_block.lang().is_none());
```

Lines 458, 476, 502-503, 518-519, 579:
Similar updates for `ordered()`, `url()`, `title()`, `depth()`.

**Step 2: Run tests**

Run: `cargo test -p tsuzulint_parser`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/tsuzulint_parser/src/markdown.rs
git commit -m "refactor(parser): update tests to use TxtNode accessors"
```

---

## Task 8: Update Visitor Tests

**Files:**
- Modify: `crates/tsuzulint_ast/src/visitor/mod.rs`
- Modify: `crates/tsuzulint_ast/src/visitor/visit_mut.rs`

**Step 1: Update visitor/mod.rs example**

Line 57:
```rust
self.found_depth = node.data.depth;
```
To:
```rust
self.found_depth = node.depth();
```

**Step 2: Update visitor/visit_mut.rs HeaderDepthAdjuster**

Lines 392-394:
```rust
if let Some(depth) = new_node.data.depth {
    let new_depth = (depth as i8 + self.offset).clamp(1, 6) as u8;
    new_node.data.depth = Some(new_depth);
}
```
To:
```rust
if let Some(depth) = new_node.depth() {
    let new_depth = (depth as i8 + self.offset).clamp(1, 6) as u8;
    new_node.data = NodeData::header(new_depth);
}
```

Lines 408, 429:
```rust
header.data.depth = Some(1);
```
To:
```rust
header.data = NodeData::header(1);
```

**Step 3: Add mutable accessor for depth**

Add to TxtNode (we need this for the mut visitor):

Actually, since TxtNode has `pub data`, we can just assign directly. The tests should work.

**Step 4: Run tests**

Run: `cargo test -p tsuzulint_ast`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/tsuzulint_ast/src/visitor/
git commit -m "refactor(ast): update visitor to use NodeData enum"
```

---

## Task 9: Update WASM Tests

**Files:**
- Modify: `crates/tsuzulint_wasm/src/lib.rs`

**Step 1: Update test test_ast_to_json_with_node_data**

Lines 432-437:
```rust
node.data.depth = Some(2);
node.data.url = Some(arena.alloc_str("https://example.com"));
node.data.title = Some(arena.alloc_str("Example"));
node.data.lang = Some(arena.alloc_str("rust"));
node.data.ordered = Some(true);
```
To:
```rust
// Create separate nodes for each data type to test serialization
let mut header_node = TxtNode::new_parent(NodeType::Header, Span::new(0, 10), &[]);
header_node.data = NodeData::header(2);

let mut link_node = TxtNode::new_parent(NodeType::Link, Span::new(0, 10), &[]);
link_node.data = NodeData::link(
    arena.alloc_str("https://example.com"),
    Some(arena.alloc_str("Example")),
);

let mut code_node = TxtNode::new_text(NodeType::CodeBlock, Span::new(0, 10), "code");
code_node.data = NodeData::code_block(Some(arena.alloc_str("rust")));

let mut list_node = TxtNode::new_parent(NodeType::List, Span::new(0, 10), &[]);
list_node.data = NodeData::list(true);
```

**Step 2: Update test assertions**

```rust
let header_json = serde_json::to_value(&header_node).unwrap();
assert_eq!(header_json["depth"], 2);

let link_json = serde_json::to_value(&link_node).unwrap();
assert_eq!(link_json["url"], "https://example.com");
assert_eq!(link_json["title"], "Example");

let code_json = serde_json::to_value(&code_node).unwrap();
assert_eq!(code_json["lang"], "rust");

let list_json = serde_json::to_value(&list_node).unwrap();
assert_eq!(list_json["ordered"], true);
```

**Step 3: Run tests**

Run: `cargo test -p tsuzulint_wasm`
Expected: PASS

**Step 4: Commit**

```bash
git add crates/tsuzulint_wasm/src/lib.rs
git commit -m "refactor(wasm): update tests for NodeData enum"
```

---

## Task 10: Update tsuzulint_ast Node Tests

**Files:**
- Modify: `crates/tsuzulint_ast/src/node.rs` (tests section)

**Step 1: Update existing tests**

Update tests that use the old struct API:

- `test_node_data_header`: Use `NodeData::header(2)` and pattern match
- `test_node_data_link`: Use `NodeData::link(...)` and pattern match
- `test_node_data_code_block`: Use `NodeData::code_block(...)`
- `test_node_data_list_ordered/unordered`: Use `NodeData::list(...)`
- `test_node_data_new_empty`: Use `NodeData::new()` (returns `None`)

**Step 2: Update serialization tests**

- `test_serialization_flattened_data`: Update to use new enum constructors
- `test_serialization_all_fields_len`: Split into separate tests per variant

**Step 3: Run all tests**

Run: `cargo test -p tsuzulint_ast`
Expected: PASS

**Step 4: Commit**

```bash
git add crates/tsuzulint_ast/src/node.rs
git commit -m "refactor(ast): update node tests for NodeData enum"
```

---

## Task 11: Export Helper Structs

**Files:**
- Modify: `crates/tsuzulint_ast/src/lib.rs`

**Step 1: Export new types**

Add to exports:
```rust
pub use node::{LinkData, NodeData, ReferenceData, DefinitionData, TxtNode};
```

**Step 2: Run full test suite**

Run: `cargo test --workspace`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/tsuzulint_ast/src/lib.rs
git commit -m "feat(ast): export LinkData, ReferenceData, DefinitionData"
```

---

## Task 12: Final Verification

**Step 1: Run lint and format**

Run: `make lint && make fmt-check`
Expected: PASS

**Step 2: Run full test suite**

Run: `make test`
Expected: PASS

**Step 3: Verify memory improvement**

Add temporary test to check sizes:
```rust
#[test]
fn verify_memory_improvement() {
    use std::mem::size_of;
    
    println!("NodeData size: {} bytes", size_of::<NodeData>());
    println!("LinkData size: {} bytes", size_of::<LinkData>());
    println!("ReferenceData size: {} bytes", size_of::<ReferenceData>());
    println!("DefinitionData size: {} bytes", size_of::<DefinitionData>());
    
    assert!(size_of::<NodeData>() <= 32, "NodeData should be <= 32 bytes");
}
```

Run: `cargo test -p tsuzulint_ast verify_memory_improvement -- --nocapture`

**Step 4: Final commit**

```bash
git add -A
git commit -m "feat(ast): complete NodeData enum optimization"
```

---

## Summary of Changes

| File | Change |
|------|--------|
| `tsuzulint_ast/src/node.rs` | Define enum, helper structs, Serialize, accessors |
| `tsuzulint_ast/src/lib.rs` | Export new types |
| `tsuzulint_ast/src/visitor/mod.rs` | Update example |
| `tsuzulint_ast/src/visitor/visit_mut.rs` | Update HeaderDepthAdjuster |
| `tsuzulint_parser/src/markdown.rs` | Update node construction and tests |
| `tsuzulint_wasm/src/lib.rs` | Update tests |

**Memory Impact:**
- Before: ~88 bytes per NodeData
- After: ~16-24 bytes per NodeData
- Savings: ~65-72%

**Breaking Changes:**
- `node.data.url` → `node.url()`
- `node.data.depth` → `node.depth()`
- `node.data = NodeData { url: Some(...), .. }` → `node.data = NodeData::link(...)`
