//! TsuzuLint LSP Server
//!
//! Language Server Protocol implementation for TsuzuLint.
//! Provides real-time linting in editors.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(test)]
static TEST_LINT_DELAY_MS: AtomicU64 = AtomicU64::new(0);

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};
use tracing::{debug, error, info};

use tsuzulint_ast::{AstArena, NodeType, TxtNode};
use tsuzulint_core::{
    Diagnostic as TsuzuLintDiagnostic, Linter, LinterConfig, Severity as TsuzuLintSeverity,
};
use tsuzulint_parser::{MarkdownParser, Parser, PlainTextParser};

struct DocumentData {
    text: String,
    version: i32,
}

pub(crate) struct BackendState {
    /// Document contents cache.
    documents: RwLock<HashMap<Url, DocumentData>>,
    /// Linter instance (may be None if initialization failed).
    linter: RwLock<Option<Linter>>,
    /// Workspace root path.
    workspace_root: RwLock<Option<std::path::PathBuf>>,
}

/// The LSP backend for TsuzuLint.
#[derive(Clone)]
pub struct Backend {
    /// LSP client for sending notifications.
    client: Client,
    /// Shared state
    state: Arc<BackendState>,
}

impl Backend {
    /// Creates a new backend with the given client.
    pub fn new(client: Client) -> Self {
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
            state: Arc::new(BackendState {
                documents: RwLock::new(HashMap::new()),
                linter: RwLock::new(linter),
                workspace_root: RwLock::new(None),
            }),
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

        let diagnostics = self.lint_text(text, &path).await;

        // Convert to LSP diagnostics
        let lsp_diagnostics: Vec<Diagnostic> = diagnostics
            .into_iter()
            .filter_map(|d| self.to_lsp_diagnostic(&d, text))
            .collect();

        self.client
            .publish_diagnostics(uri.clone(), lsp_diagnostics, version)
            .await;
    }

    /// Lints text and returns TsuzuLint diagnostics.
    ///
    /// This method offloads the blocking lint operation to `spawn_blocking`
    /// to avoid blocking the async runtime.
    async fn lint_text(&self, text: &str, path: &std::path::Path) -> Vec<TsuzuLintDiagnostic> {
        let state = self.state.clone();
        let text = text.to_string();
        let path = path.to_path_buf();

        tokio::task::spawn_blocking(move || {
            // Simulate load for testing (injected delay via static)
            #[cfg(test)]
            {
                let delay_ms = TEST_LINT_DELAY_MS.load(Ordering::Relaxed);
                if delay_ms > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                }
            }

            // Safely acquire read lock, handling potential poisoning
            let linter_guard = match state.linter.read() {
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

            match linter.lint_content(&text, &path) {
                Ok(diagnostics) => diagnostics,
                Err(e) => {
                    error!("Lint error: {}", e);
                    vec![]
                }
            }
        })
        .await
        .unwrap_or_default()
    }

    /// Converts a TsuzuLint diagnostic to an LSP diagnostic.
    fn to_lsp_diagnostic(&self, diag: &TsuzuLintDiagnostic, text: &str) -> Option<Diagnostic> {
        let range = self.offset_to_range(diag.span.start as usize, diag.span.end as usize, text)?;

        let severity = match diag.severity {
            TsuzuLintSeverity::Error => DiagnosticSeverity::ERROR,
            TsuzuLintSeverity::Warning => DiagnosticSeverity::WARNING,
            TsuzuLintSeverity::Info => DiagnosticSeverity::INFORMATION,
        };

        Some(Diagnostic {
            range,
            severity: Some(severity),
            code: Some(NumberOrString::String(diag.rule_id.clone())),
            source: Some("tsuzulint".to_string()),
            message: diag.message.clone(),
            ..Default::default()
        })
    }

    /// Converts byte offsets to an LSP range.
    fn offset_to_range(&self, start: usize, end: usize, text: &str) -> Option<Range> {
        let start_pos = Self::offset_to_position(start, text)?;
        let end_pos = Self::offset_to_position(end, text)?;
        Some(Range::new(start_pos, end_pos))
    }

