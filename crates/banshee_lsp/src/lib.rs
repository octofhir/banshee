//! A generic PostgreSQL language server built on the banshee engine.
//!
//! Synchronous (no async runtime): the server loop reads JSON-RPC over stdio
//! via `lsp-server`. A single [`CachedSchemaProvider`], when supplied, drives
//! schema-aware diagnostics and completion; without one the server still offers
//! formatting, syntax diagnostics and the lint rules that need no schema.

mod convert;
mod semantic_tokens;
mod sig_help;
mod symbols;

use std::collections::HashMap;
use std::error::Error;

use banshee_completion::providers::FunctionProvider;
use banshee_config::BansheeConfig;
use banshee_hir::{
    Analysis, AnalysisOptions, BuiltinLintPack, Diagnostic as HirDiagnostic, NullSchemaProvider,
    Resolution, SchemaProvider, analyze_query_with_options, resolve::resolve_column,
};
use banshee_schema::CachedSchemaProvider;
use banshee_syntax::ast::{AstNode, ColumnRef};
use lsp_server::{Connection, ExtractError, Message, Request, RequestId, Response};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams,
    CodeActionProviderCapability, CompletionItem, CompletionOptions, CompletionParams,
    CompletionResponse, Diagnostic, DocumentFormattingParams, DocumentRangeFormattingParams,
    DocumentSymbolParams, DocumentSymbolResponse, Hover, HoverContents, HoverParams,
    HoverProviderCapability, MarkupContent, MarkupKind, OneOf, Position, PublishDiagnosticsParams,
    SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensOptions, SemanticTokensParams,
    SemanticTokensResult, SemanticTokensServerCapabilities, ServerCapabilities,
    SignatureHelpOptions, SignatureHelpParams, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextEdit, Uri, WorkspaceEdit,
    notification::{
        DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
        PublishDiagnostics,
    },
    request::{
        CodeActionRequest, Completion, DocumentSymbolRequest, Formatting, HoverRequest,
        RangeFormatting, SemanticTokensFullRequest, SignatureHelpRequest,
    },
};
use text_size::TextSize;

use convert::LineIndex;

/// Runs the language server over stdio until the client disconnects.
pub fn run_stdio(
    config: BansheeConfig,
    schema: Option<CachedSchemaProvider>,
) -> anyhow::Result<()> {
    let (connection, io_threads) = Connection::stdio();
    let capabilities = serde_json::to_value(server_capabilities())?;
    let _init = connection.initialize(capabilities)?;

    let mut server = Server {
        config,
        schema,
        docs: HashMap::new(),
        connection,
    };
    let result = server.run();
    // Drop the server (and its connection sender) so the writer thread sees the
    // channel close; otherwise `join` blocks forever.
    drop(server);
    io_threads.join()?;
    result
}

fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".into(), " ".into(), "(".into(), ",".into()]),
            ..Default::default()
        }),
        document_formatting_provider: Some(OneOf::Left(true)),
        document_range_formatting_provider: Some(OneOf::Left(true)),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".into(), ",".into()]),
            ..Default::default()
        }),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                legend: SemanticTokensLegend {
                    token_types: semantic_tokens::LEGEND.to_vec(),
                    token_modifiers: vec![],
                },
                full: Some(SemanticTokensFullOptions::Bool(true)),
                range: Some(false),
                ..Default::default()
            },
        )),
        ..Default::default()
    }
}

/// A cached, analyzed document. Recomputed once per edit, then shared by
/// diagnostics, hover and code-action requests that fire on the same revision.
struct DocEntry {
    text: String,
    /// Lint findings plus parse errors, already filtered by rule selection.
    diagnostics: Vec<HirDiagnostic>,
    /// Resolved scope, used for hover.
    analysis: Analysis,
}

struct Server {
    config: BansheeConfig,
    schema: Option<CachedSchemaProvider>,
    docs: HashMap<Uri, DocEntry>,
    connection: Connection,
}

