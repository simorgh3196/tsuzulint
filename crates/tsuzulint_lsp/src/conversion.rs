//! LSP type conversion utilities.

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

use tsuzulint_core::{Diagnostic as TsuzuLintDiagnostic, Severity as TsuzuLintSeverity};

/// Converts a TsuzuLint diagnostic to an LSP diagnostic.
pub fn to_lsp_diagnostic(diag: &TsuzuLintDiagnostic, text: &str) -> Option<Diagnostic> {
    let range = offset_to_range(diag.span.start as usize, diag.span.end as usize, text)?;

    let severity = match diag.severity {
        TsuzuLintSeverity::Error => DiagnosticSeverity::ERROR,
        TsuzuLintSeverity::Warning => DiagnosticSeverity::WARNING,
        TsuzuLintSeverity::Info => DiagnosticSeverity::INFORMATION,
    };

    Some(Diagnostic {
        range,
        severity: Some(severity),
        code: Some(NumberOrString::String(diag.rule_id.clone())),
        source: Some("tsuzulint".to_string()),
        message: diag.message.clone(),
        ..Default::default()
    })
}

/// Converts byte offsets to an LSP range.
pub fn offset_to_range(start: usize, end: usize, text: &str) -> Option<Range> {
    let start_pos = offset_to_position(start, text)?;
    let end_pos = offset_to_position(end, text)?;
    Some(Range::new(start_pos, end_pos))
}

/// Converts a byte offset to an LSP position.
///
/// This function converts a UTF-8 byte offset to a line/column position
/// as expected by the LSP protocol (UTF-16 code units).
///
/// # Preconditions
///
/// - The offset must be at a valid UTF-8 character boundary. If the offset
///   splits a multi-byte character, the behavior is undefined and may produce
///   incorrect results.
///
/// # Returns
///
/// - `Some(Position)` if the offset is valid (≤ text length)
/// - `None` if the offset exceeds the text length
pub fn offset_to_position(offset: usize, text: &str) -> Option<Position> {
    if offset > text.len() {
        return None;
    }

    let mut line = 0u32;
    let mut col = 0u32;
    let mut current_offset = 0;

    for ch in text.chars() {
        if current_offset >= offset {
            break;
        }

        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += ch.len_utf16() as u32;
        }

        current_offset += ch.len_utf8();
    }

    Some(Position::new(line, col))
}

