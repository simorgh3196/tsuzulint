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
    ///
    /// The sentinel `0` (`b'\0'`) means "use the processor's default delimiter"; the
    /// [`DelimitedProcessor`] resolves it to `,` (CSV) or tab (TSV). As a consequence `b'\0'`
    /// cannot be used as a real delimiter. (Plan 3's config builder sets the real byte or leaves
    /// `0`.)
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
    /// Match by header name (requires `has_header`). Header cells are trimmed before comparison,
    /// while this selector string is compared as-is, so callers should pass an already-trimmed
    /// name.
    Name(String),
    /// Match by 1-based column number (as written in config). `1` is the first column; `0` is
    /// out of range (the number is 1-based) and resolves to no column — the processor converts
    /// via `checked_sub(1)`, so `Index(0)` yields `None` and the column is skipped. Plan 3 will
    /// surface a note for such unresolved targets.
    Index(u32),
}

/// The rule sets to apply per region: a base set plus per-column sets. The CLI builds each set
/// (base, and `base ⊕ column-overlay` for every column) so this type stays rule-agnostic.
pub struct RegionRules {
    base: Vec<Box<dyn Rule>>,
    columns: Vec<ColumnRuleSet>,
}

/// One column's rule set plus the index/name it matches against a [`RegionTag`].
struct ColumnRuleSet {
    /// 0-based column index to match against `RegionTag.index`, if this column was selected by index.
    index: Option<u32>,
    /// Header name to match against `RegionTag.name`, if this column was selected by name.
    name: Option<String>,
    rules: Vec<Box<dyn Rule>>,
}

impl RegionRules {
    /// A `RegionRules` with only a base set (used for Markdown / un-tagged documents).
    #[must_use]
    pub fn base_only(base: Vec<Box<dyn Rule>>) -> Self {
        RegionRules {
            base,
            columns: Vec::new(),
        }
    }

    /// Register the rule set for a column selected by 0-based `index` and/or header `name`.
    pub fn push_column(
        &mut self,
        index: Option<u32>,
        name: Option<String>,
        rules: Vec<Box<dyn Rule>>,
    ) {
        self.columns.push(ColumnRuleSet { index, name, rules });
    }

    /// The rules to run for a region with `tag`: a matching column set, else the base set.
    ///
    /// Resolution is **name first, then index** (independent of column insertion order): a region
    /// tag carries both a 0-based index and a header name, so a name-selected set and an
    /// index-selected set can both match the same column. Searching by name first makes the
    /// documented "name match takes priority" contract hold regardless of how the columns were
    /// pushed.
    #[must_use]
    pub fn for_tag(&self, tag: &RegionTag) -> Vec<&dyn Rule> {
        if let Some(name) = tag.name.as_deref()
            && let Some(set) = self
                .columns
                .iter()
                .find(|set| set.name.as_deref() == Some(name))
        {
            return set.rules.iter().map(|r| r.as_ref()).collect();
        }
        if let Some(index) = tag.index
            && let Some(set) = self.columns.iter().find(|set| set.index == Some(index))
        {
            return set.rules.iter().map(|r| r.as_ref()).collect();
        }
        self.base.iter().map(|r| r.as_ref()).collect()
    }

    /// The ids of every rule across the base and all column sets — for the cache key.
    #[must_use]
    pub fn rule_ids(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = self.base.iter().map(|r| r.meta().id.as_str()).collect();
        for set in &self.columns {
            ids.extend(set.rules.iter().map(|r| r.meta().id.as_str()));
        }
        ids
    }

