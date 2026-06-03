//! Test-only helper: run one rule over a markdown source through the real engine.
//!
//! `tzlint_core` (the parser + engine) is a dev-dependency only — it is not part of the crate's
//! normal dependency graph, so the rules crate stays free of the engine (and of any future
//! `tzlint_core → tzlint_rules` cycle).

use tzlint_pdk::{Diagnostic, Rule};

/// Parse `source`, archive it, and run `rule` through `Engine::lint`, returning its diagnostics
/// (engine-sorted, kind-filtered exactly as in production).
pub(crate) fn diagnose(rule: &dyn Rule, source: &str) -> Vec<Diagnostic> {
    let ast = tzlint_core::parse(source).expect("test source parses");
    let bytes = tzlint_ast::to_archive(&ast).expect("archive");
    let archived = tzlint_ast::access(&bytes).expect("access");
    tzlint_core::Engine::lint(archived, None, &[rule])
}
