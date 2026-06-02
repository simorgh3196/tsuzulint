//! The format-neutral processor seam: a file's extension selects a [`Processor`] that
//! turns source into either a full [`Ast`] (Markdown's path) or a list of lintable
//! [`Region`]s. See `docs/design/input-format-processors.md`.

use tzlint_ast::Span;

use crate::ParseError;

/// A format-specific parser. Adding a new format = implement this trait and register it in
/// [`Registry::with_builtins`].
pub trait Processor {
    /// Handled file extensions, dot-less and lowercase (e.g. `["csv"]`, `["md", "markdown"]`).
    fn extensions(&self) -> &[&str];

    /// Parse `source`. Return [`Parsed::Regions`] for the common prose-extraction path, or
    /// [`Parsed::Ast`] when the format's own structure must be visible to rules.
    fn parse(&self, source: &str, cfg: &ProcessorConfig) -> Result<Parsed, ParseError>;
}

/// The result of [`Processor::parse`]. Spans on both arms are absolute byte offsets into the
/// original `source`.
pub enum Parsed {
    /// Lintable regions; the core parses each slice and produces per-slice mini-ASTs.
    Regions(Vec<Region>),
    /// A complete AST (text = whole source, spans absolute). Markdown takes this arm.
    Ast(tzlint_ast::Ast),
}

/// One lintable region: the unit that shares a rule set and a parse mode.
pub struct Region {
    /// Source slices making up this region (e.g. all cells of one column). Each slice is a
    /// **contiguous** byte range of `source` and is linted as an independent mini-document.
    pub slices: Vec<Span>,
    /// What this region is, so config can target rules at it (used by later plans).
    pub tag: RegionTag,
    /// How to interpret each slice before linting.
    pub parse_mode: ParseMode,
}

/// Format-neutral region identity. `"column"` is just one `kind` used by the delimited
/// processor; it is **not** a core concept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionTag {
    /// Region kind, a processor-defined static string (`Some("column")` for CSV/TSV);
    /// `None` for a single-document format (Markdown, plain text).
    pub kind: Option<&'static str>,
    /// 0-based ordinal within the kind (e.g. column index), or `None`.
    pub index: Option<u32>,
    /// A name (e.g. a column header), or `None`.
    pub name: Option<String>,
}

impl RegionTag {
    /// The tag for a whole single-document format (no kind/index/name).
    #[must_use]
    pub fn whole() -> Self {
        RegionTag { kind: None, index: None, name: None }
    }

    /// The tag for a delimited-format column at 0-based `index`, with an optional header `name`.
    #[must_use]
    pub fn column(index: u32, name: Option<String>) -> Self {
        RegionTag { kind: Some("column"), index: Some(index), name }
    }
}

/// How a region's slices are parsed before linting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParseMode {
    /// Parse as Markdown (CommonMark + GFM), reusing the Markdown rules. The default.
    #[default]
    Markdown,
    /// Treat each slice as one plain paragraph (no Markdown constructs).
    PlainText,
}

/// Per-format configuration handed to [`Processor::parse`]. Empty for now; later plans add
/// delimited-format settings (columns, header, delimiter, parse modes).
#[derive(Debug, Clone, Default)]
pub struct ProcessorConfig;

#[cfg(test)]
mod tests {
    use super::*;

    /// A test-only processor that reports a fixed extension and returns no regions.
    struct Fake;
    impl Processor for Fake {
        fn extensions(&self) -> &[&str] {
            &["fake"]
        }
        fn parse(&self, _source: &str, _cfg: &ProcessorConfig) -> Result<Parsed, ParseError> {
            Ok(Parsed::Regions(Vec::new()))
        }
    }

    #[test]
    fn region_tag_constructors_and_parse_mode() {
        let whole = RegionTag::whole();
        assert!(whole.kind.is_none());
        let col = RegionTag::column(2, Some("body".to_string()));
        assert_eq!(col.kind, Some("column"));
        assert_eq!(col.index, Some(2));
        assert_eq!(col.name.as_deref(), Some("body"));
        assert_eq!(ParseMode::default(), ParseMode::Markdown);
    }

    #[test]
    fn fake_processor_reports_extension() {
        let f = Fake;
        assert_eq!(f.extensions(), &["fake"]);
        let parsed = f.parse("x", &ProcessorConfig::default()).unwrap();
        assert!(matches!(parsed, Parsed::Regions(rs) if rs.is_empty()));
    }
}
