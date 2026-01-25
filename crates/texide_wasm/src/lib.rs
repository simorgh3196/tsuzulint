//! # texide_wasm
//!
//! WebAssembly bindings for Texide, enabling browser-based text linting.
//!
//! This crate provides JavaScript/TypeScript bindings for the Texide linter
//! using wasm-bindgen. It enables running the complete linter in the browser
//! with support for dynamically loading WASM rules.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │ Browser (JavaScript)                    │
//! │   ↓                                     │
//! │ texide_wasm (this crate)                │
//! │   ↓                                     │
//! │ texide_plugin (browser feature)         │
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
//! import init, { TextLinter } from 'texide-wasm';
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

use texide_ast::AstArena;
use texide_parser::{MarkdownParser, Parser, PlainTextParser};
use texide_plugin::{Diagnostic, PluginHost, Severity};

/// Initialize panic hook for better error messages in browser console.
#[wasm_bindgen(start)]
pub fn init_panic_hook() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

/// A text linter for browser environments.
///
/// This is the main entry point for using Texide in the browser.
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
        self.host
            .loaded_rules()
            .iter()
            .map(|s| s.to_string())
            .collect()
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
        let ast_json = ast_to_json(&ast);

        // Run all rules
        let diagnostics = self
            .host
            .run_all_rules(&ast_json, content, None)
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
        let ast_json = ast_to_json(&ast);

        // Run all rules
        let diagnostics = self
            .host
            .run_all_rules(&ast_json, content, None)
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

/// Converts a TxtNode to a JSON value.
fn ast_to_json(node: &texide_ast::TxtNode) -> serde_json::Value {
    let mut obj = serde_json::Map::new();

    obj.insert(
        "type".to_string(),
        serde_json::Value::String(format!("{:?}", node.node_type)),
    );

    obj.insert(
        "range".to_string(),
        serde_json::json!([node.span.start, node.span.end]),
    );

    if let Some(value) = node.value {
        obj.insert(
            "value".to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }

    if !node.children.is_empty() {
        let children: Vec<serde_json::Value> = node.children.iter().map(ast_to_json).collect();
        obj.insert("children".to_string(), serde_json::Value::Array(children));
    }

    // Add node data if present
    if let Some(url) = node.data.url {
        obj.insert(
            "url".to_string(),
            serde_json::Value::String(url.to_string()),
        );
    }
    if let Some(title) = node.data.title {
        obj.insert(
            "title".to_string(),
            serde_json::Value::String(title.to_string()),
        );
    }
    if let Some(depth) = node.data.depth {
        obj.insert("depth".to_string(), serde_json::Value::Number(depth.into()));
    }
    if let Some(ordered) = node.data.ordered {
        obj.insert("ordered".to_string(), serde_json::Value::Bool(ordered));
    }
    if let Some(lang) = node.data.lang {
        obj.insert(
            "lang".to_string(),
            serde_json::Value::String(lang.to_string()),
        );
    }

    serde_json::Value::Object(obj)
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
}
