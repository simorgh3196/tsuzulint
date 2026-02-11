//! # tsuzulint_wasm
//!
//! WebAssembly bindings for TsuzuLint, enabling browser-based text linting.
//!
//! This crate provides JavaScript/TypeScript bindings for the TsuzuLint linter
//! using wasm-bindgen. It enables running the complete linter in the browser
//! with support for dynamically loading WASM rules.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │ Browser (JavaScript)                    │
//! │   ↓                                     │
//! │ tsuzulint_wasm (this crate)                │
//! │   ↓                                     │
//! │ tsuzulint_plugin (browser feature)         │
//! │   ↓                                     │
//! │ wasmi (pure Rust WASM interpreter)      │
//! │   ↓                                     │
//! │ WASM Rules (same as native)             │
//! └─────────────────────────────────────────┘
//! ```
//!
//! ## Usage (JavaScript/TypeScript)
//!
//! ```javascript
//! import init, { TextLinter } from 'tsuzulint-wasm';
//!
//! async function main() {
//!   await init();
//!
//!   const linter = new TextLinter();
//!
//!   // Load a rule from WASM bytes
//!   const ruleWasm = await fetch('/rules/no-todo.wasm')
//!     .then(r => r.arrayBuffer())
//!     .then(buf => new Uint8Array(buf));
//!
//!   linter.loadRule(ruleWasm);
//!   linter.configureRule('no-todo', JSON.stringify({ allowed: ['FIXME'] }));
//!
//!   // Lint text
//!   const diagnostics = linter.lint('# Hello\n\nTODO: Fix this', 'markdown');
//!   console.log(diagnostics);
//! }
//! ```

use wasm_bindgen::prelude::*;

use tsuzulint_ast::AstArena;
use tsuzulint_parser::{MarkdownParser, Parser, PlainTextParser};
use tsuzulint_plugin::{Diagnostic, PluginHost, Severity};

/// Initialize panic hook for better error messages in browser console.
#[wasm_bindgen(start)]
pub fn init_panic_hook() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

/// A text linter for browser environments.
///
/// This is the main entry point for using TsuzuLint in the browser.
/// It provides methods to load rules, configure them, and lint text.
#[wasm_bindgen]
pub struct TextLinter {
    host: PluginHost,
}

