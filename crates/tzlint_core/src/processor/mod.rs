//! The format-neutral processor seam: a file's extension selects a [`Processor`] that
//! turns source into either a full [`Ast`] (Markdown's path) or a list of lintable
//! [`Region`]s. See `docs/design/input-format-processors.md`.

use tzlint_ast::{Ast, Node, NodeId, NodeKind, OptionNodeId, Span};
use tzlint_pdk::{Diagnostic, Rule};

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
        RegionTag {
            kind: None,
            index: None,
            name: None,
        }
    }

    /// The tag for a delimited-format column at 0-based `index`, with an optional header `name`.
    #[must_use]
    pub fn column(index: u32, name: Option<String>) -> Self {
        RegionTag {
            kind: Some("column"),
            index: Some(index),
            name,
        }
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

/// Per-format configuration handed to [`Processor::parse`]. Markdown ignores it; delimited
/// processors read [`delimited`](ProcessorConfig::delimited).
#[derive(Debug, Clone, Default)]
pub struct ProcessorConfig {
    /// Settings for the delimited (CSV/TSV) processor, or `None` when no columns are configured
    /// (in which case the processor lints nothing).
    pub delimited: Option<DelimitedConfig>,
}

/// Resolved settings for one delimited format (the `formats.csv` / `formats.tsv` section).
#[derive(Debug, Clone)]
pub struct DelimitedConfig {
    /// The field delimiter byte (`b','` for CSV, `b'\t'` for TSV; overridable).
    pub delimiter: u8,
    /// Whether the first record is a header row (excluded from linting; enables name lookup).
    pub has_header: bool,
    /// The columns to lint, in config order.
    pub columns: Vec<ColumnTarget>,
}

/// One target column: how to select it and how to interpret its cells.
#[derive(Debug, Clone)]
pub struct ColumnTarget {
    /// How this column is identified.
    pub selector: ColumnSelector,
    /// How each cell's text is parsed before linting.
    pub parse_mode: ParseMode,
}

/// How a target column is identified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnSelector {
    /// Match by header name (requires `has_header`).
    Name(String),
    /// Match by 1-based column number (as written in config).
    Index(u32),
}

mod markdown;
pub use markdown::MarkdownProcessor;

mod scanner;

mod delimited;
pub use delimited::DelimitedProcessor;

/// A set of built-in [`Processor`]s, resolved by file extension. The Markdown processor is
/// always the default/fallback for unknown or missing extensions.
pub struct Registry {
    /// The fallback processor (Markdown). Resolved when no extension matches.
    default: Box<dyn Processor>,
    /// Additional processors, tried before the default by extension.
    others: Vec<Box<dyn Processor>>,
}

impl Registry {
    /// The registry of built-in processors. Markdown is the default; CSV/TSV are registered.
    #[must_use]
    pub fn with_builtins() -> Self {
        Registry {
            default: Box::new(MarkdownProcessor),
            others: vec![
                Box::new(DelimitedProcessor::csv()),
                Box::new(DelimitedProcessor::tsv()),
            ],
        }
    }

    /// The processor handling `ext` (case-insensitive, dot-less), or the Markdown default when
    /// `ext` is `None` or unmatched.
    ///
    /// Permanent contract: only the `others` are matched by extension (case-insensitively); the
    /// `default` (Markdown) processor is returned for any unrecognized or missing extension, so a
    /// file always has a processor.
    #[must_use]
    pub fn for_ext(&self, ext: Option<&str>) -> &dyn Processor {
        if let Some(ext) = ext {
            let lower = ext.to_ascii_lowercase();
            for p in &self.others {
                if p.extensions().iter().any(|e| *e == lower) {
                    return p.as_ref();
                }
            }
        }
        self.default.as_ref()
    }

    /// Register an additional processor (tried before the Markdown default by extension).
    pub fn push(&mut self, processor: Box<dyn Processor>) {
        self.others.push(processor);
    }
}

/// The single dispatch entry: select a processor by `ext`, parse `source`, lint every resulting
/// AST with `rules`, and return the merged diagnostics in the engine's stable order.
///
/// A parse failure is returned as [`ParseError`]; the caller renders it as one diagnostic for the
/// document (mirroring the existing direct/cached paths).
pub fn lint_document(
    ext: Option<&str>,
    source: &str,
    registry: &Registry,
    rules: &[&dyn Rule],
) -> Result<Vec<Diagnostic>, ParseError> {
    let processor = registry.for_ext(ext);
    let parsed = processor.parse(source, &ProcessorConfig::default())?;
    let asts: Vec<Ast> = match parsed {
        Parsed::Ast(ast) => vec![ast],
        Parsed::Regions(regions) => build_region_asts(&regions, source)?,
    };
    let mut diagnostics = Vec::new();
    for ast in &asts {
        let bytes = tzlint_ast::to_archive(ast).map_err(|e| ParseError {
            message: format!("archive failed: {e}"),
        })?;
        let archived = tzlint_ast::access(&bytes).map_err(|e| ParseError {
            message: format!("archive failed: {e}"),
        })?;
        diagnostics.extend(crate::Engine::lint(archived, rules));
    }
    // Sort unconditionally — even on the single-AST path — so callers never observe an unstable
    // order, and so diagnostics merged across multiple regions come out in one stable sequence.
    diagnostics.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    Ok(diagnostics)
}