impl Server {
    fn run(&mut self) -> anyhow::Result<()> {
        // Drain the receiver. `recv` errors only when the channel is closed.
        while let Ok(msg) = self.connection.receiver.recv() {
            match msg {
                Message::Request(req) => {
                    if self.connection.handle_shutdown(&req)? {
                        return Ok(());
                    }
                    self.handle_request(req)?;
                }
                Message::Notification(note) => self.handle_notification(note)?,
                Message::Response(_) => {}
            }
        }
        Ok(())
    }

    fn handle_request(&mut self, req: Request) -> anyhow::Result<()> {
        let req = match cast::<Completion>(req) {
            Ok((id, params)) => return self.on_completion(id, params),
            Err(req) => req,
        };
        let req = match cast::<Formatting>(req) {
            Ok((id, params)) => return self.on_formatting(id, params),
            Err(req) => req,
        };
        let req = match cast::<RangeFormatting>(req) {
            Ok((id, params)) => return self.on_range_formatting(id, params),
            Err(req) => req,
        };
        let req = match cast::<CodeActionRequest>(req) {
            Ok((id, params)) => return self.on_code_action(id, params),
            Err(req) => req,
        };
        let req = match cast::<HoverRequest>(req) {
            Ok((id, params)) => return self.on_hover(id, params),
            Err(req) => req,
        };
        let req = match cast::<DocumentSymbolRequest>(req) {
            Ok((id, params)) => return self.on_document_symbol(id, params),
            Err(req) => req,
        };
        let req = match cast::<SemanticTokensFullRequest>(req) {
            Ok((id, params)) => return self.on_semantic_tokens(id, params),
            Err(req) => req,
        };
        let req = match cast::<SignatureHelpRequest>(req) {
            Ok((id, params)) => return self.on_signature_help(id, params),
            Err(req) => req,
        };
        // Unhandled method: reply with an empty result so the client is not left
        // waiting.
        self.respond(Response::new_ok(req.id, serde_json::Value::Null))
    }

