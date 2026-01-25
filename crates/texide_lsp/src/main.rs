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

use texide_core::{
    Diagnostic as TexideDiagnostic, Linter, LinterConfig, Severity as TexideSeverity,
};

/// The LSP backend for Texide.
struct Backend {
    /// LSP client for sending notifications.
    client: Client,
    /// Document contents cache.
    documents: RwLock<HashMap<Url, String>>,
    /// Linter configuration.
    /// Linter instance.
    linter: RwLock<Linter>,
}

impl Backend {
    /// Creates a new backend with the given client.
    fn new(client: Client) -> Self {
        // Initialize linter with default config
        // Initialize linter with default config
        // Real config will be loaded during `initialize` if available
        let config = LinterConfig::new();
        let linter = Linter::new(config).expect("Failed to initialize linter");

        Self {
            client,
            documents: RwLock::new(HashMap::new()),
            linter: RwLock::new(linter),
        }
    }

    /// Validates a document and publishes diagnostics.
    async fn validate_document(&self, uri: &Url, text: &str, version: Option<i32>) {
        debug!("Validating document: {}", uri);

        let path = match uri.to_file_path() {
            Ok(p) => p,
            Err(_) => {
                debug!("Skipping validation for non-file URI: {}", uri);
                return;
            }
        };

        let diagnostics = self.lint_text(text, &path);

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
    fn lint_text(&self, text: &str, path: &std::path::Path) -> Vec<TexideDiagnostic> {
        // Safely acquire read lock, handling potential poisoning
        let linter_guard = match self.linter.read() {
            Ok(guard) => guard,
            Err(poisoned) => {
                error!("Linter lock poisoned: {}", poisoned);
                return vec![];
            }
        };

        match linter_guard.lint_content(text, path) {
            Ok(diagnostics) => diagnostics,
            Err(e) => {
                error!("Lint error: {}", e);
                vec![]
            }
        }
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

    /// Helper to compare Positions (p1 <= p2)
    fn positions_le(&self, p1: Position, p2: Position) -> bool {
        p1.line < p2.line || (p1.line == p2.line && p1.character <= p2.character)
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        info!("Texide LSP server initializing...");

        if let Some(path) = params.root_uri.and_then(|u| u.to_file_path().ok()) {
            let config_files = [".texide.json", ".texiderc", "texide.config.json"];
            for name in config_files {
                let config_path = path.join(name);
                if config_path.exists() {
                    info!("Found config file: {}", config_path.display());
                    match LinterConfig::from_file(&config_path) {
                        Ok(config) => {
                            info!("Loaded configuration from workspace");
                            let mut linter = self.linter.write().unwrap();
                            *linter = Linter::new(config).expect("Failed to re-initialize linter");
                        }
                        Err(e) => {
                            error!("Failed to load config: {}", e);
                        }
                    }
                    break;
                }
            }
        }

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
                // Code action support for auto-fix
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
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

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        debug!("Code action request: {}", params.text_document.uri);

        // Get document content
        let uri = &params.text_document.uri;
        let text = {
            let docs = match self.documents.read() {
                Ok(guard) => guard,
                Err(e) => {
                    error!("Documents lock poisoned: {}", e);
                    return Ok(None);
                }
            };
            match docs.get(uri) {
                Some(text) => text.clone(),
                None => return Ok(None),
            }
        };

        let path = match uri.to_file_path() {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };

        // Re-run linting to get diagnostics with fixes
        // Note: In a real implementation, we should cache diagnostics map to avoid re-linting
        let diagnostics = self.lint_text(&text, &path);

        let mut actions = Vec::new();

        for diag in diagnostics {
            if let Some(fix) = diag.fix {
                // Check if the diagnostic range intersects with the requested range
                // For simplicity, we check if the fix applies to the diagnostic in the requested range
                // A more robust check would involve comparing LSP ranges

                let fix_range =
                    self.offset_to_range(fix.span.start as usize, fix.span.end as usize, &text);

                if let Some(range) = fix_range {
                    // Check intersection with params.range using proper Position comparison
                    // Two ranges intersect if (start1 <= end2) && (start2 <= end1)
                    if self.positions_le(range.start, params.range.end)
                        && self.positions_le(params.range.start, range.end)
                    {
                        let action = CodeAction {
                            title: format!("Fix: {}", diag.message),
                            kind: Some(CodeActionKind::QUICKFIX),
                            diagnostics: None, // We could link back to the LSP diagnostic here
                            edit: Some(WorkspaceEdit {
                                changes: Some(HashMap::from([(
                                    uri.clone(),
                                    vec![TextEdit {
                                        range,
                                        new_text: fix.text,
                                    }],
                                )])),
                                ..Default::default()
                            }),
                            ..Default::default()
                        };
                        actions.push(CodeActionOrCommand::CodeAction(action));
                    }
                }
            }
        }

        Ok(Some(actions))
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
