//! SARIF (Static Analysis Results Interchange Format) output formatter.
//!
//! Implements SARIF 2.1.0 format for integration with GitHub Advanced Security
//! and other CI/CD tools.

use serde::Serialize;
use std::collections::HashMap;

use texide_plugin::{Diagnostic, Severity};

use crate::LintResult;

/// SARIF version constant
const SARIF_VERSION: &str = "2.1.0";

/// Tool information for SARIF
const TOOL_NAME: &str = "texide";

/// Generates SARIF output from lint results
pub fn generate_sarif(results: &[LintResult]) -> Result<String, serde_json::Error> {
    let sarif_log = SarifLog::from_results(results);
    serde_json::to_string_pretty(&sarif_log)
}

/// Root SARIF log structure
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SarifLog {
    #[serde(rename = "$schema")]
    schema: String,
    version: String,
    runs: Vec<Run>,
}

impl SarifLog {
    /// Creates a SARIF log from lint results
    fn from_results(results: &[LintResult]) -> Self {
        let run = Run::from_results(results);
        Self {
            schema: "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json".to_string(),
            version: SARIF_VERSION.to_string(),
            runs: vec![run],
        }
    }
}

/// A single run of the tool
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Run {
    tool: Tool,
    results: Vec<SarifResult>,
}

impl Run {
    /// Creates a run from lint results
    fn from_results(lint_results: &[LintResult]) -> Self {
        let mut results = Vec::new();
        let mut rules_map: HashMap<String, ReportingDescriptor> = HashMap::new();

        for lint_result in lint_results {
            for diagnostic in &lint_result.diagnostics {
                // Add result
                let sarif_result = SarifResult::from_diagnostic(diagnostic, &lint_result.path);
                results.push(sarif_result);

                // Track unique rules
                if !rules_map.contains_key(&diagnostic.rule_id) {
                    rules_map.insert(
                        diagnostic.rule_id.clone(),
                        ReportingDescriptor::new(&diagnostic.rule_id, &diagnostic.message),
                    );
                }
            }
        }

        Self {
            tool: Tool::new(rules_map),
            results,
        }
    }
}

/// Tool information
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Tool {
    driver: ToolComponent,
}

impl Tool {
    /// Creates tool information with rules
    fn new(rules: HashMap<String, ReportingDescriptor>) -> Self {
        Self {
            driver: ToolComponent::new(rules),
        }
    }
}

/// Tool component (driver)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolComponent {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    rules: Vec<ReportingDescriptor>,
}

impl ToolComponent {
    /// Creates a tool component
    fn new(rules: HashMap<String, ReportingDescriptor>) -> Self {
        let rules_vec: Vec<_> = rules.into_values().collect();
        Self {
            name: TOOL_NAME.to_string(),
            version: option_env!("CARGO_PKG_VERSION").map(|s| s.to_string()),
            rules: rules_vec,
        }
    }
}

/// Rule descriptor
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportingDescriptor {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    short_description: Option<Message>,
}

impl ReportingDescriptor {
    /// Creates a rule descriptor
    fn new(id: &str, message: &str) -> Self {
        Self {
            id: id.to_string(),
            name: Some(id.to_string()),
            short_description: Some(Message::text(message)),
        }
    }
}

/// A message
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Message {
    text: String,
}

impl Message {
    /// Creates a simple text message
    fn text(s: impl Into<String>) -> Self {
        Self { text: s.into() }
    }
}

/// A single result (diagnostic)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SarifResult {
    rule_id: String,
    level: String,
    message: Message,
    locations: Vec<Location>,
}

impl SarifResult {
    /// Creates a SARIF result from a diagnostic
    fn from_diagnostic(diagnostic: &Diagnostic, path: &std::path::Path) -> Self {
        let level = match diagnostic.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "note",
        };

        Self {
            rule_id: diagnostic.rule_id.clone(),
            level: level.to_string(),
            message: Message::text(&diagnostic.message),
            locations: vec![Location::from_diagnostic(diagnostic, path)],
        }
    }
}

/// Location information
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Location {
    physical_location: PhysicalLocation,
}

impl Location {
    /// Creates a location from diagnostic
    fn from_diagnostic(diagnostic: &Diagnostic, path: &std::path::Path) -> Self {
        Self {
            physical_location: PhysicalLocation::from_diagnostic(diagnostic, path),
        }
    }
}

/// Physical location
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PhysicalLocation {
    artifact_location: ArtifactLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    region: Option<Region>,
}

impl PhysicalLocation {
    /// Creates physical location
    fn from_diagnostic(diagnostic: &Diagnostic, path: &std::path::Path) -> Self {
        let region = diagnostic.loc.as_ref().map(Region::from_location);

        Self {
            artifact_location: ArtifactLocation::new(path),
            region,
        }
    }
}

/// Artifact location (file path)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactLocation {
    uri: String,
}

impl ArtifactLocation {
    /// Creates artifact location from path
    fn new(path: &std::path::Path) -> Self {
        Self {
            uri: path.to_string_lossy().to_string(),
        }
    }
}

/// Region (line/column information)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Region {
    #[serde(skip_serializing_if = "Option::is_none")]
    start_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_column: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_column: Option<u32>,
}

