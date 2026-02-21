//! Document symbol extraction for outline view.

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tracing::{debug, error};

use tsuzulint_ast::{AstArena, NodeType, TxtNode};
use tsuzulint_parser::{MarkdownParser, Parser, PlainTextParser};

use crate::conversion::offset_to_range;
use crate::state::SharedState;

/// Handles the `textDocument/documentSymbol` request.
pub async fn handle_document_symbol(
    state: &SharedState,
    params: DocumentSymbolParams,
) -> Result<Option<DocumentSymbolResponse>> {
    debug!("Document symbol request: {}", params.text_document.uri);

    let uri = &params.text_document.uri;
    let text = match get_document_content(state, uri) {
        Some(t) => t,
        None => return Ok(None),
    };

    let path = match uri.to_file_path() {
        Ok(p) => p,
        Err(_) => std::path::PathBuf::from("untitled"),
    };

    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let parser: Box<dyn Parser> = if extension == "md" || extension == "markdown" {
        Box::new(MarkdownParser::new())
    } else {
        Box::new(PlainTextParser::new())
    };

    let arena = AstArena::new();
    let ast = match parser.parse(&arena, &text) {
        Ok(ast) => ast,
        Err(e) => {
            error!("Failed to parse document for symbols: {}", e);
            return Ok(None);
        }
    };

    let symbols = SymbolExtractor::new(&text).extract(&ast);
    Ok(Some(DocumentSymbolResponse::Nested(symbols)))
}

/// Symbol extractor utility.
pub struct SymbolExtractor<'a> {
    text: &'a str,
}

impl<'a> SymbolExtractor<'a> {
    /// Creates a new symbol extractor.
    pub fn new(text: &'a str) -> Self {
        Self { text }
    }

    /// Extracts document symbols from the AST.
    pub fn extract(&self, node: &TxtNode) -> Vec<DocumentSymbol> {
        let mut symbols = Vec::new();

        for child in node.children.iter() {
            let symbol_kind = match child.node_type {
                NodeType::Header => SymbolKind::STRING,
                NodeType::CodeBlock => SymbolKind::FUNCTION,
                _ => continue,
            };

            let mut detail = String::new();
            if child.node_type == NodeType::Header {
                self.collect_text(child, &mut detail);
            } else if child.node_type == NodeType::CodeBlock {
                detail = "Code Block".to_string();
            }

            if let Some(range) = offset_to_range(
                child.span.start as usize,
                child.span.end as usize,
                self.text,
            ) {
                let selection_range = range;

                #[allow(deprecated)]
                let symbol = DocumentSymbol {
                    name: if detail.is_empty() {
                        format!("{}", child.node_type)
                    } else {
                        detail
                    },
                    detail: None,
                    kind: symbol_kind,
                    tags: None,
                    deprecated: None,
                    range,
                    selection_range,
                    children: None,
                };

                symbols.push(symbol);
            }
        }

        symbols
    }

    /// Recursively collects text from Str nodes.
    fn collect_text(&self, node: &TxtNode, out: &mut String) {
        if node.node_type == NodeType::Str {
            let start = node.span.start as usize;
            let end = node.span.end as usize;
            if start <= end && end <= self.text.len() {
                out.push_str(&self.text[start..end]);
            }
        }
        for child in node.children.iter() {
            self.collect_text(child, out);
        }
    }
}

fn get_document_content(state: &SharedState, uri: &Url) -> Option<String> {
    let docs = state.documents.read().ok()?;
    docs.get(uri).map(|d| d.text.clone())
}
