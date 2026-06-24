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
/// Each diagnostic carries the raw byte `span`, the mapped 1-based `position` (each end with a
/// scalar-value `column` and an additive UTF-16 `utf16Column`), the lowercase `severity`, the
/// `rule_id`, the `message`, and any `fixes`.
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
    serde_json::to_writer_pretty(&mut *writer, &files).map_err(io::Error::other)?;
    writeln!(writer)
}

/// One end of a `position`: the 1-based `line`, the scalar-value `column` (the original key,
/// left untouched), and the additive `utf16Column` — the UTF-16 code-unit column editors and the
/// LSP address text by. See [`LineIndex::utf16_column`].
fn point(index: &LineIndex, source: &str, offset: u32) -> Value {
    let (line, column) = index.position(source, offset);
    json!({
        "line": line,
        "column": column,
        "utf16Column": index.utf16_column(source, offset),
    })
}

/// The JSON object for a single diagnostic.
fn diagnostic_json(source: &str, index: &LineIndex, diagnostic: &Diagnostic) -> Value {
    let fixes: Vec<Value> = diagnostic
        .fixes
        .iter()
        .map(|fix| {
            json!({
                "span": { "start": fix.span.start, "end": fix.span.end },
                "position": {
                    "start": point(index, source, fix.span.start),
                    "end": point(index, source, fix.span.end),
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
            "start": point(index, source, diagnostic.span.start),
            "end": point(index, source, diagnostic.span.end),
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

// ── `rules` subcommand output ─────────────────────────────────────────────────────────
//
// Renders the effective built-in rule set (see [`RuleInfo`]) for the `rules list` / `rules
// explain` subcommands — separate from the diagnostic renderers above.

use crate::rules::RuleInfo;

/// Render the rule list as aligned `id  on|off  severity` columns, then a summary line.
pub fn render_rule_list_text(writer: &mut dyn Write, infos: &[RuleInfo]) -> io::Result<()> {
    let id_width = infos.iter().map(|info| info.id.len()).max().unwrap_or(0);
    let enabled = infos.iter().filter(|info| info.enabled).count();
    for info in infos {
        writeln!(
            writer,
            "{:<id_width$}  {:<3}  {}",
            info.id,
            if info.enabled { "on" } else { "off" },
            severity_str(info.severity),
        )?;
    }
    writeln!(
        writer,
        "{} built-in rule(s), {enabled} enabled",
        infos.len(),
    )
}

/// Render the rule list as a JSON array of `{ id, enabled, severity }` objects.
pub fn render_rule_list_json(writer: &mut dyn Write, infos: &[RuleInfo]) -> io::Result<()> {
    let rules: Vec<Value> = infos
        .iter()
        .map(|info| {
            json!({
                "id": info.id,
                "enabled": info.enabled,
                "severity": severity_str(info.severity),
            })
        })
        .collect();
    serde_json::to_writer_pretty(&mut *writer, &rules).map_err(io::Error::other)?;
    writeln!(writer)
}

/// Render one rule's effective state (`rules explain`): status, severity (noting whether it is a
/// config override or the default), and config-supplied options if any.
pub fn render_rule_explain(writer: &mut dyn Write, info: &RuleInfo) -> io::Result<()> {
    writeln!(writer, "rule:     {}", info.id)?;
    writeln!(
        writer,
        "status:   {}",
        if info.enabled {
            "enabled"
        } else {
            "disabled (by config)"
        },
    )?;
    let severity_note = if info.severity_overridden {
        "overridden by config"
    } else {
        "default"
    };
    writeln!(
        writer,
        "severity: {} ({severity_note})",
        severity_str(info.severity),
    )?;
    match &info.options {
        // `serde_json::to_string` only errors on a non-string map key, which a parsed config
        // value never has; surface it as an io error rather than unwrapping regardless.
        Some(options) => {
            let rendered = serde_json::to_string(options).map_err(io::Error::other)?;
            writeln!(writer, "options:  {rendered} (from config)")
        }
        None => writeln!(writer, "options:  (defaults)"),
    }
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
        // "壱\nabc\n": the span starts at 'a' (line 2, col 1). All BMP, so utf16Column == column.
        assert_eq!(
            d["position"]["start"],
            json!({ "line": 2, "column": 1, "utf16Column": 1 })
        );
        assert_eq!(d["fixes"][0]["replacement"], "、");
        assert_eq!(d["fixes"][0]["span"], json!({ "start": 4, "end": 7 }));
        assert_eq!(
            d["fixes"][0]["position"]["start"],
            json!({ "line": 2, "column": 1, "utf16Column": 1 })
        );
        assert_eq!(
            d["fixes"][0]["position"]["end"],
            json!({ "line": 2, "column": 4, "utf16Column": 4 })
        );
    }

    #[test]
    fn json_contract_is_stable() {
        // The full JSON contract the `editors/vscode/` extension parses (see docs/json-output.md).
        // Pin every key and nesting level so an accidental shape change fails loudly. Source
        // "あ x\n": あ = bytes 0..3, space 3, 'x' 4..5 — so the span over 'x' is at column 3.
        let diag = Diagnostic::new("no-todo", Severity::Warning, Span::new(4, 5), "found x")
            .with_fix(Fix::replace(Span::new(4, 5), "y"));
        let value = render_json_value(&[report("doc.md", "あ x\n", vec![diag])]);
        assert_eq!(
            value,
            json!([
                {
                    "path": "doc.md",
                    "diagnostics": [
                        {
                            "rule_id": "no-todo",
                            "severity": "warning",
                            "message": "found x",
                            "span": { "start": 4, "end": 5 },
                            "position": {
                                "start": { "line": 1, "column": 3, "utf16Column": 3 },
                                "end": { "line": 1, "column": 4, "utf16Column": 4 },
                            },
                            "fixes": [
                                {
                                    "span": { "start": 4, "end": 5 },
                                    "position": {
                                        "start": { "line": 1, "column": 3, "utf16Column": 3 },
                                        "end": { "line": 1, "column": 4, "utf16Column": 4 },
                                    },
                                    "replacement": "y",
                                }
                            ],
                        }
                    ],
                }
            ]),
            "{value:#}"
        );
    }

    #[test]
    fn json_carries_a_utf16_column_alongside_the_scalar_column() {
        // "😀" is an astral-plane char: one scalar value but two UTF-16 code units. The diagnostic
        // covers the 'x' that follows it, so the UTF-16 column runs ahead of the scalar column —
        // and the scalar `column` key is untouched.
        let diag = Diagnostic::new("no-todo", Severity::Warning, Span::new(4, 5), "msg")
            .with_fix(Fix::replace(Span::new(4, 5), "y"));
        let value = render_json_value(&[report("a.md", "😀x\n", vec![diag])]);
        let d = &value[0]["diagnostics"][0];
        assert_eq!(
            d["position"]["start"],
            json!({ "line": 1, "column": 2, "utf16Column": 3 })
        );
        assert_eq!(
            d["position"]["end"],
            json!({ "line": 1, "column": 3, "utf16Column": 4 })
        );
        // The fix position carries it too.
        assert_eq!(
            d["fixes"][0]["position"]["start"],
            json!({ "line": 1, "column": 2, "utf16Column": 3 })
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

    fn ri(id: &'static str, enabled: bool, severity: Severity) -> RuleInfo {
        RuleInfo {
            id,
            enabled,
            severity,
            severity_overridden: false,
            options: None,
        }
    }

    fn render_to_string(f: impl FnOnce(&mut Vec<u8>) -> io::Result<()>) -> String {
        let mut buf = Vec::new();
        f(&mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn rule_list_text_lists_state_and_summarizes() {
        let infos = vec![
            ri("max-ten", true, Severity::Warning),
            ri("sentence-length", false, Severity::Error),
        ];
        let out = render_to_string(|w| render_rule_list_text(w, &infos));
        let lines: Vec<&str> = out.lines().collect();
        assert!(
            lines
                .iter()
                .any(|l| l.contains("max-ten") && l.contains("on") && l.contains("warning")),
            "{out}"
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains("sentence-length") && l.contains("off") && l.contains("error")),
            "{out}"
        );
        assert!(out.contains("2 built-in rule(s), 1 enabled"), "{out}");
    }

    #[test]
    fn rule_list_text_aligns_the_severity_column() {
        // Differing id lengths are padded so the on/off and severity columns line up.
        let infos = vec![
            ri("max-ten", true, Severity::Warning),
            ri("no-mixed-zenkaku-hankaku-alphabet", true, Severity::Warning),
        ];
        let out = render_to_string(|w| render_rule_list_text(w, &infos));
        let cols: Vec<usize> = out
            .lines()
            .filter(|l| l.contains("warning"))
            .map(|l| l.find("warning").unwrap())
            .collect();
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0], cols[1], "severity column should align: {out:?}");
    }

    #[test]
    fn rule_list_json_is_an_array_of_objects() {
        let infos = vec![
            ri("max-ten", true, Severity::Warning),
            ri("no-todo", false, Severity::Info),
        ];
        let mut buf = Vec::new();
        render_rule_list_json(&mut buf, &infos).unwrap();
        let value: Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(
            value[0],
            json!({ "id": "max-ten", "enabled": true, "severity": "warning" })
        );
        assert_eq!(
            value[1],
            json!({ "id": "no-todo", "enabled": false, "severity": "info" })
        );
    }

    #[test]
    fn rule_explain_default_shows_defaults() {
        let out =
            render_to_string(|w| render_rule_explain(w, &ri("max-ten", true, Severity::Warning)));
        assert!(out.contains("rule:     max-ten"), "{out}");
        assert!(out.contains("status:   enabled"), "{out}");
        assert!(out.contains("severity: warning (default)"), "{out}");
        assert!(out.contains("options:  (defaults)"), "{out}");
    }

    #[test]
    fn rule_explain_shows_override_disabled_and_options() {
        let info = RuleInfo {
            id: "max-ten",
            enabled: false,
            severity: Severity::Error,
            severity_overridden: true,
            options: Some(json!({ "max": 0 })),
        };
        let out = render_to_string(|w| render_rule_explain(w, &info));
        assert!(out.contains("status:   disabled (by config)"), "{out}");
        assert!(
            out.contains("severity: error (overridden by config)"),
            "{out}"
        );
        assert!(out.contains("options:  {\"max\":0} (from config)"), "{out}");
    }

    #[test]
    fn renderers_propagate_writer_errors() {
        // A zero-capacity `&mut [u8]` sink fails (`WriteZero`) on any non-empty write, exercising
        // each renderer's error-propagation arm — the module's output policy is that a failed
        // result write surfaces as an error (the CLI maps it to `ExitStatus::Error`).
        fn write_fails(render: impl FnOnce(&mut dyn Write) -> io::Result<()>) -> bool {
            let mut sink: &mut [u8] = &mut [];
            render(&mut sink).is_err()
        }
        let reports = [report(
            "a.md",
            "x\n",
            vec![Diagnostic::new(
                "no-todo",
                Severity::Warning,
                Span::new(0, 1),
                "m",
            )],
        )];
        assert!(write_fails(|w| render_text(w, &reports)));
        assert!(write_fails(|w| render_json(w, &reports)));
        assert!(write_fails(|w| render_sarif(w, &reports)));

        let infos = [ri("max-ten", true, Severity::Warning)];
        assert!(write_fails(|w| render_rule_list_text(w, &infos)));
        assert!(write_fails(|w| render_rule_list_json(w, &infos)));
        assert!(write_fails(|w| render_rule_explain(w, &infos[0])));
    }
}