/// Build one independent mini-[`Ast`] per slice of every region. Each mini-AST's `text` is the
/// whole `source` and its node spans are **absolute** byte offsets into it, so diagnostics from
/// linting it are already in the original file's coordinates (no later shifting).
///
/// A slice that fails to parse (Markdown only) propagates its [`ParseError`]; the caller turns
/// it into a single diagnostic for the document.
pub(crate) fn build_region_asts(regions: &[Region], source: &str) -> Result<Vec<Ast>, ParseError> {
    let mut asts = Vec::new();
    for region in regions {
        // `region.tag` is intentionally unused at this stage; later plans consume it for
        // per-region rule selection.
        for slice in &region.slices {
            let start = slice.start;
            let end = slice.end;
            // An out-of-range slice (a processor-contract violation) degrades to empty text rather
            // than panicking.
            let text = source.get(start as usize..end as usize).unwrap_or("");
            let ast = match region.parse_mode {
                ParseMode::Markdown => markdown_slice_ast(text, start, source)?,
                ParseMode::PlainText => plaintext_slice_ast(start, end, source),
            };
            asts.push(ast);
        }
    }
    Ok(asts)
}

/// Parse `text` (a slice of `source` starting at byte `start`) as Markdown, then shift every
/// node span by `start` so they are absolute into `source`, and set the AST's text to the whole
/// `source`.
fn markdown_slice_ast(text: &str, start: u32, source: &str) -> Result<Ast, ParseError> {
    let mut ast = crate::parse(text)?;
    for node in &mut ast.nodes {
        node.span = Span::new(node.span.start + start, node.span.end + start);
    }
    ast.text = source.to_string();
    Ok(ast)
}

