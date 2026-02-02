//! # tsuzulint_parser
//!
//! Parser abstraction layer for TsuzuLint.
//!
//! This crate provides:
//! - A `Parser` trait for implementing custom parsers
//! - Built-in Markdown parser using `markdown-rs`
//! - Built-in plain text parser
//!
//! ## Architecture
//!
//! Parsers convert source text into TxtAST nodes. The parser trait allows
//! for custom file format support via WASM plugins.
//!
//! ## Example
//!
//! ```rust,ignore
//! use tsuzulint_parser::{MarkdownParser, Parser};
//! use tsuzulint_ast::AstArena;
//!
//! let arena = AstArena::new();
//! let parser = MarkdownParser::new();
//! let source = "# Hello\n\nThis is a paragraph.";
//!
//! let ast = parser.parse(&arena, source).unwrap();
//! ```

mod error;
mod markdown;
mod text;
mod traits;

pub use error::ParseError;
pub use markdown::MarkdownParser;
pub use text::PlainTextParser;
pub use traits::Parser;
