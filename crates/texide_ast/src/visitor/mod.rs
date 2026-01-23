//! Visitor pattern for TxtAST traversal.
//!
//! This module provides traits and functions for traversing TxtAST nodes.
//!
//! # Overview
//!
//! - [`Visitor`] - Read-only traversal trait
//! - [`MutVisitor`] - AST transformation trait
//! - [`walk_node`] - Dispatch function for type-specific visitors
//! - [`walk_children`] - Traverse all children of a node
//!
//! # Examples
//!
//! ## Collecting Text Content
//!
//! ```rust
//! use texide_ast::{TxtNode, NodeType, Span, AstArena};
//! use texide_ast::visitor::{Visitor, VisitResult, walk_node};
//! use std::ops::ControlFlow;
//!
//! struct TextCollector<'a> {
//!     texts: Vec<&'a str>,
//! }
//!
//! impl<'a> Visitor<'a> for TextCollector<'a> {
//!     fn visit_str(&mut self, node: &TxtNode<'a>) -> VisitResult {
//!         if let Some(text) = node.value {
//!             self.texts.push(text);
//!         }
//!         ControlFlow::Continue(())
//!     }
//! }
//!
//! let arena = AstArena::new();
//! let text = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 5), "hello"));
//! let children = arena.alloc_slice_copy(&[*text]);
//! let doc = TxtNode::new_parent(NodeType::Document, Span::new(0, 5), children);
//!
//! let mut collector = TextCollector { texts: Vec::new() };
//! walk_node(&mut collector, &doc);
//! assert_eq!(collector.texts, vec!["hello"]);
//! ```
//!
//! ## Early Termination
//!
//! ```rust
//! use texide_ast::{TxtNode, NodeType, Span, AstArena};
//! use texide_ast::visitor::{Visitor, VisitResult, walk_node};
//! use std::ops::ControlFlow;
//!
//! struct FirstHeaderFinder {
//!     found_depth: Option<u8>,
//! }
//!
//! impl<'a> Visitor<'a> for FirstHeaderFinder {
//!     fn visit_header(&mut self, node: &TxtNode<'a>) -> VisitResult {
//!         self.found_depth = node.data.depth;
//!         ControlFlow::Break(()) // Stop traversal
//!     }
//! }
//! ```

mod visit;
mod visit_mut;
mod walk;

pub use visit::{VisitResult, Visitor};
pub use visit_mut::{walk_children_mut, walk_node_mut, MutVisitor, VisitMutResult};
pub use walk::{walk_children, walk_node};
