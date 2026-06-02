//! Resolved per-format configuration (the `formats.<id>` section of a config file).

use std::collections::BTreeMap;

use tzlint_pdk::RuleId;

use crate::RuleSetting;
use crate::processor::{ColumnSelector, ParseMode};

/// Resolved settings for one delimited format (`formats.csv` / `formats.tsv`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatConfig {
    /// Whether the first record is a header row.
    pub has_header: bool,
    /// An explicit field delimiter override (else the format default: `,` for csv, tab for tsv).
    pub delimiter: Option<char>,
    /// The target columns to lint, in config order.
    pub columns: Vec<ColumnConfig>,
}

/// One target column: how to select it, how to parse its cells, and its rule overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnConfig {
    /// By header name or 1-based number.
    pub selector: ColumnSelector,
    /// How each cell is parsed before linting (default Markdown).
    pub parse_mode: ParseMode,
    /// Rules layered ON TOP of the base `rules` for this column (column wins).
    pub rules: BTreeMap<RuleId, RuleSetting>,
}
