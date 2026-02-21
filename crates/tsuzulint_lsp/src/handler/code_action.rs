//! Code action handler for auto-fix support.

use std::collections::HashMap;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tracing::debug;

use crate::conversion::{offset_to_range, positions_le};
use crate::state::SharedState;
use tsuzulint_core::Diagnostic as TsuzuLintDiagnostic;

/// Handles the `textDocument/codeAction` request.
pub async fn handle_code_action(
    state: &SharedState,
    diagnostics: &[TsuzuLintDiagnostic],
    params: CodeActionParams,
) -> Result<Option<CodeActionResponse>> {
    debug!("Code action request: {}", params.text_document.uri);

    let uri = &params.text_document.uri;
    let text = match get_document_content(state, uri) {
        Some(t) => t,
        None => return Ok(None),
    };

    let mut actions = Vec::new();

    let (wants_fix_all, wants_quickfix) = match &params.context.only {
        Some(only) => (
            only.contains(&CodeActionKind::SOURCE_FIX_ALL),
            only.contains(&CodeActionKind::QUICKFIX),
        ),
        None => (true, true),
    };

    if wants_fix_all {
        add_fix_all_actions(diagnostics, &text, uri, &mut actions);
    }

    if wants_quickfix {
        add_quickfix_actions(diagnostics, &text, uri, &params.range, &mut actions);
    }

    Ok(Some(actions))
}

fn add_fix_all_actions(
    diagnostics: &[TsuzuLintDiagnostic],
    text: &str,
    uri: &Url,
    actions: &mut Vec<CodeActionOrCommand>,
) {
    let mut edits = Vec::new();
    let mut fixable_diags: Vec<_> = diagnostics.iter().filter(|d| d.fix.is_some()).collect();

    fixable_diags.sort_by(|a, b| b.span.start.cmp(&a.span.start));

    for diag in fixable_diags {
        if let Some(ref fix) = diag.fix
            && let Some(range) =
                offset_to_range(fix.span.start as usize, fix.span.end as usize, text)
        {
            edits.push(TextEdit {
                range,
                new_text: fix.text.clone(),
            });
        }
    }

    if !edits.is_empty() {
        let mut changes = HashMap::new();
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

fn add_quickfix_actions(
    diagnostics: &[TsuzuLintDiagnostic],
    text: &str,
    uri: &Url,
    request_range: &Range,
    actions: &mut Vec<CodeActionOrCommand>,
) {
    for diag in diagnostics {
        if let Some(ref fix) = diag.fix
            && let Some(range) =
                offset_to_range(fix.span.start as usize, fix.span.end as usize, text)
            && positions_le(range.start, request_range.end)
            && positions_le(request_range.start, range.end)
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
}

fn get_document_content(state: &SharedState, uri: &Url) -> Option<String> {
    let docs = state.documents.read().ok()?;
    docs.get(uri).map(|d| d.text.clone())
}