    fn handle_notification(&mut self, note: lsp_server::Notification) -> anyhow::Result<()> {
        match note.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let p: lsp_types::DidOpenTextDocumentParams = serde_json::from_value(note.params)?;
                let uri = p.text_document.uri.clone();
                self.update_doc(uri.clone(), p.text_document.text);
                self.publish_diagnostics(&uri)?;
            }
            DidChangeTextDocument::METHOD => {
                let p: lsp_types::DidChangeTextDocumentParams =
                    serde_json::from_value(note.params)?;
                // FULL sync: last change holds the whole document.
                if let Some(change) = p.content_changes.into_iter().last() {
                    let uri = p.text_document.uri.clone();
                    self.update_doc(uri.clone(), change.text);
                    self.publish_diagnostics(&uri)?;
                }
            }
            DidCloseTextDocument::METHOD => {
                let p: lsp_types::DidCloseTextDocumentParams = serde_json::from_value(note.params)?;
                self.docs.remove(&p.text_document.uri);
            }
            _ => {}
        }
        Ok(())
    }

    // ---- analysis -----------------------------------------------------------

    fn hir_provider(&self) -> Option<&dyn SchemaProvider> {
        self.schema.as_ref().map(|p| p as &dyn SchemaProvider)
    }

    fn lint_options(&self) -> AnalysisOptions {
        let mut packs = vec![
            BuiltinLintPack::Core,
            BuiltinLintPack::Jsonb,
            BuiltinLintPack::Convention,
            BuiltinLintPack::Migration,
        ];
        if self.config.lint.is_rule_enabled("CP01") || self.config.lint.is_rule_enabled("CP02") {
            packs.push(BuiltinLintPack::Capitalisation);
        }
        AnalysisOptions::new().with_builtin_lint_packs(packs)
    }

    /// Parses and analyzes `text`, caching the result under `uri`.
    fn update_doc(&mut self, uri: Uri, text: String) {
        let parse = banshee_parser::parse(&text);
        let options = self.lint_options();
        let null = NullSchemaProvider;
        let provider = self.hir_provider().unwrap_or(&null);
        let schema_aware = self.schema.is_some();

        let analysis = analyze_query_with_options(&parse, provider, &options);

        let mut diagnostics: Vec<HirDiagnostic> = analysis
            .diagnostics
            .iter()
            .filter(|d| match d.code {
                None => schema_aware,
                // Reference checks (RF*) need a schema; suppress them without one.
                Some(code) if code.is_reference() => {
                    schema_aware && self.config.lint.is_rule_enabled(code.as_str())
                }
                Some(code) => self.config.lint.is_rule_enabled(code.as_str()),
            })
            .cloned()
            .collect();
        for err in parse.errors() {
            diagnostics.push(HirDiagnostic::error(err.message.clone()).with_range(err.range));
        }

        self.docs.insert(
            uri,
            DocEntry {
                text,
                diagnostics,
                analysis,
            },
        );
    }

    fn publish_diagnostics(&self, uri: &Uri) -> anyhow::Result<()> {
        let Some(entry) = self.docs.get(uri) else {
            return Ok(());
        };
        let index = LineIndex::new(&entry.text);
        let diags: Vec<Diagnostic> = entry
            .diagnostics
            .iter()
            .map(|d| convert::diagnostic(&index, d))
            .collect();

        let params = PublishDiagnosticsParams {
            uri: uri.clone(),
            diagnostics: diags,
            version: None,
        };
        self.notify::<PublishDiagnostics>(params)
    }

    // ---- requests -----------------------------------------------------------

    fn on_completion(&mut self, id: RequestId, params: CompletionParams) -> anyhow::Result<()> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let items = self
            .docs
            .get(&uri)
            .map(|entry| self.completions(&entry.text, pos))
            .unwrap_or_default();
        self.respond(Response::new_ok(id, CompletionResponse::Array(items)))
    }

    fn completions(&self, text: &str, pos: Position) -> Vec<CompletionItem> {
        let index = LineIndex::new(text);
        let offset = TextSize::from(index.offset(pos));
        let mut request = banshee_completion::CompletionRequest::new(text, offset);
        if let Some(p) = &self.schema {
            request = request.with_schema_provider(p).with_function_provider(p);
        }
        let result = banshee_completion::complete(request);
        result
            .items
            .into_iter()
            .map(|item| CompletionItem {
                label: item.label,
                detail: item.detail,
                insert_text: item.insert_text,
                ..Default::default()
            })
            .collect()
    }

    fn on_formatting(
        &mut self,
        id: RequestId,
        params: DocumentFormattingParams,
    ) -> anyhow::Result<()> {
        let uri = params.text_document.uri;
        let edits = self
            .docs
            .get(&uri)
            .map(|entry| {
                let index = LineIndex::new(&entry.text);
                self.config
                    .format_edits(&entry.text)
                    .iter()
                    .map(|e| TextEdit {
                        range: index.range(e.range),
                        new_text: e.new_text.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        self.respond(Response::new_ok(id, edits))
    }

    fn on_range_formatting(
        &mut self,
        id: RequestId,
        params: DocumentRangeFormattingParams,
    ) -> anyhow::Result<()> {
        let uri = params.text_document.uri;
        let edits = self
            .docs
            .get(&uri)
            .map(|entry| {
                let index = LineIndex::new(&entry.text);
                let start = index.offset(params.range.start);
                let end = index.offset(params.range.end);
                self.config
                    .format_range(&entry.text, start, end)
                    .iter()
                    .map(|e| TextEdit {
                        range: index.range(e.range),
                        new_text: e.new_text.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        self.respond(Response::new_ok(id, edits))
    }

    fn on_code_action(&mut self, id: RequestId, params: CodeActionParams) -> anyhow::Result<()> {
        let uri = params.text_document.uri.clone();
        let actions = self
            .docs
            .get(&uri)
            .map(|entry| code_actions(&uri, entry, &params))
            .unwrap_or_default();
        self.respond(Response::new_ok(id, actions))
    }

    fn on_document_symbol(
        &mut self,
        id: RequestId,
        params: DocumentSymbolParams,
    ) -> anyhow::Result<()> {
        let uri = params.text_document.uri;
        let symbols = self
            .docs
            .get(&uri)
            .map(|entry| {
                let parse = banshee_parser::parse(&entry.text);
                let index = LineIndex::new(&entry.text);
                symbols::document_symbols(&parse.syntax(), &index)
            })
            .unwrap_or_default();
        self.respond(Response::new_ok(
            id,
            DocumentSymbolResponse::Nested(symbols),
        ))
    }

    fn on_signature_help(
        &mut self,
        id: RequestId,
        params: SignatureHelpParams,
    ) -> anyhow::Result<()> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let help = self.docs.get(&uri).and_then(|entry| {
            let parse = banshee_parser::parse(&entry.text);
            let index = LineIndex::new(&entry.text);
            let offset = index.offset(pos);
            let provider = self.schema.as_ref().map(|p| p as &dyn FunctionProvider);
            sig_help::signature_help(&parse.syntax(), offset, provider)
        });
        self.respond(Response::new_ok(id, help))
    }

    fn on_semantic_tokens(
        &mut self,
        id: RequestId,
        params: SemanticTokensParams,
    ) -> anyhow::Result<()> {
        let uri = params.text_document.uri;
        let tokens = self.docs.get(&uri).map(|entry| {
            let parse = banshee_parser::parse(&entry.text);
            let index = LineIndex::new(&entry.text);
            semantic_tokens::semantic_tokens(&parse.syntax(), &index)
        });
        let result = tokens.map(SemanticTokensResult::Tokens);
        self.respond(Response::new_ok(id, result))
    }

    fn on_hover(&mut self, id: RequestId, params: HoverParams) -> anyhow::Result<()> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let hover = self.docs.get(&uri).and_then(|entry| hover_at(entry, pos));
        self.respond(Response::new_ok(id, hover))
    }

    // ---- transport ----------------------------------------------------------

    fn respond(&self, response: Response) -> anyhow::Result<()> {
        self.connection
            .sender
            .send(Message::Response(response))
            .map_err(|e| anyhow::anyhow!("failed to send response: {e}"))
    }

    fn notify<N: lsp_types::notification::Notification>(
        &self,
        params: N::Params,
    ) -> anyhow::Result<()> {
        let note = lsp_server::Notification::new(N::METHOD.to_string(), params);
        self.connection
            .sender
            .send(Message::Notification(note))
            .map_err(|e| anyhow::anyhow!("failed to send notification: {e}"))
    }
}

/// Builds quick-fix code actions from a document's cached diagnostics that
/// overlap the requested range.
fn code_actions(
    uri: &Uri,
    entry: &DocEntry,
    params: &CodeActionParams,
) -> Vec<CodeActionOrCommand> {
    let index = LineIndex::new(&entry.text);
    let want = params.range;
    let mut actions = Vec::new();

    for diag in &entry.diagnostics {
        let Some(range) = diag.range else { continue };
        let lsp_range = index.range(range);
        if lsp_range.end < want.start || lsp_range.start > want.end {
            continue;
        }
        for fix in &diag.fixes {
            let edits: Vec<TextEdit> = fix
                .edits
                .iter()
                .map(|e| convert::text_edit(&index, e))
                .collect();
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: fix.title.clone(),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![convert::diagnostic(&index, diag)]),
                edit: Some(WorkspaceEdit {
                    changes: Some(HashMap::from([(uri.clone(), edits)])),
                    ..Default::default()
                }),
                ..Default::default()
            }));
        }
    }
    actions
}

/// Produces hover content for a column reference under the cursor.
///
/// Finds the innermost [`ColumnRef`] covering the offset, then resolves it
/// against the analyzed scope. Resolution needs a schema, so hover is empty
/// when the server runs schema-less.
fn hover_at(entry: &DocEntry, pos: Position) -> Option<Hover> {
    let index = LineIndex::new(&entry.text);
    let offset = index.offset(pos);

    let parse = banshee_parser::parse(&entry.text);
    let root = parse.syntax();

    // Innermost ColumnRef whose range covers the offset.
    let column_ref = root
        .descendants()
        .filter_map(|n| ColumnRef::cast(n.clone()))
        .filter(|c| {
            let r = c.syntax().text_range();
            u32::from(r.start()) <= offset && offset <= u32::from(r.end())
        })
        .min_by_key(|c| u32::from(c.syntax().text_range().len()))?;

    // Build dotted parts (schema?, table?, column) preserving order.
    let mut parts = Vec::new();
    if let Some(s) = column_ref.schema() {
        parts.push(s.text().to_string());
    }
    if let Some(t) = column_ref.table() {
        parts.push(t.text().to_string());
    }
    parts.push(column_ref.column()?.text().to_string());

    let (resolved, ambiguous) = match resolve_column(&parts, &entry.analysis.scope) {
        Resolution::Resolved(c) => (c, false),
        Resolution::Ambiguous(mut c) if !c.is_empty() => (c.remove(0), true),
        _ => return None,
    };

    let ty = resolved
        .column
        .data_type
        .as_ref()
        .map(|t| t.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let table = resolved
        .table
        .alias
        .clone()
        .unwrap_or_else(|| resolved.table.name.clone());

    let value = format!(
        "**{}** `{}`\n\ntype: `{}`\n\nfrom: `{}`",
        if ambiguous {
            "column (ambiguous)"
        } else {
            "column"
        },
        resolved.column.name,
        ty,
        table,
    );

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: Some(index.range(column_ref.syntax().text_range())),
    })
}

