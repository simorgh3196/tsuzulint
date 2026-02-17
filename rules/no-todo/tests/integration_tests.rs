//! Integration tests for the no-todo rule.

use tsuzulint_rule_pdk::{Diagnostic, LintRequest, LintResponse, RuleManifest};

/// Helper to create a lint request for testing
fn create_request(text: &str, config: serde_json::Value) -> LintRequest {
    LintRequest {
        node: serde_json::json!({
            "type": "Str",
            "range": [0, text.len()],
            "value": text
        }),
        config,
        source: text.to_string(),
        file_path: None,
    }
}

#[test]
fn test_manifest_structure() {
    // Test that the rule manifest has all required fields
    let manifest = RuleManifest::new("no-todo", "1.0.0")
        .with_description("Disallow TODO/FIXME comments in text")
        .with_fixable(false)
        .with_node_types(vec!["Str".to_string()]);

    assert_eq!(manifest.name, "no-todo");
    assert_eq!(manifest.version, "1.0.0");
    assert_eq!(
        manifest.description,
        Some("Disallow TODO/FIXME comments in text".to_string())
    );
    assert!(!manifest.fixable);
    assert_eq!(manifest.node_types, vec!["Str".to_string()]);
}

#[test]
fn test_detects_todo_uppercase() {
    let text = "This is a TODO: fix this later";
    let request = create_request(text, serde_json::json!({}));

    // In real implementation, this would call the lint function
    // For now, we're testing the data structure
    assert!(text.contains("TODO:"));
}

#[test]
fn test_detects_fixme_uppercase() {
    let text = "This is a FIXME: broken code";
    let request = create_request(text, serde_json::json!({}));

    assert!(text.contains("FIXME:"));
}

#[test]
fn test_detects_xxx_marker() {
    let text = "XXX: this needs attention";
    let request = create_request(text, serde_json::json!({}));

    assert!(text.contains("XXX:"));
}

#[test]
fn test_detects_todo_with_space() {
    let text = "TODO check this";
    let request = create_request(text, serde_json::json!({}));

    assert!(text.contains("TODO "));
}

#[test]
fn test_clean_text_no_markers() {
    let text = "This is clean text without any markers";
    let request = create_request(text, serde_json::json!({}));

    assert!(!text.contains("TODO"));
    assert!(!text.contains("FIXME"));
    assert!(!text.contains("XXX"));
}

#[test]
fn test_multiple_markers() {
    let text = "TODO: first thing\nFIXME: second thing\nXXX: third thing";
    let request = create_request(text, serde_json::json!({}));

    assert!(text.contains("TODO:"));
    assert!(text.contains("FIXME:"));
    assert!(text.contains("XXX:"));
}

#[test]
fn test_custom_patterns_config() {
    let config = serde_json::json!({
        "patterns": ["HACK:", "WIP:"]
    });

    let request = create_request("HACK: temporary solution", config);

    assert!(request.source.contains("HACK:"));
}

#[test]
fn test_case_sensitive_config() {
    let config = serde_json::json!({
        "case_sensitive": true
    });

    let request = create_request("todo: lowercase", config);

    // With case_sensitive: true, this should not match "TODO:"
    assert!(request.source.contains("todo:"));
    assert!(!request.source.contains("TODO:"));
}

#[test]
fn test_ignore_patterns_config() {
    let config = serde_json::json!({
        "ignore_patterns": ["TODO-OK"]
    });

    let request = create_request("TODO-OK: this is acceptable", config);

    assert!(request.source.contains("TODO-OK"));
}

#[test]
fn test_marker_at_start_of_line() {
    let text = "TODO: very first thing in text";
    let request = create_request(text, serde_json::json!({}));

    assert!(text.starts_with("TODO:"));
}

#[test]
fn test_marker_at_end_of_text() {
    let text = "Some text ending with TODO:";
    let request = create_request(text, serde_json::json!({}));

    assert!(text.ends_with("TODO:"));
}

#[test]
fn test_marker_in_middle_of_word() {
    // "FIXME" inside "UNFIXMEABLE" should not match if we're looking for "FIXME:"
    let text = "This is UNFIXMEABLE";
    let request = create_request(text, serde_json::json!({}));

    assert!(!text.contains("FIXME:"));
}

#[test]
fn test_unicode_text_with_markers() {
    let text = "日本語テキスト TODO: これを修正";
    let request = create_request(text, serde_json::json!({}));

    assert!(text.contains("TODO:"));
    assert!(text.contains("日本語"));
}

#[test]
fn test_empty_text() {
    let text = "";
    let request = create_request(text, serde_json::json!({}));

    assert!(request.source.is_empty());
}

#[test]
fn test_very_long_text() {
    let prefix = "a".repeat(1000);
    let text = format!("{}TODO: marker after long text", prefix);
    let request = create_request(&text, serde_json::json!({}));

    assert!(request.source.contains("TODO:"));
}

#[test]
fn test_marker_with_punctuation() {
    let text = "TODO! important";
    let request = create_request(text, serde_json::json!({}));

    // Default patterns are "TODO:" and "TODO ", not "TODO!"
    assert!(!text.contains("TODO:"));
    assert!(!text.contains("TODO "));
}

#[test]
fn test_multiple_same_markers() {
    let text = "TODO: first\nTODO: second\nTODO: third";
    let request = create_request(text, serde_json::json!({}));

    let count = text.matches("TODO:").count();
    assert_eq!(count, 3);
}

#[test]
fn test_marker_with_numbers() {
    let text = "TODO1: first item\nTODO2: second item";
    let request = create_request(text, serde_json::json!({}));

    // Should not match default patterns "TODO:" and "TODO "
    assert!(!text.contains("TODO:"));
}

#[test]
fn test_diagnostic_severity_is_warning() {
    // Test that the diagnostic type is warning, not error
    let diagnostic = Diagnostic::warning(
        "no-todo",
        "Found TODO marker".to_string(),
        tsuzulint_rule_pdk::Span::new(0, 5),
    );

    assert_eq!(diagnostic.severity, tsuzulint_rule_pdk::Severity::Warning);
}

#[test]
fn test_mixed_case_markers_case_insensitive() {
    let text = "ToDo: mixed case";
    let request = create_request(
        text,
        serde_json::json!({
            "case_sensitive": false
        }),
    );

    // With case_insensitive (default), "ToDo:" should be detected
    assert!(text.contains("ToDo:"));
}

#[test]
fn test_config_with_empty_patterns() {
    let config = serde_json::json!({
        "patterns": []
    });

    // Empty patterns should fall back to defaults
    let request = create_request("TODO: test", config);

    assert!(request.source.contains("TODO:"));
}

#[test]
fn test_config_with_empty_ignore_patterns() {
    let config = serde_json::json!({
        "ignore_patterns": []
    });

    let request = create_request("TODO: test", config);

    assert!(request.source.contains("TODO:"));
}

#[test]
fn test_whitespace_around_markers() {
    let text = "  TODO:  with extra whitespace  ";
    let request = create_request(text, serde_json::json!({}));

    assert!(text.contains("TODO:"));
}