/// A fixed Root → Paragraph → Text spine spanning `[start, end)`, with the AST's text set to the
/// whole `source` (so spans are absolute).
fn plaintext_slice_ast(start: u32, end: u32, source: &str) -> Ast {
    let span = Span::new(start, end);
    let nodes = vec![
        Node {
            kind: NodeKind::ROOT,
            span,
            parent: NodeId(0),
            first_child: OptionNodeId::some(NodeId(1)),
            next_sibling: OptionNodeId::NONE,
        },
        Node {
            kind: NodeKind::PARAGRAPH,
            span,
            parent: NodeId(0),
            first_child: OptionNodeId::some(NodeId(2)),
            next_sibling: OptionNodeId::NONE,
        },
        Node {
            kind: NodeKind::TEXT,
            span,
            parent: NodeId(1),
            first_child: OptionNodeId::NONE,
            next_sibling: OptionNodeId::NONE,
        },
    ];
    Ast {
        nodes,
        text: source.to_string(),
        root: NodeId(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_includes_csv_and_tsv() {
        let reg = Registry::with_builtins();
        assert!(reg.for_ext(Some("csv")).extensions().contains(&"csv"));
        assert!(reg.for_ext(Some("tsv")).extensions().contains(&"tsv"));
        // Unknown still falls back to Markdown.
        assert!(reg.for_ext(Some("rtf")).extensions().contains(&"md"));
    }

    #[test]
    fn csv_with_default_config_lints_nothing_end_to_end() {
        // Through the dispatch entry, a .csv with the default (empty) ProcessorConfig yields no
        // diagnostics — opt-in safety.
        let reg = Registry::with_builtins();
        let diags = lint_document(Some("csv"), "id,body\n1,ﾊﾛｰ\n", &reg, &[]).unwrap();
        assert!(diags.is_empty());
    }

    #[test]
    fn delimited_config_constructs() {
        let cfg = ProcessorConfig {
            delimited: Some(DelimitedConfig {
                delimiter: b',',
                has_header: true,
                columns: vec![
                    ColumnTarget { selector: ColumnSelector::Name("body".into()), parse_mode: ParseMode::Markdown },
                    ColumnTarget { selector: ColumnSelector::Index(2), parse_mode: ParseMode::PlainText },
                ],
            }),
        };
        let d = cfg.delimited.as_ref().unwrap();
        assert_eq!(d.delimiter, b',');
        assert!(d.has_header);
        assert_eq!(d.columns.len(), 2);
        assert_eq!(d.columns[1].selector, ColumnSelector::Index(2));
    }

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

    #[test]
    fn registry_resolves_known_extension_and_defaults_to_markdown() {
        let reg = Registry::with_builtins();
        // Markdown is registered for md/markdown…
        assert!(reg.for_ext(Some("md")).extensions().contains(&"md"));
        assert!(
            reg.for_ext(Some("markdown"))
                .extensions()
                .contains(&"markdown")
        );
        // …and is the fallback for unknown / missing extensions.
        let default_exts = reg.for_ext(None).extensions().to_vec();
        assert!(default_exts.contains(&"md"));
        assert_eq!(
            reg.for_ext(Some("UNKNOWN")).extensions().to_vec(),
            default_exts
        );
        // Case-insensitive match.
        assert!(reg.for_ext(Some("MD")).extensions().contains(&"md"));
    }

    use tzlint_ast::{NodeKind, Span};

    fn region(slices: &[(u32, u32)], mode: ParseMode) -> Region {
        Region {
            slices: slices.iter().map(|(s, e)| Span::new(*s, *e)).collect(),
            tag: RegionTag::whole(),
            parse_mode: mode,
        }
    }

    #[test]
    fn plaintext_slice_builds_absolute_root_paragraph_text() {
        let source = "id,body\n1,hello\n";
        // "hello" is at bytes 10..15.
        let asts = build_region_asts(&[region(&[(10, 15)], ParseMode::PlainText)], source).unwrap();
        assert_eq!(asts.len(), 1);
        let ast = &asts[0];
        // text is the whole source; spans are absolute into it.
        assert_eq!(ast.text, source);
        assert_eq!(ast.nodes[ast.root.0 as usize].kind, NodeKind::ROOT);
        let text_node = ast.nodes.iter().find(|n| n.kind == NodeKind::TEXT).unwrap();
        assert_eq!(text_node.span, Span::new(10, 15));
        assert_eq!(
            &ast.text[text_node.span.start as usize..text_node.span.end as usize],
            "hello"
        );
    }

    #[test]
    fn markdown_slice_shifts_spans_to_absolute() {
        let source = "id,body\n1,**bold**\n";
        // "**bold**" is at bytes 10..18.
        let asts = build_region_asts(&[region(&[(10, 18)], ParseMode::Markdown)], source).unwrap();
        let ast = &asts[0];
        assert_eq!(ast.text, source);
        // The STRONG node's span must be absolute into the whole source.
        let strong = ast
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::STRONG)
            .unwrap();
        assert_eq!(
            &ast.text[strong.span.start as usize..strong.span.end as usize],
            "**bold**"
        );
        assert_eq!(strong.span, Span::new(10, 18));
    }

    #[test]
    fn one_mini_ast_per_slice() {
        let source = "a\nbb\nccc\n";
        let asts =
            build_region_asts(&[region(&[(0, 1), (2, 4)], ParseMode::PlainText)], source).unwrap();
        assert_eq!(asts.len(), 2); // independent mini-document per slice
    }

    use tzlint_pdk::{Context, Diagnostic, NodeRef, Rule, RuleMeta, Severity};

    /// Flags every TEXT node at its own span (so we can read back absolute offsets).
    struct FlagText {
        meta: RuleMeta,
    }
    impl FlagText {
        fn new() -> Self {
            FlagText {
                meta: RuleMeta::new("flag-text", Severity::Warning, vec![NodeKind::TEXT]),
            }
        }
    }
    impl Rule for FlagText {
        fn meta(&self) -> &RuleMeta {
            &self.meta
        }
        fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
            cx.report(node.span(), "text");
        }
    }

    /// A processor that yields one PlainText region covering bytes 2..7 of the source.
    struct SliceAt;
    impl Processor for SliceAt {
        fn extensions(&self) -> &[&str] {
            &["slice"]
        }
        fn parse(&self, _source: &str, _cfg: &ProcessorConfig) -> Result<Parsed, ParseError> {
            Ok(Parsed::Regions(vec![Region {
                slices: vec![Span::new(2, 7)],
                tag: RegionTag::whole(),
                parse_mode: ParseMode::PlainText,
            }]))
        }
    }

    fn diag_spans(diags: &[Diagnostic]) -> Vec<(u32, u32)> {
        // `Diagnostic.span` is a public field (not a method); `node.span()` (NodeRef) is a method.
        diags.iter().map(|d| (d.span.start, d.span.end)).collect()
    }

    #[test]
    fn lint_document_markdown_matches_direct_parse() {
        // The Markdown path must equal parse → archive → access → Engine::lint exactly.
        let source = "本文。\n";
        let reg = Registry::with_builtins();
        let rule = FlagText::new();
        let rules: Vec<&dyn Rule> = vec![&rule];

        let via_document = lint_document(Some("md"), source, &reg, &rules).unwrap();

        let ast = crate::parse(source).unwrap();
        let bytes = tzlint_ast::to_archive(&ast).unwrap();
        let archived = tzlint_ast::access(&bytes).unwrap();
        let direct = crate::Engine::lint(archived, &rules);

        assert_eq!(diag_spans(&via_document), diag_spans(&direct));
    }

    #[test]
    fn lint_document_regions_yields_absolute_spans() {
        let source = "abXYZok"; // slice 2..7 == "XYZok"
        let reg = {
            let mut r = Registry::with_builtins();
            r.push(Box::new(SliceAt)); // see Step 3 for `push`
            r
        };
        let rule = FlagText::new();
        let rules: Vec<&dyn Rule> = vec![&rule];
        let diags = lint_document(Some("slice"), source, &reg, &rules).unwrap();
        // The single TEXT node spans the slice 2..7 absolutely.
        assert_eq!(diag_spans(&diags), vec![(2, 7)]);
    }
}
