//! Texide LSP Server
//!
//! Language Server Protocol implementation for Texide.
//! Provides real-time linting in editors.

use std::collections::HashMap;
use std::sync::RwLock;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};
use tracing::{debug, error, info};
use tracing_subscriber::EnvFilter;

use texide_ast::AstArena;
use texide_core::{Diagnostic as TexideDiagnostic, LinterConfig, Severity as TexideSeverity};
use texide_parser::{MarkdownParser, Parser, PlainTextParser};

/// The LSP backend for Texide.
struct Backend {
    /// LSP client for sending notifications.
    client: Client,
    /// Document contents cache.
    documents: RwLock<HashMap<Url, String>>,
    /// Linter configuration.
    config: RwLock<LinterConfig>,
}

impl Backend {
    /// Creates a new backend with the given client.
    fn new(client: Client) -> Self {
        Self {
            client,
            documents: RwLock::new(HashMap::new()),
            config: RwLock::new(LinterConfig::new()),
        }
    }

    /// Validates a document and publishes diagnostics.
    async fn validate_document(&self, uri: &Url, text: &str, version: Option<i32>) {
        debug!("Validating document: {}", uri);

        let diagnostics = self.lint_text(text, uri.path());

        // Convert to LSP diagnostics
        let lsp_diagnostics: Vec<Diagnostic> = diagnostics
            .into_iter()
            .filter_map(|d| self.to_lsp_diagnostic(&d, text))
            .collect();

        self.client
            .publish_diagnostics(uri.clone(), lsp_diagnostics, version)
            .await;
    }

    /// Lints text and returns Texide diagnostics.
    fn lint_text(&self, text: &str, file_path: &str) -> Vec<TexideDiagnostic> {
        // Determine parser based on file extension
        let extension = file_path.rsplit('.').next().unwrap_or("");

        let parser: Box<dyn Parser> = match extension {
            "md" | "markdown" => Box::new(MarkdownParser::new()),
            _ => Box::new(PlainTextParser::new()),
        };

        // Parse the document
        let arena = AstArena::new();
        let ast = match parser.parse(&arena, text) {
            Ok(ast) => ast,
            Err(e) => {
                error!("Parse error: {}", e);
                return vec![];
            }
        };

        // For now, return empty diagnostics
        // TODO: Integrate with plugin system to run WASM rules
        let _ = ast;
        vec![]
    }

    /// Converts a Texide diagnostic to an LSP diagnostic.
    fn to_lsp_diagnostic(&self, diag: &TexideDiagnostic, text: &str) -> Option<Diagnostic> {
        let range = self.offset_to_range(diag.span.start as usize, diag.span.end as usize, text)?;

        let severity = match diag.severity {
            TexideSeverity::Error => DiagnosticSeverity::ERROR,
            TexideSeverity::Warning => DiagnosticSeverity::WARNING,
            TexideSeverity::Info => DiagnosticSeverity::INFORMATION,
        };

        Some(Diagnostic {
            range,
            severity: Some(severity),
            code: Some(NumberOrString::String(diag.rule_id.clone())),
            source: Some("texide".to_string()),
            message: diag.message.clone(),
            ..Default::default()
        })
    }

    /// Converts byte offsets to an LSP range.
    fn offset_to_range(&self, start: usize, end: usize, text: &str) -> Option<Range> {
        let start_pos = self.offset_to_position(start, text)?;
        let end_pos = self.offset_to_position(end, text)?;
        Some(Range::new(start_pos, end_pos))
    }

    /// Converts a byte offset to an LSP position.
    fn offset_to_position(&self, offset: usize, text: &str) -> Option<Position> {
        if offset > text.len() {
            return None;
        }

        let mut line = 0u32;
        let mut col = 0u32;
        let mut current_offset = 0;

        for ch in text.chars() {
            if current_offset >= offset {
                break;
            }

            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }

            current_offset += ch.len_utf8();
        }

        Some(Position::new(line, col))
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        info!("Texide LSP server initializing...");

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                            include_text: Some(true),
                        })),
                        ..Default::default()
                    },
                )),
                // TODO: Add code action support for auto-fix
                // code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "texide-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Texide LSP server initialized!")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        info!("Texide LSP server shutting down...");
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        debug!("Document opened: {}", params.text_document.uri);

        // Store document content
        {
            let mut docs = self.documents.write().unwrap();
            docs.insert(
                params.text_document.uri.clone(),
                params.text_document.text.clone(),
            );
        }

        // Validate on open
        self.validate_document(
            &params.text_document.uri,
            &params.text_document.text,
            Some(params.text_document.version),
        )
        .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        debug!("Document changed: {}", params.text_document.uri);

        // Get the full text (we use FULL sync)
        if let Some(change) = params.content_changes.into_iter().next() {
            // Update stored content
            {
                let mut docs = self.documents.write().unwrap();
                docs.insert(params.text_document.uri.clone(), change.text.clone());
            }

            // Validate on change
            self.validate_document(
                &params.text_document.uri,
                &change.text,
                Some(params.text_document.version),
            )
            .await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        debug!("Document saved: {}", params.text_document.uri);

        if let Some(text) = params.text {
            self.validate_document(&params.text_document.uri, &text, None)
                .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        debug!("Document closed: {}", params.text_document.uri);

        // Remove from cache
        {
            let mut docs = self.documents.write().unwrap();
            docs.remove(&params.text_document.uri);
        }

        // Clear diagnostics
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }
}

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("texide_lsp=debug".parse().unwrap()),
        )
        .with_writer(std::io::stderr)
        .init();

    info!("Texide LSP server starting...");

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