impl Region {
    /// Creates region from location
    fn from_location(loc: &texide_ast::Location) -> Self {
        Self {
            start_line: Some(loc.start.line),
            start_column: Some(loc.start.column),
            end_line: Some(loc.end.line),
            end_column: Some(loc.end.column),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use texide_ast::{Location, Position, Span};

    fn create_test_diagnostic(
        rule_id: &str,
        message: &str,
        severity: Severity,
        start: u32,
        end: u32,
    ) -> Diagnostic {
        Diagnostic::new(rule_id, message, Span::new(start, end)).with_severity(severity)
    }

    #[test]
    fn test_sarif_empty_results() {
        let results: Vec<LintResult> = vec![];
        let sarif = generate_sarif(&results).unwrap();

        // Verify it's valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&sarif).unwrap();
        assert_eq!(parsed["version"], "2.1.0");
        // SARIF requires at least one run even with empty results
        assert_eq!(parsed["runs"].as_array().unwrap().len(), 1);
        assert!(parsed["runs"][0]["results"].as_array().unwrap().is_empty());
        assert_eq!(parsed["runs"][0]["tool"]["driver"]["name"], "texide");
    }

    #[test]
    fn test_sarif_single_error() {
        let diagnostic = create_test_diagnostic("no-todo", "Found TODO", Severity::Error, 10, 14);
        let result = LintResult::new(PathBuf::from("test.md"), vec![diagnostic]);

        let sarif = generate_sarif(&[result]).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&sarif).unwrap();

        assert_eq!(parsed["version"], "2.1.0");
        assert_eq!(parsed["runs"].as_array().unwrap().len(), 1);

        let run = &parsed["runs"][0];
        assert_eq!(run["tool"]["driver"]["name"], "texide");
        assert_eq!(run["results"].as_array().unwrap().len(), 1);

        let result = &run["results"][0];
        assert_eq!(result["ruleId"], "no-todo");
        assert_eq!(result["level"], "error");
        assert_eq!(result["message"]["text"], "Found TODO");
    }

    #[test]
    fn test_sarif_multiple_results() {
        let diag1 = create_test_diagnostic("rule1", "Error 1", Severity::Error, 0, 5);
        let diag2 = create_test_diagnostic("rule2", "Warning 1", Severity::Warning, 10, 15);
        let result1 = LintResult::new(PathBuf::from("file1.md"), vec![diag1]);
        let result2 = LintResult::new(PathBuf::from("file2.md"), vec![diag2]);

        let sarif = generate_sarif(&[result1, result2]).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&sarif).unwrap();

        let run = &parsed["runs"][0];
        assert_eq!(run["results"].as_array().unwrap().len(), 2);

        // Check first result
        assert_eq!(run["results"][0]["ruleId"], "rule1");
        assert_eq!(run["results"][0]["level"], "error");

        // Check second result
        assert_eq!(run["results"][1]["ruleId"], "rule2");
        assert_eq!(run["results"][1]["level"], "warning");
    }

    #[test]
    fn test_sarif_severity_mapping() {
        let error = create_test_diagnostic("e", "msg", Severity::Error, 0, 1);
        let warning = create_test_diagnostic("w", "msg", Severity::Warning, 0, 1);
        let info = create_test_diagnostic("i", "msg", Severity::Info, 0, 1);

        let result = LintResult::new(PathBuf::from("test.md"), vec![error, warning, info]);
        let sarif = generate_sarif(&[result]).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&sarif).unwrap();

        let results = parsed["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results[0]["level"], "error");
        assert_eq!(results[1]["level"], "warning");
        assert_eq!(results[2]["level"], "note");
    }

    #[test]
    fn test_sarif_with_location() {
        let loc = Location::new(Position::new(5, 10), Position::new(5, 20));
        let diagnostic = Diagnostic::new("rule", "message", Span::new(100, 110))
            .with_severity(Severity::Warning)
            .with_location(loc);

        let result = LintResult::new(PathBuf::from("test.md"), vec![diagnostic]);
        let sarif = generate_sarif(&[result]).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&sarif).unwrap();

        let location = &parsed["runs"][0]["results"][0]["locations"][0];
        let region = &location["physicalLocation"]["region"];
        assert_eq!(region["startLine"], 5);
        assert_eq!(region["startColumn"], 10);
        assert_eq!(region["endLine"], 5);
        assert_eq!(region["endColumn"], 20);
    }

    #[test]
    fn test_sarif_rules_collection() {
        let diag1 = create_test_diagnostic("rule-a", "Message A", Severity::Error, 0, 5);
        let diag2 = create_test_diagnostic("rule-b", "Message B", Severity::Warning, 10, 15);
        let diag3 = create_test_diagnostic("rule-a", "Message A again", Severity::Error, 20, 25);

        let result = LintResult::new(PathBuf::from("test.md"), vec![diag1, diag2, diag3]);
        let sarif = generate_sarif(&[result]).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&sarif).unwrap();

        let rules = parsed["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .unwrap();
        assert_eq!(rules.len(), 2); // Only 2 unique rules

        let rule_ids: Vec<&str> = rules.iter().map(|r| r["id"].as_str().unwrap()).collect();
        assert!(rule_ids.contains(&"rule-a"));
        assert!(rule_ids.contains(&"rule-b"));
    }

    #[test]
    fn test_sarif_schema_url() {
        let results: Vec<LintResult> = vec![];
        let sarif = generate_sarif(&results).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&sarif).unwrap();

        assert!(
            parsed["$schema"]
                .as_str()
                .unwrap()
                .contains("sarif-schema-2.1.0.json")
        );
    }
}