/// Helper to compare Positions (p1 <= p2)
pub fn positions_le(p1: Position, p2: Position) -> bool {
    p1.line < p2.line || (p1.line == p2.line && p1.character <= p2.character)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsuzulint_ast::Span;

    #[test]
    fn test_offset_to_position_basic_ascii() {
        let text = "Hello World";
        assert_eq!(offset_to_position(0, text), Some(Position::new(0, 0)));
        assert_eq!(offset_to_position(5, text), Some(Position::new(0, 5)));
        assert_eq!(offset_to_position(10, text), Some(Position::new(0, 10)));
        assert_eq!(offset_to_position(11, text), Some(Position::new(0, 11)));
        assert_eq!(offset_to_position(12, text), None);
    }

    #[test]
    fn test_offset_to_position_multiline() {
        let text = "Line 1\nLine 2\nLine 3";
        assert_eq!(offset_to_position(7, text), Some(Position::new(1, 0)));
        assert_eq!(offset_to_position(20, text), Some(Position::new(2, 6)));
    }

    #[test]
    fn test_offset_to_position_unicode_multibyte() {
        let text = "あいう";
        assert_eq!(offset_to_position(0, text), Some(Position::new(0, 0)));
        assert_eq!(offset_to_position(3, text), Some(Position::new(0, 1)));
        assert_eq!(offset_to_position(6, text), Some(Position::new(0, 2)));
        assert_eq!(offset_to_position(9, text), Some(Position::new(0, 3)));
    }

    #[test]
    fn test_offset_to_position_supplementary_plane_chars() {
        let text = "a🎉b";
        assert_eq!(offset_to_position(0, text), Some(Position::new(0, 0)));
        assert_eq!(offset_to_position(1, text), Some(Position::new(0, 1)));
        assert_eq!(offset_to_position(5, text), Some(Position::new(0, 3)));
    }

    #[test]
    fn test_offset_to_position_empty_string() {
        assert_eq!(offset_to_position(0, ""), Some(Position::new(0, 0)));
        assert_eq!(offset_to_position(1, ""), None);
    }

    #[test]
    fn test_positions_le() {
        let p1 = Position::new(0, 5);
        let p2 = Position::new(0, 10);
        assert!(positions_le(p1, p2));
        assert!(!positions_le(p2, p1));

        let p3 = Position::new(1, 0);
        let p4 = Position::new(2, 5);
        assert!(positions_le(p3, p4));
        assert!(!positions_le(p4, p3));

        let p5 = Position::new(0, 5);
        let p6 = Position::new(0, 5);
        assert!(positions_le(p5, p6));
    }

    #[test]
    fn test_to_lsp_diagnostic_basic() {
        let diag = TsuzuLintDiagnostic::new("test-rule", "Test message", Span::new(0, 5));
        let text = "Hello World";

        let lsp_diag = to_lsp_diagnostic(&diag, text).unwrap();

        assert_eq!(
            lsp_diag.range,
            Range::new(Position::new(0, 0), Position::new(0, 5))
        );
        assert_eq!(lsp_diag.message, "Test message");
        assert_eq!(
            lsp_diag.code,
            Some(NumberOrString::String("test-rule".to_string()))
        );
        assert_eq!(lsp_diag.source, Some("tsuzulint".to_string()));
    }

    #[test]
    fn test_to_lsp_diagnostic_severity() {
        let text = "Hello World";

        let error_diag = TsuzuLintDiagnostic::new("r", "m", Span::new(0, 1))
            .with_severity(TsuzuLintSeverity::Error);
        let lsp_error = to_lsp_diagnostic(&error_diag, text).unwrap();
        assert_eq!(lsp_error.severity, Some(DiagnosticSeverity::ERROR));

        let warning_diag = TsuzuLintDiagnostic::new("r", "m", Span::new(0, 1))
            .with_severity(TsuzuLintSeverity::Warning);
        let lsp_warning = to_lsp_diagnostic(&warning_diag, text).unwrap();
        assert_eq!(lsp_warning.severity, Some(DiagnosticSeverity::WARNING));

        let info_diag = TsuzuLintDiagnostic::new("r", "m", Span::new(0, 1))
            .with_severity(TsuzuLintSeverity::Info);
        let lsp_info = to_lsp_diagnostic(&info_diag, text).unwrap();
        assert_eq!(lsp_info.severity, Some(DiagnosticSeverity::INFORMATION));
    }

    #[test]
    fn test_to_lsp_diagnostic_invalid_offset() {
        let diag = TsuzuLintDiagnostic::new("test-rule", "Test", Span::new(0, 100));
        let text = "Hello";

        assert!(to_lsp_diagnostic(&diag, text).is_none());
    }

    #[test]
    fn test_offset_to_range_basic() {
        let text = "Hello World";

        let range = offset_to_range(0, 5, text).unwrap();
        assert_eq!(range, Range::new(Position::new(0, 0), Position::new(0, 5)));

        let range = offset_to_range(6, 11, text).unwrap();
        assert_eq!(range, Range::new(Position::new(0, 6), Position::new(0, 11)));
    }

    #[test]
    fn test_offset_to_range_multiline() {
        let text = "Line 1\nLine 2\nLine 3";

        let range = offset_to_range(0, 12, text).unwrap();
        assert_eq!(range, Range::new(Position::new(0, 0), Position::new(1, 5)));

        let range = offset_to_range(7, 14, text).unwrap();
        assert_eq!(range, Range::new(Position::new(1, 0), Position::new(2, 0)));
    }

    #[test]
    fn test_offset_to_range_unicode() {
        let text = "あいうえお";

        let range = offset_to_range(0, 6, text).unwrap();
        assert_eq!(range, Range::new(Position::new(0, 0), Position::new(0, 2)));
    }

    #[test]
    fn test_offset_to_range_invalid() {
        let text = "Hello";

        assert!(offset_to_range(0, 100, text).is_none());
        assert!(offset_to_range(100, 200, text).is_none());
    }
}
