//! Parser trait definition.

use tsuzulint_ast::{AstArena, TxtNode};

use crate::ParseError;

/// Trait for parsing source text into TxtAST.
///
/// Implementations of this trait convert source text into an abstract syntax
/// tree that can be analyzed by lint rules.
///
/// # Example
///
/// ```rust,ignore
/// use tsuzulint_parser::Parser;
/// use tsuzulint_ast::AstArena;
///
/// struct MyParser;
///
/// impl Parser for MyParser {
///     fn name(&self) -> &str {
///         "my-parser"
///     }
///
///     fn extensions(&self) -> &[&str] {
///         &["myext"]
///     }
///
///     fn parse<'a>(
///         &self,
///         arena: &'a AstArena,
///         source: &str,
///     ) -> Result<TxtNode<'a>, ParseError> {
///         // Parse implementation
///         todo!()
///     }
/// }
/// ```
pub trait Parser {
    /// Returns the name of this parser.
    fn name(&self) -> &str;

    /// Returns the file extensions this parser handles.
    ///
    /// Extensions should not include the leading dot (e.g., `["md", "markdown"]`).
    fn extensions(&self) -> &[&str];

    /// Parses the source text into a TxtAST.
    ///
    /// # Arguments
    ///
    /// * `arena` - The arena allocator for AST nodes
    /// * `source` - The source text to parse
    ///
    /// # Returns
    ///
    /// The root `TxtNode` of the parsed AST, or an error if parsing fails.
    fn parse<'a>(&self, arena: &'a AstArena, source: &str) -> Result<TxtNode<'a>, ParseError>;

    /// Returns true if this parser can handle the given file extension.
    fn can_parse(&self, extension: &str) -> bool {
        self.extensions()
            .iter()
            .any(|ext| ext.eq_ignore_ascii_case(extension))
    }
}
