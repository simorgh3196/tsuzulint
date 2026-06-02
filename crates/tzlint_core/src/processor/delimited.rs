//! The built-in delimited (CSV/TSV) processor. It scans the source into cell content spans and
//! emits one [`Region`] per configured target column. See `docs/design/input-format-processors.md`.

use super::scanner::scan_records;
use super::{
    ColumnSelector, DelimitedConfig, Parsed, Processor, ProcessorConfig, Region, RegionTag,
};
use crate::ParseError;

/// Lints configured columns of a delimited file. Built as [`DelimitedProcessor::csv`] or
/// [`DelimitedProcessor::tsv`]; the default delimiter is overridable via the config.
pub struct DelimitedProcessor {
    extensions: &'static [&'static str],
    default_delimiter: u8,
}

impl DelimitedProcessor {
    /// A CSV processor (`.csv`, default delimiter `,`).
    #[must_use]
    pub fn csv() -> Self {
        DelimitedProcessor {
            extensions: &["csv"],
            default_delimiter: b',',
        }
    }

    /// A TSV processor (`.tsv`, default delimiter tab).
    #[must_use]
    pub fn tsv() -> Self {
        DelimitedProcessor {
            extensions: &["tsv"],
            default_delimiter: b'\t',
        }
    }
}

impl Processor for DelimitedProcessor {
    fn extensions(&self) -> &[&str] {
        self.extensions
    }

    fn parse(&self, source: &str, cfg: &ProcessorConfig) -> Result<Parsed, ParseError> {
        // No columns configured for this format → lint nothing (opt-in).
        let Some(d) = cfg.delimited.as_ref() else {
            return Ok(Parsed::Regions(Vec::new()));
        };
        Ok(Parsed::Regions(build_regions(
            source,
            d,
            self.default_delimiter,
        )))
    }
}

