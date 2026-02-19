//! # tsuzulint_ast
//!
//! TxtAST definitions for TsuzuLint.
//!
//! This crate provides the Abstract Syntax Tree (AST) types used by TsuzuLint.
//! The AST is designed to be compatible with textlint's TxtAST specification while
//! being optimized for Rust's memory model using Arena allocation.
//!
//! ## Architecture
//!
//! - Uses `bumpalo` for Arena allocation (Oxc-like architecture)
//! - All AST nodes are allocated in a single arena per file
//! - Reference locality is maximized for cache efficiency
//! - Memory is freed all at once when parsing is complete
//!
//! ## Example
//!
//! ```rust
//! use tsuzulint_ast::{AstArena, TxtNode, NodeType, Span};
//!
//! let arena = AstArena::new();
//!
//! // Nodes are allocated in the arena using constructor methods
//! let node = arena.alloc(TxtNode::new_parent(
//!     NodeType::Document,
//!     Span::new(0, 100),
//!     &[],
//! ));
//! ```

mod arena;
mod node;
mod node_type;
mod span;
pub mod visitor;

pub use arena::AstArena;
pub use node::{DefinitionData, LinkData, NodeData, ReferenceData, TxtNode};
pub use node_type::NodeType;
pub use span::{Location, Position, Span};

// Re-export commonly used visitor items for convenience
pub use visitor::{MutVisitor, VisitResult, Visitor};
