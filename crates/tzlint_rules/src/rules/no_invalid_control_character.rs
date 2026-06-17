//! `no-invalid-control-character` — flag invalid control characters in prose text.
//!
//! Mirrors the upstream textlint rule of the same name. Disallowed codepoint ranges:
//! - C0 controls: U+0000–U+0008 (NUL through BS)
//! - U+000B LINE TABULATION (VT)
//! - U+000C FORM FEED (FF)
//! - U+000E–U+001F (SO through US)
//! - U+007F DELETE
//! - C1 controls: U+0080–U+009F
//! - BiDi formatting: U+202A–U+202E
//!
//! Allowed (not flagged): U+0009 TAB, U+000A LF, U+000D CR.

use tzlint_ast::{NodeKind, Span};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "no-invalid-control-character";

/// Whether `c` is an invalid control character that should be flagged.
fn is_invalid_control(c: char) -> bool {
    let cp = c as u32;
    matches!(
        cp,
        // C0 controls except TAB (0x09), LF (0x0A), CR (0x0D)
        0x00..=0x08
        | 0x0B
        | 0x0C
        | 0x0E..=0x1F
        // DEL
        | 0x7F
        // C1 controls
        | 0x80..=0x9F
        // BiDi formatting characters (upstream includes 0x202A–0x202E)
        | 0x202A..=0x202E
    )
}

/// Flags each invalid control character found in text nodes.
pub struct NoInvalidControlCharacter {
    meta: RuleMeta,
}

impl NoInvalidControlCharacter {
    /// Construct the rule (no options).
    pub fn new() -> Self {
        NoInvalidControlCharacter {
            meta: RuleMeta::new(ID, Severity::Warning, vec![NodeKind::TEXT]),
        }
    }
}

impl Default for NoInvalidControlCharacter {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for NoInvalidControlCharacter {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
        let base = node.span().start;
        for (i, c) in node.text().char_indices() {
            if is_invalid_control(c) {
                cx.report(
                    Span::new(
                        base.saturating_add(i as u32),
                        base.saturating_add((i + c.len_utf8()) as u32),
                    ),
                    format!(
                        "不正なコントロール文字 U+{:04X} が含まれています。",
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
    fn flags_nul_byte() {
        assert_eq!(
            diagnose(&NoInvalidControlCharacter::new(), "hello\u{0000}world\n").len(),
            1
        );
    }

    #[test]
    fn flags_c0_control_characters() {
        // SOH through BS (0x01–0x08)
        for cp in 0x01u32..=0x08 {
            let c = char::from_u32(cp).unwrap();
            let src = format!("a{c}b\n");
            assert_eq!(
                diagnose(&NoInvalidControlCharacter::new(), &src).len(),
                1,
                "U+{cp:04X} should be flagged"
            );
        }
        // VT (0x0B) and FF (0x0C)
        for cp in [0x0Bu32, 0x0C] {
            let c = char::from_u32(cp).unwrap();
            let src = format!("a{c}b\n");
            assert_eq!(
                diagnose(&NoInvalidControlCharacter::new(), &src).len(),
                1,
                "U+{cp:04X} should be flagged"
            );
        }
        // SO through US (0x0E–0x1F)
        for cp in 0x0Eu32..=0x1F {
            let c = char::from_u32(cp).unwrap();
            let src = format!("a{c}b\n");
            assert_eq!(
                diagnose(&NoInvalidControlCharacter::new(), &src).len(),
                1,
                "U+{cp:04X} should be flagged"
            );
        }
    }

    #[test]
    fn flags_del() {
        assert_eq!(
            diagnose(&NoInvalidControlCharacter::new(), "a\u{007F}b\n").len(),
            1
        );
    }

    #[test]
    fn flags_c1_controls() {
        for cp in [0x80u32, 0x85, 0x9F] {
            let c = char::from_u32(cp).unwrap();
            let src = format!("a{c}b\n");
            assert_eq!(
                diagnose(&NoInvalidControlCharacter::new(), &src).len(),
                1,
                "U+{cp:04X} should be flagged"
            );
        }
    }

    #[test]
    fn flags_bidi_formatting_characters() {
        // U+202E RIGHT-TO-LEFT OVERRIDE
        assert_eq!(
            diagnose(&NoInvalidControlCharacter::new(), "a\u{202E}b\n").len(),
            1
        );
        // U+202A LEFT-TO-RIGHT EMBEDDING
        assert_eq!(
            diagnose(&NoInvalidControlCharacter::new(), "a\u{202A}b\n").len(),
            1
        );
    }

    #[test]
    fn allows_tab_lf_cr() {
        // TAB is allowed
        assert!(diagnose(&NoInvalidControlCharacter::new(), "col1\tcol2\n").is_empty());
        // LF is the normal line ending
        assert!(diagnose(&NoInvalidControlCharacter::new(), "line1\nline2\n").is_empty());
        // CR: markdown source may include it; it's allowed
        assert!(diagnose(&NoInvalidControlCharacter::new(), "text\r\n").is_empty());
    }

    #[test]
    fn plain_text_is_clean() {
        assert!(
            diagnose(
                &NoInvalidControlCharacter::new(),
                "普通の文章です。Normal text here.\n"
            )
            .is_empty()
        );
    }
}
