//! `no-nfd` — flag decomposed (NFD) text via its combining marks.

use tzlint_ast::{NodeKind, Span};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "no-nfd";

/// Whether `c` is a combining mark used here as a proxy for "text is in NFD form".
fn is_combining_mark(c: char) -> bool {
    matches!(
        c as u32,
        0x0300..=0x036F   // Combining Diacritical Marks
        | 0x1AB0..=0x1AFF // … Extended
        | 0x1DC0..=0x1DFF // … Supplement
        | 0x20D0..=0x20FF // … for Symbols
        | 0xFE20..=0xFE2F // Combining Half Marks
        | 0x3099..=0x309A // Japanese combining (han)dakuten
    )
}

/// Flags each combining mark (a proxy for decomposed/NFD text).
pub struct NoNfd {
    meta: RuleMeta,
}

impl NoNfd {
    /// Construct the rule (no options).
    pub fn new() -> Self {
        NoNfd {
            meta: RuleMeta::new(ID, Severity::Warning, vec![NodeKind::TEXT]),
        }
    }
}

impl Default for NoNfd {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for NoNfd {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
        let base = node.span().start;
        for (i, c) in node.text().char_indices() {
            if is_combining_mark(c) {
                cx.report(
                    Span::new(
                        base.saturating_add(i as u32),
                        base.saturating_add((i + c.len_utf8()) as u32),
                    ),
                    format!(
                        "Combining mark U+{:04X} detected; normalize text to NFC.",
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
    fn flags_decomposed_text() {
        // か + combining dakuten (U+3099) instead of the precomposed が.
        assert_eq!(diagnose(&NoNfd::new(), "か\u{3099}\n").len(), 1);
    }

    #[test]
    fn flags_a_mark_from_every_configured_range() {
        // One representative combining mark from each block `is_combining_mark` covers.
        for cp in [0x0300u32, 0x1AB0, 0x1DC0, 0x20D0, 0xFE20, 0x3099] {
            let mark = char::from_u32(cp).unwrap();
            let src = format!("a{mark}\n");
            assert_eq!(diagnose(&NoNfd::new(), &src).len(), 1, "U+{cp:04X}");
        }
    }

    #[test]
    fn precomposed_text_is_clean() {
        assert!(diagnose(&NoNfd::new(), "がぎ漢字\n").is_empty());
    }
}
