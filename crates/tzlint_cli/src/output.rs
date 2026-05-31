//! Rendering lint results to stdout, in either the human-readable text format or JSON.
//!
//! Byte spans are mapped to 1-based `(line, column)` positions with [`LineIndex`] at render
//! time (the source is kept alongside the diagnostics for exactly this). Diagnostics arrive
//! already in the engine's stable total order, so no re-sorting happens here.

use std::io::{self, Write};
use std::path::PathBuf;

use serde_json::{Value, json};
use tzlint_core::LineIndex;
use tzlint_pdk::{Diagnostic, Severity};

/// One linted file: its path, the exact source that was linted (needed to map byte offsets to
/// line/column), and the diagnostics the engine produced for it.
pub struct FileReport {
    /// The path as the user supplied it.
    pub path: PathBuf,
    /// The source text that was linted (BOM-stripped by the parser, but byte-for-byte what the
    /// spans index into).
    pub source: String,
    /// Diagnostics in the engine's stable total order.
    pub diagnostics: Vec<Diagnostic>,
}

/// The lowercase wire name for a severity (stable: used in both text and JSON output).
fn severity_str(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
        Severity::Hint => "hint",
    }
}

/// Render `reports` as text: one `path:line:col: severity: message [rule]` line per
/// diagnostic, then a one-line summary.
pub fn render_text(writer: &mut dyn Write, reports: &[FileReport]) -> io::Result<()> {
    let mut total_issues = 0usize;
    for report in reports {
        let index = LineIndex::new(&report.source);
        for diagnostic in &report.diagnostics {
            let (line, column) = index.position(&report.source, diagnostic.span.start);
            writeln!(
                writer,
                "{}:{}:{}: {}: {} [{}]",
                report.path.display(),
                line,
                column,
                severity_str(diagnostic.severity),
                diagnostic.message,
                diagnostic.rule_id,
            )?;
            total_issues += 1;
        }
    }
    writeln!(
        writer,
        "{} file(s) checked, {} issue(s) found",
        reports.len(),
        total_issues,
    )
}

/// Render `reports` as a pretty-printed JSON array of `{ path, diagnostics }` objects.
///
/// Each diagnostic carries the raw byte `span`, the mapped 1-based line/column `position`,
/// the lowercase `severity`, the `rule_id`, the `message`, and any `fixes`.
pub fn render_json(writer: &mut dyn Write, reports: &[FileReport]) -> io::Result<()> {
    let files: Vec<Value> = reports
        .iter()
        .map(|report| {
            let index = LineIndex::new(&report.source);
            let diagnostics: Vec<Value> = report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic_json(&report.source, &index, diagnostic))
                .collect();
            json!({
                "path": report.path.display().to_string(),
                "diagnostics": diagnostics,
            })
        })
        .collect();

    // `serde_json` only fails here on an underlying writer error, which is already `io`.
    serde_json::to_writer_pretty(&mut *writer, &Value::Array(files)).map_err(io::Error::other)?;
    writeln!(writer)
}

