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
    for result in results {
        let displayable_diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.certainty != Certainty::Heuristic)
            .collect();

        if displayable_diags.is_empty() {
            continue;
        }

        println!("\n{}:", result.path.display());
        for diag in displayable_diags {
            let severity = match diag.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Info => "info",
            };
            println!(
                "  {}:{} {} [{}]: {}",
                diag.span.start, diag.span.end, severity, diag.rule_id, diag.message
            );
        }
    }

    let total_files = results.len();
    let total_issues = count_displayable_issues(results);
    let cached = results.iter().filter(|r| r.from_cache).count();

    println!();
    println!(
        "Checked {} files ({} from cache), found {} issues",
        total_files, cached, total_issues
    );

    if timings {
        output_timings(results);
    }
}

fn output_timings(results: &[LintResult]) {
    let mut total_duration = Duration::new(0, 0);
    let mut rule_timings: HashMap<String, Duration> = HashMap::new();

    for result in results {
        for (rule, duration) in &result.timings {
            *rule_timings.entry(rule.clone()).or_default() += *duration;
            total_duration += *duration;
        }
    }

    if !rule_timings.is_empty() {
        println!("\nPerformance Timings:");
        println!("{:<30} | {:<15} | {:<10}", "Rule", "Duration", "%");
        println!("{:-<30}-+-{:-<15}-+-{:-<10}", "", "", "");

        let mut sorted_timings: Vec<_> = rule_timings.into_iter().collect();
        sorted_timings.sort_by(|a, b| b.1.cmp(&a.1));

        for (rule, duration) in sorted_timings {
            let percentage = if total_duration.as_secs_f64() > 0.0 {
                (duration.as_secs_f64() / total_duration.as_secs_f64()) * 100.0
            } else {
                0.0
            };
            println!("{:<30} | {:<15?} | {:<10.1}%", rule, duration, percentage);
        }
        println!("{:-<30}-+-{:-<15}-+-{:-<10}", "", "", "");
        println!("{:<30} | {:<15?}", "Total", total_duration);
    }
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
}
