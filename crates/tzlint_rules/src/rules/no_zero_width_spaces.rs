//! `no-zero-width-spaces` — flag invisible / zero-width codepoints in prose.

use tzlint_ast::{NodeKind, Span};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "no-zero-width-spaces";

/// Whether `c` is one of the disallowed invisible codepoints.
fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{200B}' // ZERO WIDTH SPACE
        | '\u{200C}' // ZERO WIDTH NON-JOINER
        | '\u{200D}' // ZERO WIDTH JOINER
        | '\u{2060}' // WORD JOINER
        | '\u{FEFF}' // ZERO WIDTH NO-BREAK SPACE (BOM)
    )
}

/// Flags each invisible / zero-width codepoint.
pub struct NoZeroWidthSpaces {
    meta: RuleMeta,
}

impl NoZeroWidthSpaces {
    /// Construct the rule (no options).
    pub fn new() -> Self {
        NoZeroWidthSpaces {
            meta: RuleMeta::new(ID, Severity::Warning, vec![NodeKind::TEXT]),
        }
    }
}

impl Default for NoZeroWidthSpaces {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for NoZeroWidthSpaces {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
        let base = node.span().start;
        for (i, c) in node.text().char_indices() {
            if is_invisible(c) {
                cx.report(
                    Span::new(
                        base.saturating_add(i as u32),
                        base.saturating_add((i + c.len_utf8()) as u32),
                    ),
                    format!(
                        "Invisible codepoint U+{:04X} is not allowed in prose.",
                        c as u32
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::diagnose;

    #[test]
    fn flags_zero_width_space() {
        assert_eq!(
            diagnose(&NoZeroWidthSpaces::new(), "hello\u{200B}world\n").len(),
            1
        );
    }

    #[test]
    fn plain_text_is_clean() {
        assert!(diagnose(&NoZeroWidthSpaces::new(), "ふつうの文章\n").is_empty());
    }
}
