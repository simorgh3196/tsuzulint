//! The built-in Markdown processor — the current `parse` behavior, expressed as a
//! [`Processor`] that returns a full AST.

use super::{Parsed, Processor, ProcessorConfig};
use crate::ParseError;

/// Parses CommonMark + GFM + frontmatter (the existing [`crate::parse`]) into a full AST.
pub struct MarkdownProcessor;

impl Processor for MarkdownProcessor {
    fn extensions(&self) -> &[&str] {
        &["md", "markdown"]
    }

    fn parse(&self, source: &str, _cfg: &ProcessorConfig) -> Result<Parsed, ParseError> {
        Ok(Parsed::Ast(crate::parse(source)?))
    }
}
