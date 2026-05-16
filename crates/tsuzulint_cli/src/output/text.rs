//! Text output formatter

use std::collections::HashMap;
use std::time::Duration;

use tsuzulint_core::{Certainty, LintResult, Severity};

pub(crate) fn count_displayable_issues(results: &[LintResult]) -> usize {
    results
        .iter()
        .map(|r| {
            r.diagnostics
                .iter()
                .filter(|d| d.certainty != Certainty::Heuristic)
                .count()
        })
        .sum()
}

pub fn output_text(results: &[LintResult], timings: bool) {
    let mut stdout = std::io::stdout();
    let _ = output_text_to(&mut stdout, results, timings);
}

pub(crate) fn output_text_to<W: std::io::Write>(
    mut writer: W,
    results: &[LintResult],
    timings: bool,
) -> std::io::Result<()> {
    for result in results {
        let displayable_diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.certainty != Certainty::Heuristic)
            .collect();

        if displayable_diags.is_empty() {
            continue;
        }

        writeln!(writer, "\n{}:", result.path.display())?;
        for diag in displayable_diags {
            let severity = match diag.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Info => "info",
            };
            writeln!(
                writer,
                "  {}:{} {} [{}]: {}",
                diag.span.start, diag.span.end, severity, diag.rule_id, diag.message
            )?;
        }
    }

    let total_files = results.len();
    let total_issues = count_displayable_issues(results);
    let cached = results.iter().filter(|r| r.from_cache).count();

    writeln!(writer)?;
    writeln!(
        writer,
        "Checked {} files ({} from cache), found {} issues",
        total_files, cached, total_issues
    )?;

    if timings {
        output_timings_to(&mut writer, results)?;
    }

    Ok(())
}

fn output_timings_to<W: std::io::Write>(
    mut writer: W,
    results: &[LintResult],
) -> std::io::Result<()> {
    let mut total_duration = Duration::new(0, 0);
    let mut rule_timings: HashMap<&str, Duration> = HashMap::new();

    for result in results {
        for (rule, duration) in &result.timings {
            *rule_timings.entry(rule.as_str()).or_default() += *duration;
            total_duration += *duration;
        }
    }

    if !rule_timings.is_empty() {
        writeln!(writer, "\nPerformance Timings:")?;
        writeln!(writer, "{:<30} | {:<15} | {:<10}", "Rule", "Duration", "%")?;
        writeln!(writer, "{:-<30}-+-{:-<15}-+-{:-<10}", "", "", "")?;

        let mut sorted_timings: Vec<_> = rule_timings.into_iter().collect();
        sorted_timings.sort_by(|a, b| b.1.cmp(&a.1));

        for (rule, duration) in sorted_timings {
            let percentage = if total_duration.as_secs_f64() > 0.0 {
                (duration.as_secs_f64() / total_duration.as_secs_f64()) * 100.0
            } else {
                0.0
            };
            writeln!(
                writer,
                "{:<30} | {:<15?} | {:<10.1}%",
                rule, duration, percentage
            )?;
        }
        writeln!(writer, "{:-<30}-+-{:-<15}-+-{:-<10}", "", "", "")?;
        writeln!(writer, "{:<30} | {:<15?}", "Total", total_duration)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::count_displayable_issues;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use tsuzulint_core::{Diagnostic, LintResult};

    #[test]
    fn test_count_displayable_issues() {
        let diag1: Diagnostic = serde_json::from_value(serde_json::json!({
            "rule_id": "rule1",
            "message": "error",
            "span": { "start": 0, "end": 5 },
            "severity": "error",
            "certainty": "certain"
        }))
        .unwrap();

        let diag2: Diagnostic = serde_json::from_value(serde_json::json!({
            "rule_id": "rule2",
            "message": "hint",
            "span": { "start": 0, "end": 5 },
            "severity": "warning",
            "certainty": "heuristic"
        }))
        .unwrap();

        let result = LintResult {
            path: PathBuf::from("test.md"),
            diagnostics: vec![diag1, diag2],
            from_cache: false,
            timings: HashMap::new(),
        };

        // We only expect 1 displayable issue (the Certain one)
        assert_eq!(count_displayable_issues(&[result]), 1);
    }

    #[test]
    fn test_output_text_to_basic() {
        let mut buf = Vec::new();
        let result = LintResult {
            path: PathBuf::from("test.md"),
            diagnostics: vec![],
            from_cache: false,
            timings: HashMap::new(),
        };

        super::output_text_to(&mut buf, &[result], false).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("Checked 1 files (0 from cache), found 0 issues"));
        assert!(!out.contains("Performance Timings"));
    }

    #[test]
    fn test_output_text_to_with_timings() {
        let mut buf = Vec::new();
        let mut timings = HashMap::new();
        timings.insert("rule-a".to_string(), std::time::Duration::from_millis(50));
        timings.insert("rule-b".to_string(), std::time::Duration::from_millis(150));

        let result = LintResult {
            path: PathBuf::from("test.md"),
            diagnostics: vec![],
            from_cache: false,
            timings,
        };

        super::output_text_to(&mut buf, &[result], true).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("Checked 1 files (0 from cache), found 0 issues"));
        assert!(out.contains("Performance Timings:"));
        assert!(out.contains("rule-a"));
        assert!(out.contains("rule-b"));
        assert!(out.contains("Total"));
    }

    #[test]
    fn test_output_text_to_with_diagnostics() {
        let mut buf = Vec::new();

        let diag: Diagnostic = serde_json::from_value(serde_json::json!({
            "rule_id": "rule-err",
            "message": "this is an error",
            "span": { "start": 10, "end": 20 },
            "severity": "error",
            "certainty": "certain"
        }))
        .unwrap();

        let diag_warn: Diagnostic = serde_json::from_value(serde_json::json!({
            "rule_id": "rule-warn",
            "message": "this is a warning",
            "span": { "start": 30, "end": 40 },
            "severity": "warning",
            "certainty": "certain"
        }))
        .unwrap();

        let diag_info: Diagnostic = serde_json::from_value(serde_json::json!({
            "rule_id": "rule-info",
            "message": "this is info",
            "span": { "start": 50, "end": 60 },
            "severity": "info",
            "certainty": "certain"
        }))
        .unwrap();

        let result = LintResult {
            path: PathBuf::from("test.md"),
            diagnostics: vec![diag, diag_warn, diag_info],
            from_cache: true,
            timings: HashMap::new(),
        };

        super::output_text_to(&mut buf, &[result], false).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("test.md:"));
        assert!(out.contains("10:20 error [rule-err]: this is an error"));
        assert!(out.contains("30:40 warning [rule-warn]: this is a warning"));
        assert!(out.contains("50:60 info [rule-info]: this is info"));
        assert!(out.contains("Checked 1 files (1 from cache), found 3 issues"));
    }
}