    /// Converts a byte offset to an LSP position.
    fn offset_to_position(offset: usize, text: &str) -> Option<Position> {
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
                col += ch.len_utf16() as u32;
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
        let root_guard = match self.state.workspace_root.read() {
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

        if let Some(config_path) = LinterConfig::discover(path) {
            info!("Found config file: {}", config_path.display());
            match LinterConfig::from_file(&config_path) {
                Ok(config) => {
                    info!("Loaded configuration from workspace");
                    match self.state.linter.write() {
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
        }
    }

    /// Sets a simulated delay for lint_text (test only, global state).
    #[cfg(test)]
    pub fn set_global_test_delay(delay: std::time::Duration) {
        TEST_LINT_DELAY_MS.store(delay.as_millis() as u64, Ordering::Relaxed);
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        info!("TsuzuLint LSP server initializing...");

        if let Some(path) = params.root_uri.and_then(|u| u.to_file_path().ok()) {
            // Store workspace root
            {
                match self.state.workspace_root.write() {
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
                name: "tsuzulint-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "TsuzuLint LSP server initialized!")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        info!("TsuzuLint LSP server shutting down...");
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        debug!("Document opened: {}", params.text_document.uri);

        // Store document content
        {
            let mut docs = match self.state.documents.write() {
                Ok(guard) => guard,
                Err(e) => {
                    error!("Documents lock poisoned: {}", e);
                    return;
                }
            };
            docs.insert(
                params.text_document.uri.clone(),
                DocumentData {
                    text: params.text_document.text.clone(),
                    version: params.text_document.version,
                },
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
            let uri = params.text_document.uri.clone();
            let version = params.text_document.version;
            let text = change.text;

            // Update stored content
            {
                let mut docs = match self.state.documents.write() {
                    Ok(guard) => guard,
                    Err(e) => {
                        error!("Documents lock poisoned: {}", e);
                        return;
                    }
                };
                docs.insert(
                    uri.clone(),
                    DocumentData {
                        text: text.clone(),
                        version,
                    },
                );
            }

            // Debounce validation
            // Clone self to move into the async block (Backend catches Arc<BackendState> internally)
            let backend = self.clone();

            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;

                // Check version
                let should_validate = {
                    let docs = match backend.state.documents.read() {
                        Ok(g) => g,
                        Err(e) => {
                            error!("Documents lock poisoned: {}", e);
                            return;
                        }
                    };
                    if let Some(doc) = docs.get(&uri) {
                        doc.version == version
                    } else {
                        false
                    }
                };

                if should_validate {
                    backend.validate_document(&uri, &text, Some(version)).await;
                }
            });
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
            LinterConfig::CONFIG_FILES
                .iter()
                .any(|name| path.ends_with(name))
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
            let mut docs = match self.state.documents.write() {
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
            let docs = match self.state.documents.read() {
                Ok(guard) => guard,
                Err(e) => {
                    error!("Documents lock poisoned: {}", e);
                    return Ok(None);
                }
            };
            match docs.get(uri) {
                Some(data) => data.text.clone(),
                None => return Ok(None),
            }
        };

        let path = match uri.to_file_path() {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };

        // Re-run linting to get diagnostics with fixes
        // Note: In a real implementation, we should cache diagnostics map to avoid re-linting
        let diagnostics = self.lint_text(&text, &path).await;

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
            // Ideally, we should use tsuzulint_core::fixer, but here we construct TextEdits.

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
                    title: "Fix all TsuzuLint issues".to_string(),
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
            let docs = match self.state.documents.read() {
                Ok(guard) => guard,
                Err(e) => {
                    error!("Documents lock poisoned: {}", e);
                    return Ok(None);
                }
            };
            match docs.get(uri) {
                Some(data) => data.text.clone(),
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
    info!("TsuzuLint LSP server starting...");

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tower_lsp::lsp_types::Position;

    #[test]
    fn test_offset_to_position_basic_ascii() {
        let text = "Hello World";
        assert_eq!(
            Backend::offset_to_position(0, text),
            Some(Position::new(0, 0))
        );
        assert_eq!(
            Backend::offset_to_position(5, text),
            Some(Position::new(0, 5))
        );
        assert_eq!(
            Backend::offset_to_position(10, text),
            Some(Position::new(0, 10))
        );
        assert_eq!(
            Backend::offset_to_position(11, text),
            Some(Position::new(0, 11))
        );
        assert_eq!(Backend::offset_to_position(12, text), None);
    }

    #[test]
    fn test_offset_to_position_multiline() {
        let text = "Line 1\nLine 2\nLine 3";
        assert_eq!(
            Backend::offset_to_position(7, text),
            Some(Position::new(1, 0))
        );
        assert_eq!(
            Backend::offset_to_position(20, text),
            Some(Position::new(2, 6))
        );
    }

    #[test]
    fn test_offset_to_position_unicode_multibyte() {
        // 'あ' is 3 bytes in UTF-8, 1 code unit in UTF-16
        let text = "あいう";
        assert_eq!(
            Backend::offset_to_position(0, text),
            Some(Position::new(0, 0))
        );
        assert_eq!(
            Backend::offset_to_position(3, text),
            Some(Position::new(0, 1))
        );
        assert_eq!(
            Backend::offset_to_position(6, text),
            Some(Position::new(0, 2))
        );
        assert_eq!(
            Backend::offset_to_position(9, text),
            Some(Position::new(0, 3))
        );
    }

    #[test]
    fn test_offset_to_position_supplementary_plane_chars() {
        // '🎉' is 4 bytes in UTF-8, 2 code units in UTF-16
        let text = "a🎉b";
        assert_eq!(
            Backend::offset_to_position(0, text),
            Some(Position::new(0, 0))
        ); // 'a'
        assert_eq!(
            Backend::offset_to_position(1, text),
            Some(Position::new(0, 1))
        ); // '🎉'
        assert_eq!(
            Backend::offset_to_position(5, text),
            Some(Position::new(0, 3))
        ); // 'b'
    }

    #[test]
    fn test_offset_to_position_empty_string() {
        assert_eq!(
            Backend::offset_to_position(0, ""),
            Some(Position::new(0, 0))
        );
        assert_eq!(Backend::offset_to_position(1, ""), None);
    }

    /// Tests that lint_text does not block the async runtime.
    ///
    /// Strategy:
    /// 1. Inject a 100ms delay into lint_text
    /// 2. Open 3 documents in parallel via LSP protocol
    /// 3. If blocking: total time ~300ms (sequential execution)
    /// 4. If non-blocking: total time ~100ms + overhead (parallel execution)
    #[tokio::test]
    async fn test_lint_text_does_not_block_runtime() {
        use std::time::{Duration, Instant};

        // Set test delay (100ms per lint operation)
        Backend::set_global_test_delay(Duration::from_millis(100));

        // Create LSP server pipes
        let (client_read, server_write) = tokio::io::duplex(4096);
        let (server_read, client_write) = tokio::io::duplex(4096);

        let (service, socket) = LspService::new(Backend::new);

        // Start server in background
        let _server_handle = tokio::spawn(async move {
            tower_lsp::Server::new(server_read, server_write, socket)
                .serve(service)
                .await;
        });

        let mut reader = tokio::io::BufReader::new(client_read);
        let mut writer = client_write;

        // Channel to collect responses
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(msg) = recv_msg(&mut reader).await {
                if tx.send(msg).is_err() {
                    break;
                }
            }
        });

        // Setup
        let temp_dir = tempfile::tempdir().unwrap();
        let root_path = temp_dir.path();
        let root_uri = Url::from_file_path(root_path).unwrap();

        // Initialize
        let init_req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"rootUri":"{}","capabilities":{{}}}}}}"#,
            root_uri
        );
        send_msg(&mut writer, &init_req).await;
        let _resp = rx.recv().await.unwrap();

        // Initialized
        let initialized_notif = r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#;
        send_msg(&mut writer, initialized_notif).await;

        // Measure time to open 3 documents in parallel
        let start = Instant::now();

        // Send 3 didOpen requests rapidly (they should be processed in parallel if non-blocking)
        for i in 0..3 {
            let file_path = temp_dir.path().join(format!("test{}.md", i));
            let file_uri = Url::from_file_path(file_path).unwrap();
            let did_open = format!(
                r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{}","languageId":"markdown","version":0,"text":"test content {}"}}}}}}"#,
                file_uri, i
            );
            send_msg(&mut writer, &did_open).await;
        }

        // Wait for all 3 publishDiagnostics responses
        let mut diagnostics_count = 0;
        let timeout = tokio::time::sleep(Duration::from_secs(5));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                msg_opt = rx.recv() => {
                    if let Some(msg) = msg_opt {
                        if msg.contains("publishDiagnostics") {
                            diagnostics_count += 1;
                            if diagnostics_count >= 3 {
                                break;
                            }
                        }
                    } else {
                        break;
                    }
                }
                _ = &mut timeout => break,
            }
        }

        let elapsed = start.elapsed();

        // Reset delay
        Backend::set_global_test_delay(Duration::from_millis(0));

        // Verify all diagnostics received
        assert_eq!(diagnostics_count, 3, "Expected 3 diagnostics responses");

        // Verify timing:
        // - If blocking: 3 × 100ms = 300ms minimum
        // - If non-blocking: ~100ms + overhead (should be < 250ms)
        println!("Elapsed time for 3 parallel lints: {:?}", elapsed);

        assert!(
            elapsed < Duration::from_millis(250),
            "Runtime was blocked! Expected < 250ms, got {:?}. \
             This indicates lint_text is blocking the async runtime.",
            elapsed
        );
    }

    async fn send_msg<W: AsyncWriteExt + Unpin>(writer: &mut W, msg: &str) {
        let content = format!("Content-Length: {}\r\n\r\n{}", msg.len(), msg);
        writer.write_all(content.as_bytes()).await.unwrap();
        writer.flush().await.unwrap();
    }

    async fn recv_msg<R: AsyncReadExt + Unpin>(reader: &mut R) -> Option<String> {
        let mut buffer = Vec::new();
        let mut content_length = 0;

        loop {
            let byte = reader.read_u8().await.ok()?;
            buffer.push(byte);
            if buffer.ends_with(b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&buffer);
                for line in headers.lines() {
                    if line.to_lowercase().starts_with("content-length:") {
                        let parts: Vec<&str> = line.split(':').collect();
                        if parts.len() == 2 {
                            content_length = parts[1].trim().parse().unwrap_or(0);
                        }
                    }
                }
                break;
            }
        }

        if content_length == 0 {
            return None;
        }

        let mut body = vec![0u8; content_length];
        reader.read_exact(&mut body).await.ok()?;

        Some(String::from_utf8(body).unwrap())
    }
}
