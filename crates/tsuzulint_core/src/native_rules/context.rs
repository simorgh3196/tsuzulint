//! Per-invocation context handed to every native rule.

use serde_json::Value;
use std::path::Path;
use tsuzulint_ast::TxtNode;
use tsuzulint_text::{Sentence, Token};

/// Everything a native rule needs to produce diagnostics for a single file.
///
/// The lifetime `'a` ties the borrows to the file-linting session, so the
/// rule can return diagnostics that reference substrings of `source` or
/// spans into `ast` without cloning.
pub struct RuleContext<'a> {
    /// Parsed AST root (`Document` node).
    pub ast: &'a TxtNode<'a>,
    /// Original source text, indexable by byte spans from the AST.
    pub source: &'a str,
    /// Morphological tokens. Empty unless some rule declared
    /// `needs_morphology()`.
    pub tokens: &'a [Token],
    /// Pre-split sentences. Empty unless some rule declared `needs_sentences()`.
    pub sentences: &'a [Sentence],
    /// The rule's configured options (raw JSON). Defaults to `Value::Null`
    /// when the config leaves the rule at its defaults.
    pub options: &'a Value,
    /// Absolute path of the file being linted, when known.
    pub file_path: Option<&'a Path>,
}
