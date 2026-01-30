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

use texide_ast::{AstArena, NodeType, TxtNode};
use texide_core::{
    Diagnostic as TexideDiagnostic, Linter, LinterConfig, Severity as TexideSeverity,
};
use texide_parser::{MarkdownParser, Parser, PlainTextParser};

/// The LSP backend for Texide.
struct Backend {
    /// LSP client for sending notifications.
    client: Client,
    /// Document contents cache.
    documents: RwLock<HashMap<Url, String>>,
    /// Linter instance (may be None if initialization failed).
    linter: RwLock<Option<Linter>>,
    /// Workspace root path.
    workspace_root: RwLock<Option<std::path::PathBuf>>,
}

impl Backend {
    /// Creates a new backend with the given client.
    fn new(client: Client) -> Self {
        // Initialize linter with default config
        // Real config will be loaded during `initialize` if available
        let config = LinterConfig::new();
        let linter = match Linter::new(config) {
            Ok(l) => Some(l),
            Err(e) => {
                // Log error and set linter to None
                // The LSP will continue to work but without linting capabilities
                error!(
                    "Failed to initialize linter: {}. LSP will run without linting.",
                    e
                );
                None
            }
        };

        Self {
            client,
            documents: RwLock::new(HashMap::new()),
            linter: RwLock::new(linter),
            workspace_root: RwLock::new(None),
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

        // Check if linter is available
        let linter = match linter_guard.as_ref() {
            Some(l) => l,
            None => {
                debug!("Linter not available, skipping linting");
                return vec![];
            }
        };

        match linter.lint_content(text, path) {
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

    /// Extracts document symbols from AST.
    fn extract_symbols(&self, node: &TxtNode, text: &str) -> Vec<DocumentSymbol> {
        let mut symbols = Vec::new();

        // We only care about specific block elements for the outline
        for child in node.children.iter() {
            let symbol_kind = match child.node_type {
                // H1 -> File/Class, H2 -> Method/Module. Using String for now.
                NodeType::Header => SymbolKind::STRING,
                NodeType::CodeBlock => SymbolKind::FUNCTION,
                _ => continue,
            };

            // Extract details (e.g., header text)
            let mut detail = String::new();
            if child.node_type == NodeType::Header {
                // Collect text children
                self.collect_text(child, &mut detail, text);
            } else if child.node_type == NodeType::CodeBlock {
                detail = "Code Block".to_string();
            }

            // Convert range
            if let Some(range) =
                self.offset_to_range(child.span.start as usize, child.span.end as usize, text)
            {
                // For selection range, ideally we want just the header text, but full range is fine for now
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
                    children: None, // Flat list for now
                };

                symbols.push(symbol);
            }
        }

        symbols
    }

    fn collect_text(&self, node: &TxtNode, out: &mut String, source: &str) {
        if node.node_type == NodeType::Str {
            let start = node.span.start as usize;
            let end = node.span.end as usize;
            if start <= end && end <= source.len() {
                out.push_str(&source[start..end]);
            }
        }
        for child in node.children.iter() {
            self.collect_text(child, out, source);
        }
    }

    /// Reloads configuration from the workspace root.
    fn reload_config(&self) {
        let root_guard = match self.workspace_root.read() {
            Ok(g) => g,
            Err(e) => {
                error!("Workspace root lock poisoned: {}", e);
                return;
            }
        };

        let path = match root_guard.as_ref() {
            Some(p) => p,
            None => {
                // No root path, cannot load config
                return;
            }
        };

        let config_files = [".texide.jsonc", ".texide.json"];
        for name in config_files {
            let config_path = path.join(name);
            if config_path.exists() {
                info!("Found config file: {}", config_path.display());
                match LinterConfig::from_file(&config_path) {
                    Ok(config) => {
                        info!("Loaded configuration from workspace");
                        match self.linter.write() {
                            Ok(mut linter_guard) => match Linter::new(config) {
                                Ok(new_linter) => {
                                    *linter_guard = Some(new_linter);
                                    info!("Linter re-initialized with new config");
                                }
                                Err(e) => {
                                    error!("Failed to create new linter: {}", e);
                                    *linter_guard = None;
                                }
                            },
                            Err(e) => error!("Linter lock poisoned: {}", e),
                        }
                    }
                    Err(e) => {
                        error!("Failed to load config: {}", e);
                    }
                }
                break;
            }
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        info!("Texide LSP server initializing...");

        if let Some(path) = params.root_uri.and_then(|u| u.to_file_path().ok()) {
            // Store workspace root
            {
                match self.workspace_root.write() {
                    Ok(mut root) => {
                        *root = Some(path);
                    }
                    Err(e) => {
                        error!("Workspace root lock poisoned: {}", e);
                        return Ok(InitializeResult::default());
                    }
                }
            }

            // Initial config load
            self.reload_config();
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
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        code_action_kinds: Some(vec![
                            CodeActionKind::QUICKFIX,
                            CodeActionKind::SOURCE_FIX_ALL,
                        ]),
                        resolve_provider: Some(false),
                        work_done_progress_options: Default::default(),
                    },
                )),
                // Document symbol support
                document_symbol_provider: Some(OneOf::Left(true)),
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
            let mut docs = match self.documents.write() {
                Ok(guard) => guard,
                Err(e) => {
                    error!("Documents lock poisoned: {}", e);
                    return;
                }
            };
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
                let mut docs = match self.documents.write() {
                    Ok(guard) => guard,
                    Err(e) => {
                        error!("Documents lock poisoned: {}", e);
                        return;
                    }
                };
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

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        debug!("Watched files changed: {:?}", params.changes);

        // Check if any config files changed
        let config_changed = params.changes.iter().any(|change| {
            let path = change.uri.path();
            path.ends_with(".texide.json") || path.ends_with(".texide.jsonc")
        });

        if config_changed {
            info!("Configuration file changed, reloading...");
            self.reload_config();
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        debug!("Document closed: {}", params.text_document.uri);

        // Remove from cache
        {
            let mut docs = match self.documents.write() {
                Ok(guard) => guard,
                Err(e) => {
                    error!("Documents lock poisoned: {}", e);
                    return;
                }
            };
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

        // Calculate which action kinds are requested
        // If `only` is None, all action kinds are allowed (per LSP spec)
        let (wants_fix_all, wants_quickfix) = match &params.context.only {
            Some(only) => (
                only.contains(&CodeActionKind::SOURCE_FIX_ALL),
                only.contains(&CodeActionKind::QUICKFIX),
            ),
            None => (true, true),
        };

        // Handle SourceFixAll
        if wants_fix_all {
            let mut changes = HashMap::new();
            let mut edits = Vec::new();

            // Apply all available fixes
            // We need to be careful about overlapping ranges.
            // Simple approach: Sort by position (descending) and apply.
            // Ideally, we should use texide_core::fixer, but here we construct TextEdits.

            let mut fixable_diags: Vec<_> =
                diagnostics.iter().filter(|d| d.fix.is_some()).collect();

            // Sort descending by start position to avoid offset shifting issues if applied sequentially
            // But LSP TextEdits are applied simultaneously by the client, so standard order often doesn't matter
            // IF ranges don't overlap. If they overlap, it's a conflict.
            // We assume rule-generated fixes don't usually overlap for different rules, OR we take one.

            fixable_diags.sort_by(|a, b| b.span.start.cmp(&a.span.start));

            for diag in fixable_diags {
                if let Some(ref fix) = diag.fix
                    && let Some(range) =
                        self.offset_to_range(fix.span.start as usize, fix.span.end as usize, &text)
                {
                    edits.push(TextEdit {
                        range,
                        new_text: fix.text.clone(),
                    });
                }
            }

            if !edits.is_empty() {
                changes.insert(uri.clone(), edits);

                let action = CodeAction {
                    title: "Fix all Texide issues".to_string(),
                    kind: Some(CodeActionKind::SOURCE_FIX_ALL),
                    edit: Some(WorkspaceEdit {
                        changes: Some(changes),
                        ..Default::default()
                    }),
                    ..Default::default()
                };
                actions.push(CodeActionOrCommand::CodeAction(action));
            }
        }

        // Generate QUICKFIX actions for diagnostics in the requested range
        if !wants_quickfix {
            return Ok(Some(actions));
        }

        for diag in &diagnostics {
            if let Some(ref fix) = diag.fix
                && let Some(range) =
                    self.offset_to_range(fix.span.start as usize, fix.span.end as usize, &text)
                && self.positions_le(range.start, params.range.end)
                && self.positions_le(params.range.start, range.end)
            {
                let action = CodeAction {
                    title: format!("Fix: {}", diag.message),
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: None,
                    edit: Some(WorkspaceEdit {
                        changes: Some(HashMap::from([(
                            uri.clone(),
                            vec![TextEdit {
                                range,
                                new_text: fix.text.clone(),
                            }],
                        )])),
                        ..Default::default()
                    }),
                    ..Default::default()
                };
                actions.push(CodeActionOrCommand::CodeAction(action));
            }
        }

        Ok(Some(actions))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        debug!("Document symbol request: {}", params.text_document.uri);

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

        // We need to parse the document to get symbols
        let path = match uri.to_file_path() {
            Ok(p) => p,
            Err(_) => std::path::PathBuf::from("untitled"),
        };

        // Select parser
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let parser: Box<dyn Parser> = if extension == "md" || extension == "markdown" {
            Box::new(MarkdownParser::new())
        } else {
            // For plain text, maybe no symbols? Or just return None
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

        let symbols = self.extract_symbols(&ast, &text);
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }
}

/// Starts the LSP server.
///
/// This function does not return unless an error occurs or the server shuts down.
pub async fn run() {
    info!("Texide LSP server starting...");

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
