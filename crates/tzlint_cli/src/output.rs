//! Rendering lint results to stdout, in either the human-readable text format or JSON.
//!
//! Byte spans are mapped to 1-based `(line, column)` positions with [`LineIndex`] at render
//! time (the source is kept alongside the diagnostics for exactly this). Diagnostics arrive
//! already in the engine's stable total order, so no re-sorting happens here.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

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

// ── SARIF 2.1.0 output ────────────────────────────────────────────────────────────────
//
// A static analysis interchange format consumed by CI integrations (notably GitHub code
// scanning). The document is built directly from the diagnostics, so it needs no extra
// metadata threading: each diagnostic becomes one `result`, and the rule ids that appear are
// collected into the run's `tool.driver.rules` (referenced by `ruleIndex`).

/// The analysis tool's name, embedded in every SARIF run's `tool.driver`.
const SARIF_TOOL_NAME: &str = "tzlint";
/// The tool version (the crate version), embedded in `tool.driver.version`.
const SARIF_TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");
/// The project home, embedded in `tool.driver.informationUri`.
const SARIF_TOOL_INFO_URI: &str = "https://github.com/simorgh3196/tsuzulint";

/// The SARIF 2.1.0 `level` for a [`Severity`]. SARIF defines only `none`/`note`/`warning`/
/// `error`, so the two informational severities both map to `note`.
fn sarif_level(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info | Severity::Hint => "note",
    }
}

/// A SARIF `artifactLocation.uri`: the path with `\` normalized to `/`, since SARIF URIs use
/// forward slashes regardless of the host platform (so a Windows path stays a valid URI).
fn sarif_uri(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

/// Render `reports` as a SARIF 2.1.0 log (one `run`, one `result` per diagnostic).
///
/// Byte spans map to a 1-based `region`; SARIF's `endColumn` is exclusive (the column after the
/// region), which is exactly what mapping the exclusive `span.end` yields. Rule ids encountered
/// are deduplicated into `tool.driver.rules` in first-appearance order and referenced by
/// `ruleIndex`. Stdin diagnostics keep their `<stdin>` label as the artifact URI.
pub fn render_sarif(writer: &mut dyn Write, reports: &[FileReport]) -> io::Result<()> {
    let mut rule_ids: Vec<&str> = Vec::new();
    let mut results: Vec<Value> = Vec::new();
    for report in reports {
        let index = LineIndex::new(&report.source);
        let uri = sarif_uri(&report.path);
        for diagnostic in &report.diagnostics {
            let rule_id = diagnostic.rule_id.as_str();
            let rule_index = match rule_ids.iter().position(|id| *id == rule_id) {
                Some(i) => i,
                None => {
                    rule_ids.push(rule_id);
                    rule_ids.len() - 1
                }
            };
            let (start_line, start_col) = index.position(&report.source, diagnostic.span.start);
            let (end_line, end_col) = index.position(&report.source, diagnostic.span.end);
            results.push(json!({
                "ruleId": rule_id,
                "ruleIndex": rule_index,
                "level": sarif_level(diagnostic.severity),
                "message": { "text": diagnostic.message },
                "locations": [{
                    "physicalLocation": {
                        "artifactLocation": { "uri": uri },
                        "region": {
                            "startLine": start_line,
                            "startColumn": start_col,
                            "endLine": end_line,
                            "endColumn": end_col,
                        },
                    },
                }],
            }));
        }
    }
    let rules: Vec<Value> = rule_ids.iter().map(|id| json!({ "id": id })).collect();
    let log = json!({
        "version": "2.1.0",
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "runs": [{
            "tool": {
                "driver": {
                    "name": SARIF_TOOL_NAME,
                    "version": SARIF_TOOL_VERSION,
                    "informationUri": SARIF_TOOL_INFO_URI,
                    "rules": rules,
                },
            },
            "results": results,
        }],
    });
    serde_json::to_writer_pretty(&mut *writer, &log).map_err(io::Error::other)?;
    writeln!(writer)
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

    fn render_sarif_value(reports: &[FileReport]) -> Value {
        let mut buf = Vec::new();
        render_sarif(&mut buf, reports).unwrap();
        serde_json::from_slice(&buf).unwrap()
    }

    #[test]
    fn sarif_has_a_valid_2_1_0_envelope() {
        let diag = Diagnostic::new("no-todo", Severity::Warning, Span::new(4, 8), "found TODO");
        let value = render_sarif_value(&[report("doc.md", "壱\nTODO\n", vec![diag])]);
        assert_eq!(value["version"], "2.1.0");
        assert!(value["$schema"].is_string(), "{value}");
        let driver = &value["runs"][0]["tool"]["driver"];
        assert_eq!(driver["name"], "tzlint");
        assert!(driver["version"].is_string(), "{driver}");
        assert!(driver["informationUri"].is_string(), "{driver}");
    }

    #[test]
    fn sarif_result_carries_rule_level_message_and_region() {
        // "壱\n" is 4 bytes, so byte 4 = line 2 col 1; byte 8 = the column after "TODO" =
        // line 2 col 5 (SARIF's exclusive endColumn).
        let diag = Diagnostic::new("no-todo", Severity::Warning, Span::new(4, 8), "found TODO");
        let value = render_sarif_value(&[report("doc.md", "壱\nTODO\n", vec![diag])]);
        let result = &value["runs"][0]["results"][0];
        assert_eq!(result["ruleId"], "no-todo");
        assert_eq!(result["ruleIndex"], 0);
        assert_eq!(result["level"], "warning");
        assert_eq!(result["message"]["text"], "found TODO");
        let physical = &result["locations"][0]["physicalLocation"];
        assert_eq!(physical["artifactLocation"]["uri"], "doc.md");
        assert_eq!(
            physical["region"],
            json!({ "startLine": 2, "startColumn": 1, "endLine": 2, "endColumn": 5 }),
            "{physical}"
        );
    }

    #[test]
    fn sarif_dedupes_rules_and_indexes_results() {
        let d = |rule: &str| Diagnostic::new(rule, Severity::Error, Span::new(0, 1), "m");
        let value = render_sarif_value(&[report("a.md", "x", vec![d("r1"), d("r2"), d("r1")])]);
        let rules = value["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .unwrap()
            .clone();
        // First-appearance order, deduplicated.
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0]["id"], "r1");
        assert_eq!(rules[1]["id"], "r2");
        let results = value["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results[0]["ruleIndex"], 0);
        assert_eq!(results[1]["ruleIndex"], 1);
        assert_eq!(results[2]["ruleIndex"], 0);
    }

    #[test]
    fn sarif_clean_report_has_empty_results_and_rules() {
        let value = render_sarif_value(&[report("a.md", "ok\n", vec![])]);
        assert_eq!(value["runs"][0]["results"], json!([]));
        assert_eq!(value["runs"][0]["tool"]["driver"]["rules"], json!([]));
    }

    #[test]
    fn sarif_level_maps_info_and_hint_to_note() {
        assert_eq!(sarif_level(Severity::Error), "error");
        assert_eq!(sarif_level(Severity::Warning), "warning");
        assert_eq!(sarif_level(Severity::Info), "note");
        assert_eq!(sarif_level(Severity::Hint), "note");
    }

    #[test]
    fn sarif_uri_uses_forward_slashes() {
        assert_eq!(sarif_uri(Path::new("docs/a.md")), "docs/a.md");
        // A backslash path (as Windows `display()` yields) is normalized to URI separators.
        assert_eq!(sarif_uri(Path::new("docs\\a.md")), "docs/a.md");
    }
}
