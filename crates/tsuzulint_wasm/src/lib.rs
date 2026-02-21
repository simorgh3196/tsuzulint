use tsuzulint_ast::AstArena;
use tsuzulint_parser::{MarkdownParser, Parser, PlainTextParser};
use tsuzulint_plugin::{Diagnostic, PluginHost, Severity};
use tsuzulint_text::{SentenceSplitter, Tokenizer};
use wasm_bindgen::prelude::*;

/// Converts any `Display`-implementing error into `JsError`.
///
/// Centralizes the `map_err(|e| JsError::new(&e.to_string()))` pattern
/// used throughout this crate. We cannot use `impl From<E> for JsError`
/// due to the orphan rule (both traits are from external crates).
fn to_js_error(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

#[wasm_bindgen]
pub struct TextLinter {
    host: PluginHost,
    tokenizer: Tokenizer,
}

struct AnalysisData {
    tokens: Vec<tsuzulint_text::Token>,
    sentences: Vec<tsuzulint_text::Sentence>,
}

#[wasm_bindgen]
impl TextLinter {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<TextLinter, JsError> {
        console_error_panic_hook::set_once();
        let host = PluginHost::new();
        let tokenizer = Tokenizer::new()
            .map_err(|e| JsError::new(&format!("Failed to initialize tokenizer: {}", e)))?;

        Ok(Self { host, tokenizer })
    }

    /// Loads a rule from WASM bytes.
    #[wasm_bindgen(js_name = loadRule)]
    pub fn load_rule(&mut self, wasm_bytes: &[u8]) -> Result<String, JsError> {
        let manifest = self.host.load_rule_bytes(wasm_bytes).map_err(to_js_error)?;
        Ok(manifest.name)
    }

    /// Configures a rule.
    #[wasm_bindgen(js_name = configureRule)]
    pub fn configure_rule(&mut self, name: &str, config_json: JsValue) -> Result<(), JsError> {
        let config = serde_wasm_bindgen::from_value(config_json).map_err(to_js_error)?;
        self.host.configure_rule(name, config).map_err(to_js_error)
    }

    /// Prepares text analysis data (tokens, sentences).
    fn prepare_text_analysis(&self, content: &str) -> Result<AnalysisData, JsError> {
        let tokens = self.tokenizer.tokenize(content).map_err(to_js_error)?;

        // TODO: Compute ignore ranges from AST (code blocks, inline code)
        let ignore_ranges: Vec<std::ops::Range<usize>> = Vec::new();
        let sentences = SentenceSplitter::split(content, &ignore_ranges);

        Ok(AnalysisData { tokens, sentences })
    }

    /// Returns the names of all loaded rules.
    #[wasm_bindgen(js_name = getLoadedRules)]
    pub fn loaded_rules(&self) -> Vec<String> {
        self.host.loaded_rules().cloned().collect()
    }

    /// Core linting pipeline shared by [`lint`] and [`lint_json`].
    ///
    /// Parses `content` using the parser selected by `file_type`,
    /// prepares text analysis, and runs all loaded rules.
    fn run_lint(&mut self, content: &str, file_type: &str) -> Result<Vec<Diagnostic>, JsError> {
        let arena = AstArena::new();

        // Select parser based on file type
        let parser: Box<dyn Parser> = match file_type {
            "markdown" | "md" => Box::new(MarkdownParser::new()),
            _ => Box::new(PlainTextParser::new()),
        };

        // Parse the content
        let ast = parser
            .parse(&arena, content)
            .map_err(|e| JsError::new(&format!("Parse error: {}", e)))?;

        // Convert AST to pre-serialized JSON
        let ast_raw = serde_json::value::to_raw_value(&ast).map_err(to_js_error)?;

        // Prepare analysis data
        let analysis = self.prepare_text_analysis(content)?;

        // Run all rules
        self.host
            .run_all_rules_with_parts(
                &ast_raw,
                content,
                &analysis.tokens,
                &analysis.sentences,
                None,
            )
            .map_err(to_js_error)
    }

    /// Lints text content and returns diagnostics as JavaScript objects.
    #[wasm_bindgen]
    pub fn lint(&mut self, content: &str, file_type: &str) -> Result<JsValue, JsError> {
        let diagnostics = self.run_lint(content, file_type)?;
        let js_diagnostics: Vec<JsDiagnostic> =
            diagnostics.into_iter().map(JsDiagnostic::from).collect();
        serde_wasm_bindgen::to_value(&js_diagnostics).map_err(to_js_error)
    }

    /// Lints text content and returns diagnostics as a JSON string.
    ///
    /// This is an alternative to [`lint`] that returns a JSON string
    /// instead of JavaScript objects, which may be more efficient for
    /// some use cases.
    #[wasm_bindgen(js_name = lintJson)]
    pub fn lint_json(&mut self, content: &str, file_type: &str) -> Result<String, JsError> {
        let diagnostics = self.run_lint(content, file_type)?;
        serde_json::to_string(&diagnostics).map_err(to_js_error)
    }
}

impl Default for TextLinter {
    fn default() -> Self {
        Self::new().expect("Failed to initialize TextLinter via default()")
    }
}

/// JavaScript-friendly diagnostic structure.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct JsDiagnostic {
    rule_id: String,
    message: String,
    start: u32,
    end: u32,
    start_line: Option<u32>,
    start_column: Option<u32>,
    end_line: Option<u32>,
    end_column: Option<u32>,
    severity: String,
    fix: Option<JsFix>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct JsFix {
    start: u32,
    end: u32,
    text: String,
}

impl From<Diagnostic> for JsDiagnostic {
    fn from(d: Diagnostic) -> Self {
        Self {
            rule_id: d.rule_id,
            message: d.message,
            start: d.span.start,
            end: d.span.end,
            start_line: d.loc.as_ref().map(|l| l.start.line),
            start_column: d.loc.as_ref().map(|l| l.start.column),
            end_line: d.loc.as_ref().map(|l| l.end.line),
            end_column: d.loc.as_ref().map(|l| l.end.column),
            severity: match d.severity {
                Severity::Error => "error".to_string(),
                Severity::Warning => "warning".to_string(),
                Severity::Info => "info".to_string(),
            },
            fix: d.fix.map(|f| JsFix {
                start: f.span.start,
                end: f.span.end,
                text: f.text,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    #[wasm_bindgen_test]
    fn test_linter_new() {
        let linter = TextLinter::new().unwrap();
        assert!(linter.loaded_rules().is_empty());
    }

    #[wasm_bindgen_test]
    fn test_linter_default() {
        let linter = TextLinter::default();
        assert!(linter.loaded_rules().is_empty());
    }

    #[test]
    fn test_js_diagnostic_from_diagnostic() {
        use tsuzulint_ast::{Location, Position, Span};
        use tsuzulint_plugin::{Diagnostic, Severity};

        let diag = Diagnostic {
            rule_id: "test-rule".to_string(),
            message: "Test message".to_string(),
            span: Span { start: 0, end: 10 },
            loc: Some(Location {
                start: Position { line: 1, column: 0 },
                end: Position {
                    line: 1,
                    column: 10,
                },
            }),
            severity: Severity::Error,
            fix: None,
        };

        let js_diag = JsDiagnostic::from(diag);
        assert_eq!(js_diag.rule_id, "test-rule");
        assert_eq!(js_diag.message, "Test message");
        assert_eq!(js_diag.start, 0);
        assert_eq!(js_diag.end, 10);
        assert_eq!(js_diag.severity, "error");
        assert_eq!(js_diag.start_line, Some(1));
        assert_eq!(js_diag.start_column, Some(0));
        assert_eq!(js_diag.end_line, Some(1));
        assert_eq!(js_diag.end_column, Some(10));
        assert!(js_diag.fix.is_none());
    }

    #[test]
    fn test_js_diagnostic_with_fix() {
        use tsuzulint_ast::Span;
        use tsuzulint_plugin::{Diagnostic, Fix, Severity};

        let diag = Diagnostic {
            rule_id: "test-rule".to_string(),
            message: "Test message".to_string(),
            span: Span { start: 0, end: 10 },
            loc: None,
            severity: Severity::Warning,
            fix: Some(Fix {
                span: Span { start: 0, end: 10 },
                text: "fixed text".to_string(),
            }),
        };

        let js_diag = JsDiagnostic::from(diag);
        assert_eq!(js_diag.severity, "warning");
        assert!(js_diag.fix.is_some());
        let fix = js_diag.fix.unwrap();
        assert_eq!(fix.start, 0);
        assert_eq!(fix.end, 10);
        assert_eq!(fix.text, "fixed text");
    }

    #[test]
    fn test_js_diagnostic_severity_mapping() {
        use tsuzulint_ast::Span;
        use tsuzulint_plugin::{Diagnostic, Severity};

        let test_cases = vec![
            (Severity::Error, "error"),
            (Severity::Warning, "warning"),
            (Severity::Info, "info"),
        ];

        for (severity, expected) in test_cases {
            let diag = Diagnostic {
                rule_id: "test".to_string(),
                message: "test".to_string(),
                span: Span { start: 0, end: 1 },
                loc: None,
                severity,
                fix: None,
            };

            let js_diag = JsDiagnostic::from(diag);
            assert_eq!(js_diag.severity, expected);
        }
    }

    #[test]
    fn test_ast_to_json_document() {
        use tsuzulint_ast::{NodeType, Span, TxtNode};

        let doc = TxtNode::new_parent(NodeType::Document, Span::new(0, 10), &[]);

        let json = serde_json::to_value(doc).unwrap();

        assert_eq!(json["type"], "Document");
        assert_eq!(json["range"][0], 0);
        assert_eq!(json["range"][1], 10);
        assert!(json["children"].is_array());
        assert_eq!(json["children"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_ast_to_json_with_value() {
        use tsuzulint_ast::{AstArena, NodeType, Span, TxtNode};

        let arena = AstArena::new();
        let text_node = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 5), "hello"));

        let json = serde_json::to_value(text_node).unwrap();

        assert!(json["value"].is_string());
        assert_eq!(json["value"], "hello");
    }

    #[test]
    fn test_ast_to_json_with_children() {
        use tsuzulint_ast::{AstArena, NodeType, Span, TxtNode};

        let arena = AstArena::new();
        let child1 = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 5), "hello"));
        let child2 = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(6, 11), "world"));
        let children = arena.alloc_slice_copy(&[*child1, *child2]);
        let parent = TxtNode::new_parent(NodeType::Paragraph, Span::new(0, 11), children);

        let json = serde_json::to_value(parent).unwrap();

        assert!(json["children"].is_array());
        let children_arr = json["children"].as_array().unwrap();
        assert_eq!(children_arr.len(), 2);
    }

    #[test]
    fn test_ast_to_json_with_node_data() {
        use tsuzulint_ast::{AstArena, NodeData, NodeType, Span, TxtNode};

        let arena = AstArena::new();

        // Test header node
        let mut header = TxtNode::new_parent(NodeType::Header, Span::new(0, 10), &[]);
        header.data = NodeData::header(2);
        let json = serde_json::to_value(header).unwrap();
        assert_eq!(json["depth"], 2);

        // Test link node
        let mut link = TxtNode::new_parent(NodeType::Link, Span::new(0, 10), &[]);
        link.data = NodeData::link(
            arena.alloc_str("https://example.com"),
            Some(arena.alloc_str("Example")),
        );
        let json = serde_json::to_value(link).unwrap();
        assert_eq!(json["url"], "https://example.com");
        assert_eq!(json["title"], "Example");

        // Test code block node
        let mut code_block = TxtNode::new_text(
            NodeType::CodeBlock,
            Span::new(0, 10),
            arena.alloc_str("code"),
        );
        code_block.data = NodeData::code_block(Some(arena.alloc_str("rust")));
        let json = serde_json::to_value(code_block).unwrap();
        assert_eq!(json["lang"], "rust");

        // Test list node
        let mut list = TxtNode::new_parent(NodeType::List, Span::new(0, 10), &[]);
        list.data = NodeData::list(true);
        let json = serde_json::to_value(list).unwrap();
        assert_eq!(json["ordered"], true);
    }

    #[wasm_bindgen_test]
    fn test_load_rule_success() {
        let mut linter = TextLinter::new().unwrap();
        // Path relative to crates/tsuzulint_wasm/src/lib.rs
        // This assumes the fixture has been built by tsuzulint_core tests or manual build
        let wasm = include_bytes!(
            "../../tsuzulint_core/tests/fixtures/simple_rule/target/wasm32-wasip1/release/simple_rule.wasm"
        );
        let result = linter.load_rule(wasm);
        assert!(result.is_ok(), "Failed to load rule: {:?}", result.err());
        assert_eq!(result.unwrap(), "test-rule");
    }

    #[wasm_bindgen_test]
    fn test_configure_unknown_rule_fails() {
        let mut linter = TextLinter::new().unwrap();
        let config = serde_json::json!({ "option": "value" });
        let js_val = serde_wasm_bindgen::to_value(&config).unwrap();

        let result = linter.configure_rule("unknown-rule", js_val);
        assert!(result.is_err());
    }

    #[wasm_bindgen_test]
    fn test_configure_rule_success() {
        let mut linter = TextLinter::new().unwrap();
        let wasm = include_bytes!(
            "../../tsuzulint_core/tests/fixtures/simple_rule/target/wasm32-wasip1/release/simple_rule.wasm"
        );
        linter.load_rule(wasm).unwrap();

        let config = serde_json::json!({ "option": "value" });
        let js_val = serde_wasm_bindgen::to_value(&config).unwrap();

        let result = linter.configure_rule("test-rule", js_val);
        assert!(result.is_ok());
    }

    #[wasm_bindgen_test]
    fn test_lint_json_empty() {
        let mut linter = TextLinter::new().unwrap();
        let result = linter.lint_json("text", "txt").unwrap();
        assert_eq!(result, "[]");
    }

    #[wasm_bindgen_test]
    fn test_lint_with_rule() {
        let mut linter = TextLinter::new().unwrap();
        let wasm = include_bytes!(
            "../../tsuzulint_core/tests/fixtures/simple_rule/target/wasm32-wasip1/release/simple_rule.wasm"
        );
        linter.load_rule(wasm).unwrap();

        let content = "This contains error.";
        let result_json = linter.lint_json(content, "txt").unwrap();

        assert!(result_json.contains("test-rule"));
        assert!(result_json.contains("Found error keyword"));
    }
}
