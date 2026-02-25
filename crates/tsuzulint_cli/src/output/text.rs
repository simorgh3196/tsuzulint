//! Text output formatter

use std::collections::HashMap;
use std::time::Duration;

use tsuzulint_core::{LintResult, Severity};

pub fn output_text(results: &[LintResult], timings: bool) {
    for result in results {
        if result.diagnostics.is_empty() {
            continue;
        }

        println!("\n{}:", result.path.display());
        for diag in &result.diagnostics {
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
    let total_issues: usize = results.iter().map(|r| r.diagnostics.len()).sum();
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
