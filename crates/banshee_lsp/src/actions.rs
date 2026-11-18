//! Lint fixes exposed as LSP quick fixes.

use std::collections::HashMap;

use banshee_config::fixes::{apply_fixes, is_safe_fix, validate_fix_output};
use banshee_hir::Fix;
use lsp_server::ErrorCode;
use lsp_types::{
    ClientCapabilities, CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams,
    DocumentChanges, OneOf, OptionalVersionedTextDocumentIdentifier, TextDocumentEdit, Uri,
    WorkspaceEdit,
};
use serde::{Deserialize, Serialize};

use crate::{DocEntry, Session, convert};

#[derive(Default)]
pub(crate) struct ActionSupport {
    versioned_edits: bool,
    resolve_edits: bool,
}

impl ActionSupport {
    pub(crate) fn from_client(capabilities: &ClientCapabilities) -> Self {
        let versioned_edits = capabilities
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.workspace_edit.as_ref())
            .is_some_and(|edit| edit.document_changes == Some(true));
        let resolve_edits = capabilities
            .text_document
            .as_ref()
            .and_then(|document| document.code_action.as_ref())
            .is_some_and(|action| {
                action.data_support == Some(true)
                    && action.resolve_support.as_ref().is_some_and(|resolve| {
                        resolve.properties.iter().any(|property| property == "edit")
                    })
            });
        Self {
            versioned_edits,
            resolve_edits,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct FixData {
    uri: Uri,
    version: i32,
    diagnostic: usize,
    fix: usize,
}

pub(crate) fn code_actions(
    entry: &DocEntry,
    params: &CodeActionParams,
    support: &ActionSupport,
) -> Vec<CodeActionOrCommand> {
    if params.context.only.as_ref().is_some_and(|kinds| {
        !kinds
            .iter()
            .any(|kind| kind.as_str().is_empty() || kind == &CodeActionKind::QUICKFIX)
    }) || banshee_format::validate::validate_input(&entry.parse).is_err()
    {
        return Vec::new();
    }

    let uri = &params.text_document.uri;
    let mut actions = Vec::new();
    for (diagnostic, diag) in entry.analysis.diagnostics.iter().enumerate() {
        let Some(range) = diag.range else { continue };
        let range = entry.index.range(range);
        if range.end < params.range.start || range.start > params.range.end {
            continue;
        }
        let safe = is_safe_fix(diag.code);
        for (fix_index, fix) in diag.fixes.iter().enumerate() {
            let mut action = CodeAction {
                title: if safe {
                    fix.title.clone()
                } else {
                    format!("{} (may change SQL behavior)", fix.title)
                },
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![convert::diagnostic(&entry.index, diag)]),
                ..Default::default()
            };
            if support.resolve_edits {
                let data = FixData {
                    uri: uri.clone(),
                    version: entry.version,
                    diagnostic,
                    fix: fix_index,
                };
                let Ok(data) = serde_json::to_value(data) else {
                    continue;
                };
                action.data = Some(data);
            } else {
                let Some(edit) = checked_edit(uri, entry, fix, safe, support) else {
                    continue;
                };
                action.edit = Some(edit);
            }
            actions.push(CodeActionOrCommand::CodeAction(action));
        }
    }
    actions
}

pub(crate) fn resolve(
    mut action: CodeAction,
    server: &Session,
) -> Result<CodeAction, (ErrorCode, String)> {
    let invalid = || {
        (
            ErrorCode::InvalidParams,
            "Unrecognized Banshee quick fix".to_owned(),
        )
    };
    let data =
        FixData::deserialize(action.data.as_ref().ok_or_else(invalid)?).map_err(|_| invalid())?;
    let entry = server
        .doc(&data.uri)
        .filter(|entry| entry.version == data.version)
        .ok_or_else(|| {
            (
                ErrorCode::ContentModified,
                "Document changed since the quick fix was offered".to_owned(),
            )
        })?;
    let diagnostic = entry
        .analysis
        .diagnostics
        .get(data.diagnostic)
        .ok_or_else(invalid)?;
    let fix = diagnostic.fixes.get(data.fix).ok_or_else(invalid)?;
    action.edit = Some(
        checked_edit(
            &data.uri,
            entry,
            fix,
            is_safe_fix(diagnostic.code),
            &server.actions,
        )
        .ok_or_else(|| {
            (
                ErrorCode::RequestFailed,
                "The quick fix could not be validated".to_owned(),
            )
        })?,
    );
    Ok(action)
}

fn checked_edit(
    uri: &Uri,
    entry: &DocEntry,
    fix: &Fix,
    safe: bool,
    support: &ActionSupport,
) -> Option<WorkspaceEdit> {
    let source = entry.index.text();
    let output = apply_fixes(source, std::iter::once(fix))?;
    if source == output
        || validate_fix_output(source, &output, &entry.settings.config.templater, !safe).is_err()
    {
        return None;
    }

    let mut edits: Vec<_> = fix
        .edits
        .iter()
        .map(|e| convert::text_edit(&entry.index, e))
        .collect();
    edits.sort_by_key(|edit| (edit.range.start, edit.range.end));
    // Two byte ranges can touch after conversion widens an edit across CRLF.
    if edits.windows(2).any(|pair| {
        let [left, right] = [pair[0].range, pair[1].range];
        left.end > right.start || (left.start == left.end && left.start == right.start)
    }) {
        edits = banshee_format::diff_edits(source, &output)
            .into_iter()
            .map(|edit| entry.index.replacement(edit.range, &edit.new_text))
            .collect();
    }

    if support.versioned_edits {
        Some(WorkspaceEdit {
            document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: Some(entry.version),
                },
                edits: edits.into_iter().map(OneOf::Left).collect(),
            }])),
            ..Default::default()
        })
    } else {
        Some(WorkspaceEdit {
            changes: Some(HashMap::from([(uri.clone(), edits)])),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use banshee_config::BansheeConfig;
    use banshee_hir::{Diagnostic, RuleCode, TextEdit};
    use lsp_types::Position;
    use serde_json::json;
    use text_size::TextRange;

    use super::*;
    fn server() -> Session {
        Session {
            settings_for: crate::fixed_settings(BansheeConfig::default(), None),
            docs: HashMap::new(),
            actions: ActionSupport {
                versioned_edits: true,
                resolve_edits: true,
            },
        }
    }

    fn params(uri: &Uri) -> CodeActionParams {
        serde_json::from_value(json!({
            "textDocument": {"uri": uri},
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 9}},
            "context": {"diagnostics": []}
        }))
        .unwrap()
    }

    #[test]
    fn resolve_checks_revision_and_returns_a_versioned_edit() {
        let uri: Uri = "file:///fix.sql".parse().unwrap();
        let mut server = server();
        server
            .update_doc(uri.clone(), 23, crate::LineIndex::new("select 1;"))
            .unwrap();
        let request = params(&uri);
        let actions = code_actions(
            server.doc(&uri).expect("analyzed document"),
            &request,
            &server.actions,
        );
        let [CodeActionOrCommand::CodeAction(action)] = actions.as_slice() else {
            panic!("expected one code action");
        };
        assert!(action.edit.is_none());
        let edit = resolve(action.clone(), &server).unwrap().edit.unwrap();
        let Some(DocumentChanges::Edits(ref changes)) = edit.document_changes else {
            panic!("expected versioned edits");
        };
        assert_eq!(changes[0].text_document.version, Some(23));
        let OneOf::Left(ref edit) = changes[0].edits[0] else {
            panic!("expected a plain text edit");
        };
        assert_eq!(edit.new_text, "SELECT");
        assert_eq!(edit.range.end, Position::new(0, 6));

        server
            .update_doc(uri.clone(), 24, crate::LineIndex::new("select 2;"))
            .unwrap();
        let err = resolve(action.clone(), &server).unwrap_err();
        assert_eq!(err.0 as i32, ErrorCode::ContentModified as i32);
    }

    #[test]
    fn rejects_invalid_repairs_and_other_action_kinds() {
        let uri: Uri = "file:///fix.sql".parse().unwrap();
        let mut server = server();
        server
            .update_doc(uri.clone(), 1, crate::LineIndex::new("SELECT 1;"))
            .unwrap();
        let range = TextRange::at(0.into(), 6.into());
        let Some(crate::OpenDocument::Analyzed(entry)) = server.docs.get_mut(&uri) else {
            panic!("expected an analyzed document");
        };
        entry.analysis.diagnostics = vec![
            Diagnostic::warning("test")
                .with_code(RuleCode::Cp01)
                .with_range(range)
                .with_fix(Fix::new(
                    "Bad edit",
                    vec![TextEdit::replace(range, "SELECT FROM (")],
                )),
        ];
        let mut request = params(&uri);
        let eager = ActionSupport::default();
        let entry = server.doc(&uri).expect("analyzed document");
        assert!(code_actions(entry, &request, &eager).is_empty());
        let actions = code_actions(entry, &request, &server.actions);
        let [CodeActionOrCommand::CodeAction(action)] = actions.as_slice() else {
            panic!("expected a deferred candidate");
        };
        let err = resolve(action.clone(), &server).unwrap_err();
        assert_eq!(err.0 as i32, ErrorCode::RequestFailed as i32);

        request.context.only = Some(vec![CodeActionKind::SOURCE_FIX_ALL]);
        assert!(code_actions(entry, &request, &server.actions).is_empty());
    }
}