#[wasm_bindgen]
impl TextLinter {
    /// Creates a new TextLinter instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            host: PluginHost::new(),
        }
    }

    /// Loads a WASM rule from bytes.
    ///
    /// # Arguments
    ///
    /// * `wasm_bytes` - The WASM binary content as a Uint8Array
    ///
    /// # Returns
    ///
    /// The rule name on success, or throws an error on failure.
    #[wasm_bindgen(js_name = loadRule)]
    pub fn load_rule(&mut self, wasm_bytes: &[u8]) -> Result<String, JsError> {
        let manifest = self
            .host
            .load_rule_bytes(wasm_bytes)
            .map_err(|e| JsError::new(&e.to_string()))?;

        Ok(manifest.name)
    }

    /// Configures a loaded rule.
    ///
    /// # Arguments
    ///
    /// * `name` - The rule name
    /// * `config_json` - The configuration as a JSON string
    #[wasm_bindgen(js_name = configureRule)]
    pub fn configure_rule(&mut self, name: &str, config_json: &str) -> Result<(), JsError> {
        let config: serde_json::Value =
            serde_json::from_str(config_json).map_err(|e| JsError::new(&e.to_string()))?;

        self.host
            .configure_rule(name, config)
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Returns the names of all loaded rules.
    #[wasm_bindgen(js_name = loadedRules)]
    pub fn loaded_rules(&self) -> Vec<String> {
        self.host.loaded_rules().map(|s| s.to_string()).collect()
    }

    /// Unloads a rule.
    ///
    /// # Arguments
    ///
    /// * `name` - The rule name to unload
    ///
    /// # Returns
    ///
    /// `true` if the rule was unloaded, `false` if it wasn't loaded.
    #[wasm_bindgen(js_name = unloadRule)]
    pub fn unload_rule(&mut self, name: &str) -> bool {
        self.host.unload_rule(name)
    }

    /// Lints text content.
    ///
    /// # Arguments
    ///
    /// * `content` - The text content to lint
    /// * `file_type` - The file type (e.g., "markdown", "txt")
    ///
    /// # Returns
    ///
    /// An array of diagnostic objects.
    pub fn lint(&mut self, content: &str, file_type: &str) -> Result<JsValue, JsError> {
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

        // Convert AST to JSON for plugin consumption
        let ast_json = serde_json::to_string(&ast).map_err(|e| JsError::new(&e.to_string()))?;
        let ast_raw = serde_json::value::RawValue::from_string(ast_json)
            .map_err(|e| JsError::new(&e.to_string()))?;

        // Pre-serialize source
        let source_json = serde_json::to_string(&content).map_err(|e| JsError::new(&e.to_string()))?;
        let source_raw = serde_json::value::RawValue::from_string(source_json)
            .map_err(|e| JsError::new(&e.to_string()))?;

        // Run all rules
        let diagnostics = self
            .host
            .run_all_rules(&ast_raw, &source_raw, None)
            .map_err(|e| JsError::new(&e.to_string()))?;

        // Convert diagnostics to JavaScript objects
        let js_diagnostics: Vec<JsDiagnostic> =
            diagnostics.into_iter().map(JsDiagnostic::from).collect();

        serde_wasm_bindgen::to_value(&js_diagnostics).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Lints text content and returns diagnostics as a JSON string.
    ///
    /// This is an alternative to `lint()` that returns a JSON string
    /// instead of JavaScript objects, which may be more efficient for
    /// some use cases.
    #[wasm_bindgen(js_name = lintJson)]
    pub fn lint_json(&mut self, content: &str, file_type: &str) -> Result<String, JsError> {
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

        // Convert AST to JSON for plugin consumption
        let ast_json = serde_json::to_string(&ast).map_err(|e| JsError::new(&e.to_string()))?;
        let ast_raw = serde_json::value::RawValue::from_string(ast_json)
            .map_err(|e| JsError::new(&e.to_string()))?;

        // Pre-serialize source
        let source_json = serde_json::to_string(&content).map_err(|e| JsError::new(&e.to_string()))?;
        let source_raw = serde_json::value::RawValue::from_string(source_json)
            .map_err(|e| JsError::new(&e.to_string()))?;

        // Run all rules
        let diagnostics = self
            .host
            .run_all_rules(&ast_raw, &source_raw, None)
            .map_err(|e| JsError::new(&e.to_string()))?;

        serde_json::to_string(&diagnostics).map_err(|e| JsError::new(&e.to_string()))
    }
}

impl Default for TextLinter {
    fn default() -> Self {
        Self::new()
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
        let linter = TextLinter::new();
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

        let json = serde_json::to_value(&doc).unwrap();

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

        let json = serde_json::to_value(&parent).unwrap();

        assert!(json["children"].is_array());
        let children_arr = json["children"].as_array().unwrap();
        assert_eq!(children_arr.len(), 2);
    }

    #[test]
    fn test_ast_to_json_with_node_data() {
        use tsuzulint_ast::{AstArena, NodeType, Span, TxtNode};

        let arena = AstArena::new();
        let mut node = TxtNode::new_parent(NodeType::Header, Span::new(0, 10), &[]);
        node.data.depth = Some(2);
        node.data.url = Some(arena.alloc_str("https://example.com"));
        node.data.title = Some(arena.alloc_str("Example"));
        node.data.lang = Some(arena.alloc_str("rust"));
        node.data.ordered = Some(true);

        let json = serde_json::to_value(&node).unwrap();

        assert_eq!(json["depth"], 2);
        assert_eq!(json["url"], "https://example.com");
        assert_eq!(json["title"], "Example");
        assert_eq!(json["lang"], "rust");
        assert_eq!(json["ordered"], true);
    }
}