/// Attempts to extract a typed request; returns the original on method mismatch.
fn cast<R>(req: Request) -> Result<(RequestId, R::Params), Request>
where
    R: lsp_types::request::Request,
    R::Params: serde::de::DeserializeOwned,
{
    match req.extract::<R::Params>(R::METHOD) {
        Ok(value) => Ok(value),
        Err(ExtractError::MethodMismatch(req)) => Err(req),
        Err(ExtractError::JsonError { method, error }) => {
            // Malformed params: log and swallow by faking a mismatch so the
            // dispatcher can fall through to the null response.
            let _: (&str, Box<dyn Error>) = (&method, Box::new(error));
            Err(Request {
                id: RequestId::from(0),
                method: String::new(),
                params: serde_json::Value::Null,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use banshee_schema::{ColumnEntry, SchemaSnapshot, TableEntry, TableKind};

    fn provider() -> CachedSchemaProvider {
        let mut snap = SchemaSnapshot::new("fp", "public");
        snap.tables.push(TableEntry {
            schema: "public".into(),
            name: "patient".into(),
            kind: TableKind::Table,
            columns: vec![ColumnEntry {
                name: "id".into(),
                data_type: "integer".into(),
                nullable: false,
                is_primary_key: true,
                ordinal: 0,
                jsonb: None,
            }],
        });
        CachedSchemaProvider::new(snap)
    }

    fn entry_for(text: &str, p: &CachedSchemaProvider) -> DocEntry {
        let parse = banshee_parser::parse(text);
        let analysis = analyze_query_with_options(
            &parse,
            p as &dyn SchemaProvider,
            &AnalysisOptions::default(),
        );
        DocEntry {
            text: text.to_string(),
            diagnostics: Vec::new(),
            analysis,
        }
    }

    #[test]
    fn hover_reports_column_type() {
        let p = provider();
        let text = "SELECT id FROM patient";
        let entry = entry_for(text, &p);
        let index = LineIndex::new(text);
        // 'id' starts at byte offset 7.
        let pos = index.position(7);
        let hover = hover_at(&entry, pos).expect("hover present on resolved column");
        if let HoverContents::Markup(m) = hover.contents {
            assert!(m.value.contains("id"), "{}", m.value);
            assert!(m.value.contains("integer"), "{}", m.value);
        } else {
            panic!("expected markup hover");
        }
    }

    #[test]
    fn hover_absent_off_reference() {
        let p = provider();
        let text = "SELECT id FROM patient";
        let entry = entry_for(text, &p);
        let index = LineIndex::new(text);
        // Offset 0 is the SELECT keyword, not a column reference.
        let pos = index.position(0);
        assert!(hover_at(&entry, pos).is_none());
    }
}