    /// The dictionary languages every enabled base+column rule requires (duplicates allowed; the
    /// caller dedupes). Mirrors [`rule_ids`](RegionRules::rule_ids); the morphology provider
    /// registry intersects this with its registered languages to fingerprint the active
    /// dictionaries into the cache key. A rule that declares no morphology requirement contributes
    /// nothing — the `filter_map` relies on the `Requirements` invariant that `required_lang()` is
    /// `Some` exactly when the rule needs morphology.
    #[must_use]
    pub fn required_langs(&self) -> Vec<tzlint_ast::morphology::Lang> {
        let mut langs: Vec<tzlint_ast::morphology::Lang> = self
            .base
            .iter()
            .filter_map(|r| r.meta().required_lang())
            .collect();
        for set in &self.columns {
            langs.extend(set.rules.iter().filter_map(|r| r.meta().required_lang()));
        }
        langs
    }
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

/// The single dispatch entry: select a processor by `ext`, parse `source` with `processor_cfg`,
/// and lint each resulting region (or the whole AST) with the rules its tag resolves to in
/// `rules`. Diagnostics carry absolute spans and are returned in the engine's stable order.
///
/// A parse failure is returned as [`ParseError`]; the caller renders it as one diagnostic for the
/// document (mirroring the existing direct/cached paths).
pub fn lint_document(
    ext: Option<&str>,
    source: &str,
    registry: &Registry,
    processor_cfg: &ProcessorConfig,
    rules: &RegionRules,
    morphology: Option<&crate::MorphologyRegistry>,
) -> Result<Vec<Diagnostic>, ParseError> {
    let processor = registry.for_ext(ext);
    let parsed = processor.parse(source, processor_cfg)?;
    let mut diagnostics = Vec::new();
    match parsed {
        Parsed::Ast(ast) => {
            let region_rules = rules.for_tag(&RegionTag::whole());
            lint_ast_into(&ast, &region_rules, morphology, &mut diagnostics)?;
        }
        Parsed::Regions(regions) => {
            for region in &regions {
                let region_rules = rules.for_tag(&region.tag);
                for ast in build_region_asts(core::slice::from_ref(region), source)? {
                    lint_ast_into(&ast, &region_rules, morphology, &mut diagnostics)?;
                }
            }
        }
    }
    // Sort unconditionally — even on the single-AST path — so callers never observe an unstable
    // order, and so diagnostics merged across multiple regions come out in one stable sequence.
    diagnostics.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    Ok(diagnostics)
}

/// Archive `ast`, build the morphology table (if a registry is supplied and active for `rules`),
/// lint with `rules`, and append the diagnostics into `out`.
///
/// This is the **M2 morphology integration point**. When `morphology` is `Some` and some enabled
/// rule requires a registered language, the registry's providers tokenize this AST's nodes into a
/// [`MorphologyV1`] table that is archived and passed to [`Engine::lint`]; otherwise the table is
/// `None` and morphology-requiring rules are skipped, byte-for-byte as before. The archived AST and
/// morphology buffers are siblings in this scope, so both outlive the borrow `Engine::lint` takes.
///
/// For a region produced by a delimited processor, each cell is an *independent mini-document* with
/// its own text spine and absolute spans, so it is tokenized independently here (correct per-cell);
/// reconciling that with the whole-`RegionRules` cache fingerprint is deferred (see design §16).
fn lint_ast_into(
    ast: &Ast,
    rules: &[&dyn Rule],
    morphology: Option<&crate::MorphologyRegistry>,
    out: &mut Vec<Diagnostic>,
) -> Result<(), ParseError> {
    let bytes = tzlint_ast::to_archive(ast).map_err(|e| ParseError {
        message: format!("archive failed: {e}"),
    })?;
    let archived = tzlint_ast::access(&bytes).map_err(|e| ParseError {
        message: format!("archive failed: {e}"),
    })?;
    // Build the per-document morphology table (None when no provider is active for these rules).
    let table = match morphology {
        Some(registry) => registry
            .build_table(archived, rules)
            .map_err(|e| ParseError {
                message: format!("morphology failed: {e}"),
            })?,
        None => None,
    };
    // Archive it into a sibling buffer that outlives the `Engine::lint` borrow below.
    let morph_bytes = match &table {
        Some(table) => Some(
            tzlint_ast::morphology::to_archive_morphology(table).map_err(|e| ParseError {
                message: format!("morphology archive failed: {e}"),
            })?,
        ),
        None => None,
    };
    let morphology = match &morph_bytes {
        Some(bytes) => Some(
            tzlint_ast::morphology::access_morphology(bytes).map_err(|e| ParseError {
                message: format!("morphology archive failed: {e}"),
            })?,
        ),
        None => None,
    };
    out.extend(crate::Engine::lint(archived, morphology, rules));
    Ok(())
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
/// node span so they are absolute into `source`, and set the AST's text to the whole `source`.
///
/// `crate::parse` strips a single leading BOM (`U+FEFF`) and returns spans relative to the
/// **stripped** text, so the shift includes that BOM's byte length. Without it, a cell whose
/// content begins with a BOM would have every span shifted 3 bytes too early — landing inside the
/// multibyte BOM, which mis-reports `line:column` and makes `apply_fixes`' char-boundary guard
/// silently drop fixes on that node. (The scanner only strips a BOM at file offset 0, so a BOM in
/// a quoted or non-first cell reaches here intact.)
fn markdown_slice_ast(text: &str, start: u32, source: &str) -> Result<Ast, ParseError> {
    // Bytes `crate::parse` strips from the front before assigning spans (0 or the BOM length).
    let bom_len = (text.len() - text.strip_prefix('\u{feff}').unwrap_or(text).len()) as u32;
    let shift = start + bom_len;
    let mut ast = crate::parse(text)?;
    for node in &mut ast.nodes {
        node.span = Span::new(node.span.start + shift, node.span.end + shift);
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
    fn builtin_extensions_are_pinned() {
        let reg = Registry::with_builtins();
        // Every built-in extension resolves to a processor that claims it.
        for ext in ["md", "markdown", "csv", "tsv"] {
            assert!(
                reg.for_ext(Some(ext)).extensions().contains(&ext),
                "extension {ext} should resolve to a processor claiming it",
            );
        }
        // An unknown extension falls back to Markdown (the default).
        assert!(reg.for_ext(Some("xyz")).extensions().contains(&"md"));
    }

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
        let diags = lint_document(
            Some("csv"),
            "id,body\n1,ﾊﾛｰ\n",
            &reg,
            &ProcessorConfig::default(),
            &RegionRules::base_only(vec![]),
            None,
        )
        .unwrap();
        assert!(diags.is_empty());
    }

    #[test]
    fn delimited_config_constructs() {
        let cfg = ProcessorConfig {
            delimited: Some(DelimitedConfig {
                delimiter: b',',
                has_header: true,
                columns: vec![
                    ColumnTarget {
                        selector: ColumnSelector::Name("body".into()),
                        parse_mode: ParseMode::Markdown,
                    },
                    ColumnTarget {
                        selector: ColumnSelector::Index(2),
                        parse_mode: ParseMode::PlainText,
                    },
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
    fn markdown_slice_with_leading_bom_rebases_onto_visible_text() {
        // A Markdown cell whose content starts with a BOM (U+FEFF, 3 bytes): `crate::parse`
        // strips it, so the rebase must skip those bytes. The TEXT span must cover the visible
        // "X" and be a valid char-boundary slice (regression for the 3-byte-too-early shift that
        // landed inside the BOM and silently dropped autofixes).
        let source = "id,body\n1,\u{feff}X\n"; // body cell "\u{feff}X" is bytes 10..14
        let asts = build_region_asts(&[region(&[(10, 14)], ParseMode::Markdown)], source).unwrap();
        let ast = &asts[0];
        let text_node = ast.nodes.iter().find(|n| n.kind == NodeKind::TEXT).unwrap();
        assert_eq!(text_node.span, Span::new(13, 14));
        assert_eq!(
            source.get(text_node.span.start as usize..text_node.span.end as usize),
            Some("X"),
        );
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
        let rr = RegionRules::base_only(vec![Box::new(FlagText::new())]);

        let via_document = lint_document(
            Some("md"),
            source,
            &reg,
            &ProcessorConfig::default(),
            &rr,
            None,
        )
        .unwrap();

        let ast = crate::parse(source).unwrap();
        let bytes = tzlint_ast::to_archive(&ast).unwrap();
        let archived = tzlint_ast::access(&bytes).unwrap();
        let direct = crate::Engine::lint(archived, None, &rules);

        assert_eq!(diag_spans(&via_document), diag_spans(&direct));
    }

    #[test]
    fn lint_document_applies_per_region_rules() {
        // A processor that yields one region tagged column(0,"body") covering bytes 0..3.
        struct OneCol;
        impl Processor for OneCol {
            fn extensions(&self) -> &[&str] {
                &["onecol"]
            }
            fn parse(&self, _s: &str, _c: &ProcessorConfig) -> Result<Parsed, ParseError> {
                Ok(Parsed::Regions(vec![Region {
                    slices: vec![Span::new(0, 3)],
                    tag: RegionTag::column(0, Some("body".into())),
                    parse_mode: ParseMode::PlainText,
                }]))
            }
        }
        let mut reg = Registry::with_builtins();
        reg.push(Box::new(OneCol));

        let flag = FlagText::new(); // base rule, flags TEXT
        let base: Vec<&dyn Rule> = vec![&flag];
        // base_only → the region falls back to base and flags its TEXT node at 0..3.
        let rr = RegionRules::base_only(vec![]); // empty base
        let mut rr_with_col = RegionRules::base_only(vec![]);
        // Give the "body" column the flag rule; base stays empty.
        // (Box a fresh FlagText for the column set.)
        rr_with_col.push_column(None, Some("body".into()), vec![Box::new(FlagText::new())]);

        let none = lint_document(
            Some("onecol"),
            "abc",
            &reg,
            &ProcessorConfig::default(),
            &rr,
            None,
        )
        .unwrap();
        assert!(
            none.is_empty(),
            "empty base + no column rules → no diagnostics"
        );
        let some = lint_document(
            Some("onecol"),
            "abc",
            &reg,
            &ProcessorConfig::default(),
            &rr_with_col,
            None,
        )
        .unwrap();
        assert_eq!(
            some.iter()
                .map(|d| (d.span.start, d.span.end))
                .collect::<Vec<_>>(),
            vec![(0, 3)]
        );
        let _ = base; // base slice kept for clarity
    }

    #[test]
    fn region_rules_resolve_by_name_then_index_then_base() {
        // Build with a tiny rule so we can identify which set was returned by its meta id.
        fn rule(id: &'static str) -> Box<dyn Rule> {
            struct R(RuleMeta);
            impl Rule for R {
                fn meta(&self) -> &RuleMeta {
                    &self.0
                }
                fn check<'a>(&self, _n: NodeRef<'a>, _c: &mut Context<'a>) {}
            }
            Box::new(R(RuleMeta::new(
                id,
                Severity::Warning,
                vec![NodeKind::TEXT],
            )))
        }
        let mut rr = RegionRules::base_only(vec![rule("base")]);
        rr.push_column(None, Some("body".into()), vec![rule("body")]);
        rr.push_column(Some(2), None, vec![rule("col2")]);

        let ids = |v: Vec<&dyn Rule>| {
            v.iter()
                .map(|r| r.meta().id.as_str().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(rr.for_tag(&RegionTag::whole())), vec!["base"]);
        assert_eq!(
            ids(rr.for_tag(&RegionTag::column(1, Some("body".into())))),
            vec!["body"]
        );
        assert_eq!(
            ids(rr.for_tag(&RegionTag::column(2, Some("other".into())))),
            vec!["col2"]
        );
        // No match → base.
        assert_eq!(
            ids(rr.for_tag(&RegionTag::column(9, Some("nope".into())))),
            vec!["base"]
        );
    }

    #[test]
    fn required_langs_unions_base_and_column_morphology_rules() {
        use tzlint_ast::morphology::Lang;

        fn morph_rule(id: &'static str, lang: Lang) -> Box<dyn Rule> {
            struct R(RuleMeta);
            impl Rule for R {
                fn meta(&self) -> &RuleMeta {
                    &self.0
                }
                fn check<'a>(&self, _n: NodeRef<'a>, _c: &mut Context<'a>) {}
            }
            Box::new(R(RuleMeta::new(
                id,
                Severity::Warning,
                vec![NodeKind::TEXT],
            )
            .with_morphology(lang)))
        }
        fn plain_rule(id: &'static str) -> Box<dyn Rule> {
            struct R(RuleMeta);
            impl Rule for R {
                fn meta(&self) -> &RuleMeta {
                    &self.0
                }
                fn check<'a>(&self, _n: NodeRef<'a>, _c: &mut Context<'a>) {}
            }
            Box::new(R(RuleMeta::new(
                id,
                Severity::Warning,
                vec![NodeKind::TEXT],
            )))
        }

        // base needs JA + a plain rule (contributes nothing); a column needs JA + KO.
        let mut rr = RegionRules::base_only(vec![morph_rule("ja", Lang::JA), plain_rule("plain")]);
        rr.push_column(
            Some(0),
            None,
            vec![morph_rule("ja2", Lang::JA), morph_rule("ko", Lang::KO)],
        );

        let langs = rr.required_langs();
        assert!(langs.contains(&Lang::JA));
        assert!(langs.contains(&Lang::KO));
        // The plain rule contributes nothing; every entry is a real morphology language.
        assert!(langs.iter().all(|l| *l == Lang::JA || *l == Lang::KO));
        assert_eq!(langs.iter().filter(|l| **l == Lang::KO).count(), 1);
    }

    #[test]
    fn for_tag_prefers_name_over_index_regardless_of_push_order() {
        // A column region tag carries BOTH an index and a header name, so a name-selected set and
        // an index-selected set can both match it. Name must win either way — independent of which
        // column was pushed first (config-key order would otherwise decide it).
        fn rule(id: &'static str) -> Box<dyn Rule> {
            struct R(RuleMeta);
            impl Rule for R {
                fn meta(&self) -> &RuleMeta {
                    &self.0
                }
                fn check<'a>(&self, _n: NodeRef<'a>, _c: &mut Context<'a>) {}
            }
            Box::new(R(RuleMeta::new(
                id,
                Severity::Warning,
                vec![NodeKind::TEXT],
            )))
        }
        let ids = |v: Vec<&dyn Rule>| {
            v.iter()
                .map(|r| r.meta().id.as_str().to_string())
                .collect::<Vec<_>>()
        };
        // The colliding region: column 0 (1st), header "body" — matched by both sets below.
        let tag = RegionTag::column(0, Some("body".into()));

        // index pushed first…
        let mut index_first = RegionRules::base_only(vec![rule("base")]);
        index_first.push_column(Some(0), None, vec![rule("by-index")]);
        index_first.push_column(None, Some("body".into()), vec![rule("by-name")]);
        assert_eq!(ids(index_first.for_tag(&tag)), vec!["by-name"]);

        // …and name pushed first — same result.
        let mut name_first = RegionRules::base_only(vec![rule("base")]);
        name_first.push_column(None, Some("body".into()), vec![rule("by-name")]);
        name_first.push_column(Some(0), None, vec![rule("by-index")]);
        assert_eq!(ids(name_first.for_tag(&tag)), vec!["by-name"]);
    }

    #[test]
    fn lint_document_regions_yields_absolute_spans() {
        let source = "abXYZok"; // slice 2..7 == "XYZok"
        let reg = {
            let mut r = Registry::with_builtins();
            r.push(Box::new(SliceAt)); // see Step 3 for `push`
            r
        };
        let rr = RegionRules::base_only(vec![Box::new(FlagText::new())]);
        let diags = lint_document(
            Some("slice"),
            source,
            &reg,
            &ProcessorConfig::default(),
            &rr,
            None,
        )
        .unwrap();
        // The single TEXT node spans the slice 2..7 absolutely.
        assert_eq!(diag_spans(&diags), vec![(2, 7)]);
    }

    /// End-to-end: an injected `WhitespaceProvider` is run over the document, the per-node
    /// `MorphologyV1` is built + archived, and a morphology rule reads its tokens via the engine.
    #[test]
    fn lint_document_runs_morphology_via_injected_provider() {
        use crate::{DictId, MorphologyRegistry};
        use tzlint_ast::morphology::Lang;
        use tzlint_pdk::WhitespaceProvider;

        struct MorphCount(RuleMeta);
        impl Rule for MorphCount {
            fn meta(&self) -> &RuleMeta {
                &self.0
            }
            fn check<'a>(&self, node: NodeRef<'a>, cx: &mut Context<'a>) {
                let n = cx.tokens_of(node.id()).count();
                cx.report(node.span(), format!("{n}"));
            }
        }
        let rules = RegionRules::base_only(vec![Box::new(MorphCount(
            RuleMeta::new("morph-count", Severity::Warning, vec![NodeKind::TEXT])
                .with_morphology(Lang::JA),
        ))]);
        let mut reg = MorphologyRegistry::new();
        reg.insert(
            Box::new(WhitespaceProvider::new(Lang::JA)),
            DictId::from_pin([9; 32]),
        );

        let diags = lint_document(
            Some("md"),
            "one two\n\nthree",
            &Registry::with_builtins(),
            &ProcessorConfig::default(),
            &rules,
            Some(&reg),
        )
        .unwrap();

        // Two TEXT leaves: "one two" → 2 whitespace tokens, "three" → 1. Sorted by span,
        // paragraph-1's leaf (count 2) precedes paragraph-2's (count 1).
        assert_eq!(
            diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>(),
            vec!["2", "1"]
        );
    }

    /// With no registry (or one with no active language), the morphology rule is skipped and the
    /// document lints exactly as before — no panic, no spurious tokens.
    #[test]
    fn lint_document_without_registry_skips_morphology_rules() {
        use crate::MorphologyRegistry;
        use tzlint_ast::morphology::Lang;

        struct MorphFlag(RuleMeta);
        impl Rule for MorphFlag {
            fn meta(&self) -> &RuleMeta {
                &self.0
            }
            fn check<'a>(&self, node: NodeRef<'a>, cx: &mut Context<'a>) {
                cx.report(node.span(), "morph");
            }
        }
        let rules = RegionRules::base_only(vec![Box::new(MorphFlag(
            RuleMeta::new("morph", Severity::Warning, vec![NodeKind::TEXT])
                .with_morphology(Lang::JA),
        ))]);
        let reg = Registry::with_builtins();
        let pcfg = ProcessorConfig::default();
        // None registry and an empty registry both skip the morphology rule entirely.
        let none = lint_document(Some("md"), "hi", &reg, &pcfg, &rules, None).unwrap();
        let empty = lint_document(
            Some("md"),
            "hi",
            &reg,
            &pcfg,
            &rules,
            Some(&MorphologyRegistry::new()),
        )
        .unwrap();
        assert!(none.is_empty(), "no registry → morphology rule skipped");
        assert!(empty.is_empty(), "empty registry → morphology rule skipped");
    }

    /// Surfaces are absolute `Span`s into the document. A second paragraph's tokens are shifted by
    /// that node's base offset, so resolving them against `ast.text` yields the original words —
    /// catching a broken base offset that token counts alone would not (a 0-based offset would make
    /// paragraph-2's first token resolve to "one t", not "three").
    #[test]
    fn lint_document_morphology_surfaces_are_absolute() {
        use crate::{DictId, MorphologyRegistry};
        use tzlint_ast::Span;
        use tzlint_ast::morphology::Lang;
        use tzlint_pdk::WhitespaceProvider;

        struct MorphSurfaces(RuleMeta);
        impl Rule for MorphSurfaces {
            fn meta(&self) -> &RuleMeta {
                &self.0
            }
            fn check<'a>(&self, node: NodeRef<'a>, cx: &mut Context<'a>) {
                let resolved: Vec<(Span, String)> = {
                    let text = cx.ast().text();
                    cx.tokens_of(node.id())
                        .map(|token| {
                            let span = token.surface();
                            let word = text
                                .get(span.start as usize..span.end as usize)
                                .unwrap_or("")
                                .to_string();
                            (span, word)
                        })
                        .collect()
                };
                for (span, word) in resolved {
                    cx.report(span, word);
                }
            }
        }
        let rules = RegionRules::base_only(vec![Box::new(MorphSurfaces(
            RuleMeta::new("surfaces", Severity::Warning, vec![NodeKind::TEXT])
                .with_morphology(Lang::JA),
        ))]);
        let mut reg = MorphologyRegistry::new();
        reg.insert(
            Box::new(WhitespaceProvider::new(Lang::JA)),
            DictId::from_pin([1; 32]),
        );

        let diags = lint_document(
            Some("md"),
            "one two\n\nthree",
            &Registry::with_builtins(),
            &ProcessorConfig::default(),
            &rules,
            Some(&reg),
        )
        .unwrap();
        assert_eq!(
            diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>(),
            vec!["one", "two", "three"]
        );
    }

    /// A provider failure propagates as a `ParseError` (the region's lint fails loudly), never a
    /// panic and never a silently dropped node.
    #[test]
    fn lint_document_propagates_provider_errors() {
        use crate::{DictId, MorphologyRegistry};
        use tzlint_ast::NodeId;
        use tzlint_ast::morphology::{Lang, MorphologyV1};
        use tzlint_pdk::{MorphologyError, MorphologyProvider};

        struct FailingProvider;
        impl MorphologyProvider for FailingProvider {
            fn lang(&self) -> Lang {
                Lang::JA
            }
            fn analyze(
                &self,
                _text: &str,
                _base: u32,
                _node: NodeId,
            ) -> Result<MorphologyV1, MorphologyError> {
                Err(MorphologyError::Backend("boom".into()))
            }
        }
        struct NeedsJa(RuleMeta);
        impl Rule for NeedsJa {
            fn meta(&self) -> &RuleMeta {
                &self.0
            }
            fn check<'a>(&self, _node: NodeRef<'a>, _cx: &mut Context<'a>) {}
        }
        let rules = RegionRules::base_only(vec![Box::new(NeedsJa(
            RuleMeta::new("needs-ja", Severity::Warning, vec![NodeKind::TEXT])
                .with_morphology(Lang::JA),
        ))]);
        let mut reg = MorphologyRegistry::new();
        reg.insert(Box::new(FailingProvider), DictId::from_pin([2; 32]));

        let err = lint_document(
            Some("md"),
            "hi",
            &Registry::with_builtins(),
            &ProcessorConfig::default(),
            &rules,
            Some(&reg),
        )
        .unwrap_err();
        assert!(
            err.message.contains("morphology failed"),
            "provider error must surface as a ParseError: {}",
            err.message
        );
    }

    /// The merge re-interns each token's reading, base form, and features into the shared pool —
    /// not just its surface. A provider that emits a rich token must have those fields survive
    /// `build_table` and be readable by a rule (`WhitespaceProvider` emits none, so it cannot
    /// exercise this path).
    #[test]
    fn lint_document_morphology_merge_preserves_reading_and_features() {
        use crate::{DictId, MorphologyRegistry};
        use tzlint_ast::Span;
        use tzlint_ast::morphology::{
            FeatureKey, Lang, MorphologyBuilder, MorphologyV1, Tagset, TokenAttrs,
        };
        use tzlint_pdk::{MorphologyError, MorphologyProvider};

        struct RichProvider;
        impl MorphologyProvider for RichProvider {
            fn lang(&self) -> Lang {
                Lang::JA
            }
            fn analyze(
                &self,
                text: &str,
                base: u32,
                node: tzlint_ast::NodeId,
            ) -> Result<MorphologyV1, MorphologyError> {
                let mut builder = MorphologyBuilder::new();
                builder.push_token(
                    TokenAttrs {
                        node,
                        surface: Span::new(base, base + text.len() as u32),
                        lang: Lang::JA,
                        tagset: Tagset::IPADIC,
                        flags: 0,
                    },
                    Some("ヨミ"),
                    Some("辞書形"),
                    &[(FeatureKey::POS, "助詞")],
                );
                Ok(builder.finish())
            }
        }

        // A rule that reads back each token's reading and POS feature through the merged table.
        struct ReadFields(RuleMeta);
        impl Rule for ReadFields {
            fn meta(&self) -> &RuleMeta {
                &self.0
            }
            fn check<'a>(&self, node: NodeRef<'a>, cx: &mut Context<'a>) {
                let Some(table) = cx.morphology() else {
                    return;
                };
                let lines: Vec<String> = cx
                    .tokens_of(node.id())
                    .map(|token| {
                        let reading = token.reading(table).unwrap_or("");
                        let pos = token
                            .features(table)
                            .find(|(key, _)| *key == FeatureKey::POS)
                            .and_then(|(_, value)| value)
                            .unwrap_or("");
                        format!("{reading}|{pos}")
                    })
                    .collect();
                for line in lines {
                    cx.report(node.span(), line);
                }
            }
        }
        let rules = RegionRules::base_only(vec![Box::new(ReadFields(
            RuleMeta::new("read-fields", Severity::Warning, vec![NodeKind::TEXT])
                .with_morphology(Lang::JA),
        ))]);
        let mut reg = MorphologyRegistry::new();
        reg.insert(Box::new(RichProvider), DictId::from_pin([3; 32]));

        let diags = lint_document(
            Some("md"),
            "あ",
            &Registry::with_builtins(),
            &ProcessorConfig::default(),
            &rules,
            Some(&reg),
        )
        .unwrap();
        // The reading and POS feature survived the re-intern in `append_table`.
        assert_eq!(
            diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>(),
            vec!["ヨミ|助詞"]
        );
    }
}