/// The JSON object for a single diagnostic.
fn diagnostic_json(source: &str, index: &LineIndex, diagnostic: &Diagnostic) -> Value {
    let (start_line, start_col) = index.position(source, diagnostic.span.start);
    let (end_line, end_col) = index.position(source, diagnostic.span.end);
    let fixes: Vec<Value> = diagnostic
        .fixes
        .iter()
        .map(|fix| {
            let (fix_start_line, fix_start_col) = index.position(source, fix.span.start);
            let (fix_end_line, fix_end_col) = index.position(source, fix.span.end);
            json!({
                "span": { "start": fix.span.start, "end": fix.span.end },
                "position": {
                    "start": { "line": fix_start_line, "column": fix_start_col },
                    "end": { "line": fix_end_line, "column": fix_end_col },
                },
                "replacement": fix.replacement,
            })
        })
        .collect();
    json!({
        "rule_id": diagnostic.rule_id.as_str(),
        "severity": severity_str(diagnostic.severity),
        "message": diagnostic.message,
        "span": { "start": diagnostic.span.start, "end": diagnostic.span.end },
        "position": {
            "start": { "line": start_line, "column": start_col },
            "end": { "line": end_line, "column": end_col },
        },
        "fixes": fixes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tzlint_ast::Span;
    use tzlint_pdk::Fix;

    fn report(path: &str, source: &str, diagnostics: Vec<Diagnostic>) -> FileReport {
        FileReport {
            path: PathBuf::from(path),
            source: source.to_string(),
            diagnostics,
        }
    }

    fn render_text_string(reports: &[FileReport]) -> String {
        let mut buf = Vec::new();
        render_text(&mut buf, reports).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn render_json_value(reports: &[FileReport]) -> Value {
        let mut buf = Vec::new();
        render_json(&mut buf, reports).unwrap();
        serde_json::from_slice(&buf).unwrap()
    }

    #[test]
    fn text_summary_for_no_diagnostics() {
        let out = render_text_string(&[report("a.md", "ok\n", vec![])]);
        assert_eq!(out, "1 file(s) checked, 0 issue(s) found\n");
    }

    #[test]
    fn text_renders_line_col_severity_message_rule() {
        // A diagnostic on the second line: "壱\n" is 4 bytes (3 for 壱 + newline), so offset 4
        // is the first char of line 2 → 2:1.
        let diag = Diagnostic::new("no-todo", Severity::Warning, Span::new(4, 8), "found TODO");
        let out = render_text_string(&[report("doc.md", "壱\nTODO\n", vec![diag])]);
        assert!(
            out.contains("doc.md:2:1: warning: found TODO [no-todo]"),
            "{out}"
        );
        assert!(out.contains("1 file(s) checked, 1 issue(s) found"), "{out}");
    }

    #[test]
    fn text_counts_issues_across_files() {
        let d = |msg: &str| Diagnostic::new("r", Severity::Error, Span::new(0, 1), msg);
        let out = render_text_string(&[
            report("a.md", "x", vec![d("one"), d("two")]),
            report("b.md", "y", vec![]),
        ]);
        assert!(out.contains("2 file(s) checked, 2 issue(s) found"), "{out}");
    }

    #[test]
    fn json_shape_for_no_diagnostics() {
        let value = render_json_value(&[report("a.md", "ok\n", vec![])]);
        assert_eq!(
            value,
            json!([{ "path": "a.md", "diagnostics": [] }]),
            "{value}"
        );
    }

    #[test]
    fn json_includes_span_position_severity_and_fix() {
        let diag = Diagnostic::new("max-ten", Severity::Info, Span::new(4, 7), "msg")
            .with_fix(Fix::replace(Span::new(4, 7), "、"));
        let value = render_json_value(&[report("doc.md", "壱\nabc\n", vec![diag])]);
        let d = &value[0]["diagnostics"][0];
        assert_eq!(d["rule_id"], "max-ten");
        assert_eq!(d["severity"], "info");
        assert_eq!(d["span"], json!({ "start": 4, "end": 7 }));
        assert_eq!(d["position"]["start"], json!({ "line": 2, "column": 1 }));
        assert_eq!(d["fixes"][0]["replacement"], "、");
        assert_eq!(d["fixes"][0]["span"], json!({ "start": 4, "end": 7 }));
        assert_eq!(
            d["fixes"][0]["position"]["start"],
            json!({ "line": 2, "column": 1 })
        );
        assert_eq!(
            d["fixes"][0]["position"]["end"],
            json!({ "line": 2, "column": 4 })
        );
    }

    #[test]
    fn severity_names_are_lowercase() {
        assert_eq!(severity_str(Severity::Error), "error");
        assert_eq!(severity_str(Severity::Warning), "warning");
        assert_eq!(severity_str(Severity::Info), "info");
        assert_eq!(severity_str(Severity::Hint), "hint");
    }
}