/// Build one region per resolvable target column, each carrying that column's non-empty cell
/// spans across the body rows. Columns that cannot be resolved (a name absent from the header,
/// or an out-of-range index) are skipped here; Plan 3 surfaces a note for them.
fn build_regions(source: &str, d: &DelimitedConfig, default_delimiter: u8) -> Vec<Region> {
    let delimiter = if d.delimiter != 0 {
        d.delimiter
    } else {
        default_delimiter
    };
    let records = scan_records(source, delimiter);

    let header_names: Vec<String> = if d.has_header {
        records
            .first()
            .map(|h| {
                h.iter()
                    .map(|s| cell_text(source, *s).trim().to_string())
                    .collect()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let body: &[Vec<tzlint_ast::Span>] = if d.has_header {
        records.get(1..).unwrap_or(&[])
    } else {
        &records
    };

    let mut regions = Vec::new();
    for target in &d.columns {
        let (index0, name): (Option<u32>, Option<String>) = match &target.selector {
            ColumnSelector::Index(one_based) => (one_based.checked_sub(1), None),
            ColumnSelector::Name(n) => (
                header_names.iter().position(|h| h == n).map(|i| i as u32),
                Some(n.clone()),
            ),
        };
        let Some(idx) = index0 else { continue };
        let slices: Vec<tzlint_ast::Span> = body
            .iter()
            .filter_map(|rec| rec.get(idx as usize).copied())
            .filter(|s| s.start != s.end) // skip empty cells
            .collect();
        if slices.is_empty() {
            continue;
        }
        let resolved_name = name.or_else(|| header_names.get(idx as usize).cloned());
        regions.push(Region {
            slices,
            tag: RegionTag::column(idx, resolved_name),
            parse_mode: target.parse_mode,
        });
    }
    regions
}

/// The &str a span covers, or `""` if the span is somehow out of range (never panics).
fn cell_text(source: &str, span: tzlint_ast::Span) -> &str {
    source
        .get(span.start as usize..span.end as usize)
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ColumnTarget, ParseMode};

    fn cfg(columns: Vec<ColumnTarget>, has_header: bool) -> ProcessorConfig {
        ProcessorConfig {
            delimited: Some(DelimitedConfig {
                delimiter: b',',
                has_header,
                columns,
            }),
        }
    }

    /// Project a Parsed::Regions result to (tag-index, parse_mode, cell-texts) for assertions.
    fn regions_of(source: &str, parsed: Parsed) -> Vec<(Option<u32>, Option<String>, Vec<&str>)> {
        match parsed {
            Parsed::Regions(rs) => rs
                .into_iter()
                .map(|r| {
                    let texts = r.slices.iter().map(|s| cell_text(source, *s)).collect();
                    (r.tag.index, r.tag.name, texts)
                })
                .collect(),
            Parsed::Ast(_) => panic!("expected regions"),
        }
    }

    #[test]
    fn selects_column_by_header_name() {
        let source = "id,body\n1,hello\n2,world\n";
        let p = DelimitedProcessor::csv();
        let parsed = p
            .parse(
                source,
                &cfg(
                    vec![ColumnTarget {
                        selector: ColumnSelector::Name("body".into()),
                        parse_mode: ParseMode::Markdown,
                    }],
                    true,
                ),
            )
            .unwrap();
        // The region must carry the "column" kind (guards against a regression to
        // `RegionTag::whole()`).
        let Parsed::Regions(rs) = &parsed else {
            panic!("expected regions");
        };
        assert_eq!(rs.len(), 1);
        assert_eq!(rs[0].tag.kind, Some("column"));
        assert_eq!(
            regions_of(source, parsed),
            vec![(Some(1), Some("body".to_string()), vec!["hello", "world"])],
        );
    }

    #[test]
    fn selects_column_by_one_based_index_without_header() {
        let source = "a,1\nb,2\n";
        let p = DelimitedProcessor::csv();
        let parsed = p
            .parse(
                source,
                &cfg(
                    vec![ColumnTarget {
                        selector: ColumnSelector::Index(1),
                        parse_mode: ParseMode::PlainText,
                    }],
                    false,
                ),
            )
            .unwrap();
        // 1-based index 1 → 0-based 0 → first column "a","b".
        assert_eq!(
            regions_of(source, parsed),
            vec![(Some(0), None, vec!["a", "b"])]
        );
    }

    #[test]
    fn unknown_column_name_yields_no_region() {
        let source = "id,body\n1,hello\n";
        let p = DelimitedProcessor::csv();
        let parsed = p
            .parse(
                source,
                &cfg(
                    vec![ColumnTarget {
                        selector: ColumnSelector::Name("missing".into()),
                        parse_mode: ParseMode::Markdown,
                    }],
                    true,
                ),
            )
            .unwrap();
        assert!(matches!(parsed, Parsed::Regions(rs) if rs.is_empty()));
    }

    #[test]
    fn no_delimited_config_lints_nothing() {
        let p = DelimitedProcessor::csv();
        let parsed = p
            .parse("id,body\n1,x\n", &ProcessorConfig::default())
            .unwrap();
        assert!(matches!(parsed, Parsed::Regions(rs) if rs.is_empty()));
    }

    #[test]
    fn index_zero_is_silently_skipped() {
        // `Index` is 1-based, so `0` is out of range: `checked_sub(1)` → None → no region.
        // (Plan 3 surfaces a note for such unresolved targets.)
        let source = "a,1\nb,2\n";
        let p = DelimitedProcessor::csv();
        let parsed = p
            .parse(
                source,
                &cfg(
                    vec![ColumnTarget {
                        selector: ColumnSelector::Index(0),
                        parse_mode: ParseMode::PlainText,
                    }],
                    false,
                ),
            )
            .unwrap();
        assert!(matches!(parsed, Parsed::Regions(rs) if rs.is_empty()));
    }

    #[test]
    fn tsv_processor_uses_tab_via_default_delimiter() {
        // `delimiter: 0` means "use the processor default"; for `tsv()` that is a tab.
        let source = "id\tbody\n1\thello\n";
        let p = DelimitedProcessor::tsv();
        let parsed = p
            .parse(
                source,
                &ProcessorConfig {
                    delimited: Some(DelimitedConfig {
                        delimiter: 0, // sentinel → processor default (tab)
                        has_header: true,
                        columns: vec![ColumnTarget {
                            selector: ColumnSelector::Name("body".into()),
                            parse_mode: ParseMode::Markdown,
                        }],
                    }),
                },
            )
            .unwrap();
        assert_eq!(
            regions_of(source, parsed),
            vec![(Some(1), Some("body".to_string()), vec!["hello"])],
        );
    }
}
