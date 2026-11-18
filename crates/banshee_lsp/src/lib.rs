//! A generic PostgreSQL language server built on the banshee engine.
//!
//! Each client has a synchronous [`Session`] that handles LSP messages. The
//! stdio adapter and embedded hosts use the same document and request logic.

mod actions;
mod convert;
mod file_uri;
mod semantic_tokens;
mod sig_help;
mod symbols;

use std::cell::OnceCell;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use banshee_completion::providers::{FunctionProvider, SchemaProvider as CompletionSchema};
use banshee_config::{BansheeConfig, ConfiguredAnalysis, Templated};
use banshee_hir::{Resolution, SchemaProvider, resolve::resolve_column};
use banshee_schema::CachedSchemaProvider;
use banshee_syntax::Parse;
use banshee_syntax::ast::{AstNode, ColumnRef};
use lsp_server::{Connection, ErrorCode, RequestId};
pub use lsp_server::{Message, Notification, Request, Response};
pub use lsp_types;
use lsp_types::request::Request as _;
use lsp_types::{
    ClientCapabilities, CodeAction, CodeActionKind, CodeActionOptions, CodeActionParams,
    CodeActionProviderCapability, CompletionList, CompletionOptions, CompletionParams,
    CompletionResponse, Diagnostic, DiagnosticSeverity, DocumentFormattingParams,
    DocumentRangeFormattingParams, DocumentSymbolParams, DocumentSymbolResponse, Hover,
    HoverContents, HoverParams, HoverProviderCapability, InitializeParams, MarkupContent,
    MarkupKind, OneOf, Position, PublishDiagnosticsParams, SemanticTokensFullOptions,
    SemanticTokensLegend, SemanticTokensOptions, SemanticTokensParams, SemanticTokensResult,
    SemanticTokensServerCapabilities, ServerCapabilities, SignatureHelpOptions,
    SignatureHelpParams, TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    Uri,
    notification::{
        DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
        PublishDiagnostics,
    },
    request::{
        CodeActionRequest, CodeActionResolveRequest, Completion, DocumentSymbolRequest, Formatting,
        HoverRequest, RangeFormatting, SemanticTokensFullRequest, SignatureHelpRequest,
    },
};
use text_size::{TextRange, TextSize};

use actions::ActionSupport;
use convert::LineIndex;

/// Schema data used for analysis, completion, and signature help.
pub trait DocumentSchema: SchemaProvider + CompletionSchema + FunctionProvider {}

impl<T: SchemaProvider + CompletionSchema + FunctionProvider> DocumentSchema for T {}

/// Configuration and optional database schema for a document.
#[derive(Default)]
pub struct DocumentSettings {
    pub config: BansheeConfig,
    pub schema: Option<Arc<dyn DocumentSchema>>,
}

impl DocumentSettings {
    /// Builds settings from a cached schema snapshot.
    pub fn from_cache(config: BansheeConfig, schema: Option<CachedSchemaProvider>) -> Self {
        Self {
            config,
            schema: schema.map(|schema| Arc::new(schema) as Arc<dyn DocumentSchema>),
        }
    }
}

type SettingsResolver = Box<dyn FnMut(&Uri) -> anyhow::Result<Arc<DocumentSettings>> + Send>;

fn fixed_settings(config: BansheeConfig, schema: Option<CachedSchemaProvider>) -> SettingsResolver {
    let settings = Arc::new(DocumentSettings::from_cache(config, schema));
    Box::new(move |_| Ok(Arc::clone(&settings)))
}

/// Runs the language server over stdio with fixed settings for all documents.
pub fn run_stdio(
    config: BansheeConfig,
    schema: Option<CachedSchemaProvider>,
) -> anyhow::Result<()> {
    run_stdio_session(fixed_settings(config, schema))
}

/// Runs the language server over stdio and resolves settings for each open
/// document revision. The resolver receives the local file path when available.
pub fn run_stdio_with_settings(
    mut resolve: impl FnMut(Option<&Path>) -> anyhow::Result<Arc<DocumentSettings>> + Send + 'static,
) -> anyhow::Result<()> {
    run_stdio_session(move |uri| resolve(file_uri::local_file_path(uri).as_deref()))
}

fn run_stdio_session(
    resolve: impl FnMut(&Uri) -> anyhow::Result<Arc<DocumentSettings>> + Send + 'static,
) -> anyhow::Result<()> {
    let (connection, io_threads) = Connection::stdio();
    let result = serve_stdio(&connection, resolve);
    // Release the channels before waiting for the transport threads.
    drop(connection);
    result?;
    Ok(io_threads.join()?)
}

fn serve_stdio(
    connection: &Connection,
    resolve: impl FnMut(&Uri) -> anyhow::Result<Arc<DocumentSettings>> + Send + 'static,
) -> anyhow::Result<()> {
    let capabilities = serde_json::to_value(server_capabilities())?;
    let init: InitializeParams = serde_json::from_value(connection.initialize(capabilities)?)?;

    let mut session = Session::new(&init.capabilities, resolve);
    while let Ok(message) = connection.receiver.recv() {
        if let Message::Request(request) = &message
            && connection.handle_shutdown(request)?
        {
            break;
        }
        match session.handle_message(message) {
            Ok(Some(reply)) => connection.sender.send(reply)?,
            Ok(None) => {}
            Err(error) => eprintln!("warning: could not apply LSP notification: {error}"),
        }
    }
    Ok(())
}

/// Capabilities to advertise when initializing a Banshee session.
pub fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::INCREMENTAL),
                ..Default::default()
            },
        )),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".into(), " ".into(), "(".into(), ",".into()]),
            ..Default::default()
        }),
        document_formatting_provider: Some(OneOf::Left(true)),
        document_range_formatting_provider: Some(OneOf::Left(true)),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![CodeActionKind::QUICKFIX]),
            resolve_provider: Some(true),
            ..Default::default()
        })),
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

/// The source, syntax and analysis of a single document revision.
struct DocEntry {
    version: i32,
    parse: Parse,
    native_parse: OnceCell<Parse>,
    index: LineIndex,
    templated: Option<Templated>,
    analysis: ConfiguredAnalysis,
    settings: Arc<DocumentSettings>,
}

enum OpenDocument {
    Analyzed(Box<DocEntry>),
    Unconfigured { version: i32, index: LineIndex },
    OutOfSync { version: i32 },
}

impl OpenDocument {
    fn version(&self) -> i32 {
        match self {
            Self::Analyzed(entry) => entry.version,
            Self::Unconfigured { version, .. } | Self::OutOfSync { version } => *version,
        }
    }

    fn index(&self) -> Option<&LineIndex> {
        match self {
            Self::Analyzed(entry) => Some(&entry.index),
            Self::Unconfigured { index, .. } => Some(index),
            Self::OutOfSync { .. } => None,
        }
    }
}

impl DocEntry {
    fn parsed_text(&self) -> &str {
        self.templated
            .as_ref()
            .map_or(self.index.text(), Templated::text)
    }

    fn parsed_offset(&self, pos: Position) -> TextSize {
        let offset = TextSize::from(self.index.offset(pos));
        self.templated
            .as_ref()
            .map_or(offset, |source| source.rendered_offset(offset))
    }

    fn source_range(&self, range: TextRange) -> TextRange {
        self.templated
            .as_ref()
            .map_or(range, |source| source.map_range(range))
    }

    fn format_edits(&self, selection: Option<TextRange>) -> Vec<lsp_types::TextEdit> {
        let source = self.index.text();
        let template = self.templated.as_ref().map(|text| (text, &self.parse));
        let native = if template.is_some() {
            self.native_parse
                .get_or_init(|| banshee_parser::parse(source))
        } else {
            &self.parse
        };
        self.settings
            .config
            .format_parsed_edits(source, native, template, selection)
            .into_iter()
            .map(|edit| self.index.replacement(edit.range, &edit.new_text))
            .collect()
    }
}

/// One initialized client's documents, settings resolver, and LSP features.
///
/// The host handles LSP initialization and shutdown, and feeds messages in
/// arrival order. A session does synchronous work and can be moved to a
/// blocking worker. Give each client its own session.
pub struct Session {
    settings_for: SettingsResolver,
    docs: HashMap<Uri, OpenDocument>,
    actions: ActionSupport,
}

impl Session {
    fn doc(&self, uri: &Uri) -> Option<&DocEntry> {
        match self.docs.get(uri)? {
            OpenDocument::Analyzed(entry) => Some(entry),
            _ => None,
        }
    }

    /// Starts a session with client capabilities and a resolver that receives
    /// the original document URI on each open or change.
    pub fn new(
        capabilities: &ClientCapabilities,
        resolve: impl FnMut(&Uri) -> anyhow::Result<Arc<DocumentSettings>> + Send + 'static,
    ) -> Self {
        Self {
            settings_for: Box::new(resolve),
            docs: HashMap::new(),
            actions: ActionSupport::from_client(capabilities),
        }
    }

    /// Returns the response or notification to send to this client. Malformed
    /// notification parameters return an error to the host.
    pub fn handle_message(&mut self, message: Message) -> anyhow::Result<Option<Message>> {
        match message {
            Message::Request(request) => Ok(Some(Message::Response(self.handle_request(request)))),
            Message::Notification(note) => {
                Ok(self.handle_notification(note)?.map(Message::Notification))
            }
            Message::Response(_) => Ok(None),
        }
    }

    fn handle_request(&self, req: Request) -> Response {
        match req.method.as_str() {
            Completion::METHOD => respond(req, |id, p| self.on_completion(id, p)),
            Formatting::METHOD => respond(req, |id, p| self.on_formatting(id, p)),
            RangeFormatting::METHOD => respond(req, |id, p| self.on_range_formatting(id, p)),
            CodeActionRequest::METHOD => respond(req, |id, p| self.on_code_action(id, p)),
            CodeActionResolveRequest::METHOD => {
                respond(req, |id, p| self.on_code_action_resolve(id, p))
            }
            HoverRequest::METHOD => respond(req, |id, p| self.on_hover(id, p)),
            DocumentSymbolRequest::METHOD => respond(req, |id, p| self.on_document_symbol(id, p)),
            SemanticTokensFullRequest::METHOD => {
                respond(req, |id, p| self.on_semantic_tokens(id, p))
            }
            SignatureHelpRequest::METHOD => respond(req, |id, p| self.on_signature_help(id, p)),
            _ => Response::new_err(req.id, ErrorCode::MethodNotFound as i32, req.method),
        }
    }

    fn handle_notification(&mut self, note: Notification) -> anyhow::Result<Option<Notification>> {
        match note.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let p: lsp_types::DidOpenTextDocumentParams = serde_json::from_value(note.params)?;
                Ok(Some(self.sync_doc(
                    p.text_document.uri,
                    p.text_document.version,
                    LineIndex::new(p.text_document.text),
                )))
            }
            DidChangeTextDocument::METHOD => {
                let p: lsp_types::DidChangeTextDocumentParams =
                    serde_json::from_value(note.params)?;
                let uri = p.text_document.uri;
                let version = p.text_document.version;
                let Some(document) = self.docs.get(&uri) else {
                    return Ok(None);
                };
                if version <= document.version() || p.content_changes.is_empty() {
                    return Ok(None);
                }
                let index = match LineIndex::apply_changes(document.index(), p.content_changes) {
                    Ok(index) => index,
                    Err(error) => {
                        self.docs.insert(uri, OpenDocument::OutOfSync { version });
                        return Err(error);
                    }
                };
                Ok(Some(self.sync_doc(uri, version, index)))
            }
            DidCloseTextDocument::METHOD => {
                let p: lsp_types::DidCloseTextDocumentParams = serde_json::from_value(note.params)?;
                let uri = p.text_document.uri;
                self.docs.remove(&uri);
                Ok(Some(publish_diagnostics(PublishDiagnosticsParams {
                    uri,
                    diagnostics: Vec::new(),
                    version: None,
                })))
            }
            _ => Ok(None),
        }
    }

    // ---- analysis -----------------------------------------------------------

    /// Parses and analyzes `text`, caching the result under `uri`.
    fn update_doc(&mut self, uri: Uri, version: i32, index: LineIndex) -> anyhow::Result<()> {
        let settings = match (self.settings_for)(&uri) {
            Ok(settings) => settings,
            Err(error) => {
                self.docs
                    .insert(uri, OpenDocument::Unconfigured { version, index });
                return Err(error);
            }
        };
        let templated = settings.config.templater.render(index.text());
        let parsed_text = templated.as_ref().map_or(index.text(), Templated::text);
        let parse = banshee_parser::parse(parsed_text);
        let analysis = settings.config.lint.analyze(
            &parse,
            settings.schema.as_deref().map(|p| p as &dyn SchemaProvider),
            Default::default(),
            templated.as_ref(),
        );

        self.docs.insert(
            uri,
            OpenDocument::Analyzed(Box::new(DocEntry {
                version,
                parse,
                native_parse: OnceCell::new(),
                index,
                templated,
                analysis,
                settings,
            })),
        );
        Ok(())
    }

    fn sync_doc(&mut self, uri: Uri, version: i32, index: LineIndex) -> Notification {
        let diagnostics = match self.update_doc(uri.clone(), version, index) {
            Ok(()) => self.doc(&uri).map_or_else(Vec::new, |entry| {
                entry
                    .analysis
                    .diagnostics
                    .iter()
                    .map(|d| convert::diagnostic(&entry.index, d))
                    .collect()
            }),
            Err(error) => {
                vec![Diagnostic {
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("banshee".to_owned()),
                    message: format!("Could not load Banshee settings: {error:#}"),
                    ..Default::default()
                }]
            }
        };
        publish_diagnostics(PublishDiagnosticsParams {
            uri,
            diagnostics,
            version: Some(version),
        })
    }

    // ---- requests -----------------------------------------------------------

    fn on_completion(&self, id: RequestId, params: CompletionParams) -> Response {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let result = self
            .doc(&uri)
            .map(|entry| self.completions(entry, pos))
            .unwrap_or_else(|| CompletionResponse::Array(Vec::new()));
        Response::new_ok(id, result)
    }

    fn completions(&self, entry: &DocEntry, pos: Position) -> CompletionResponse {
        let settings = &entry.settings.config.completion;
        let mut request = banshee_completion::CompletionRequest::new(
            entry.parsed_text(),
            entry.parsed_offset(pos),
        )
        .with_parse(&entry.parse)
        .with_keywords(settings.keywords)
        .with_functions(settings.functions)
        .with_limit(settings.max_items);
        if let Some(p) = entry.settings.schema.as_deref() {
            request = request.with_schema_provider(p).with_function_provider(p);
        }
        let result = banshee_completion::complete(request);
        let items = result
            .items
            .into_iter()
            .map(convert::completion_item)
            .collect();
        if result.is_incomplete {
            CompletionResponse::List(CompletionList {
                is_incomplete: true,
                items,
            })
        } else {
            CompletionResponse::Array(items)
        }
    }

    fn on_formatting(&self, id: RequestId, params: DocumentFormattingParams) -> Response {
        let uri = params.text_document.uri;
        let edits = self
            .doc(&uri)
            .map(|entry| entry.format_edits(None))
            .unwrap_or_default();
        Response::new_ok(id, edits)
    }

    fn on_range_formatting(
        &self,
        id: RequestId,
        params: DocumentRangeFormattingParams,
    ) -> Response {
        let uri = params.text_document.uri;
        let edits = self
            .doc(&uri)
            .map(|entry| {
                let start = entry.index.offset(params.range.start);
                let end = entry.index.offset(params.range.end);
                if start < end {
                    entry.format_edits(Some(TextRange::new(start.into(), end.into())))
                } else {
                    Vec::new()
                }
            })
            .unwrap_or_default();
        Response::new_ok(id, edits)
    }

    fn on_code_action(&self, id: RequestId, params: CodeActionParams) -> Response {
        let actions = self
            .doc(&params.text_document.uri)
            .map(|entry| actions::code_actions(entry, &params, &self.actions))
            .unwrap_or_default();
        Response::new_ok(id, actions)
    }

    fn on_code_action_resolve(&self, id: RequestId, action: CodeAction) -> Response {
        match actions::resolve(action, self) {
            Ok(action) => Response::new_ok(id, action),
            Err((code, message)) => Response::new_err(id, code as i32, message),
        }
    }

    fn on_document_symbol(&self, id: RequestId, params: DocumentSymbolParams) -> Response {
        let uri = params.text_document.uri;
        let symbols = self
            .doc(&uri)
            .map(|entry| {
                symbols::document_symbols(&entry.parse.syntax(), |range| {
                    entry.index.range(entry.source_range(range))
                })
            })
            .unwrap_or_default();
        Response::new_ok(id, DocumentSymbolResponse::Nested(symbols))
    }

    fn on_signature_help(&self, id: RequestId, params: SignatureHelpParams) -> Response {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let help = self.doc(&uri).and_then(|entry| {
            let offset = u32::from(entry.parsed_offset(pos));
            let provider = entry
                .settings
                .schema
                .as_deref()
                .map(|p| p as &dyn FunctionProvider);
            sig_help::signature_help(&entry.parse.syntax(), offset, provider)
        });
        Response::new_ok(id, help)
    }

    fn on_semantic_tokens(&self, id: RequestId, params: SemanticTokensParams) -> Response {
        let uri = params.text_document.uri;
        let tokens = self.doc(&uri).map(|entry| {
            semantic_tokens::semantic_tokens(
                &entry.parse.syntax(),
                &entry.index,
                entry.templated.as_ref(),
            )
        });
        let result = tokens.map(SemanticTokensResult::Tokens);
        Response::new_ok(id, result)
    }

    fn on_hover(&self, id: RequestId, params: HoverParams) -> Response {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let hover = self.doc(&uri).and_then(|entry| hover_at(entry, pos));
        Response::new_ok(id, hover)
    }
}

fn publish_diagnostics(params: PublishDiagnosticsParams) -> Notification {
    Notification::new(PublishDiagnostics::METHOD.to_owned(), params)
}

/// Produces hover content for a column reference under the cursor.
///
/// Finds the innermost [`ColumnRef`] covering the offset, then resolves it
/// against the analyzed scope. Resolution needs a schema, so hover is empty
/// when the server runs schema-less.
fn hover_at(entry: &DocEntry, pos: Position) -> Option<Hover> {
    let index = &entry.index;
    let offset = u32::from(entry.parsed_offset(pos));

    let root = entry.parse.syntax();

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
    let column = column_ref.column()?;
    parts.push(column.text().to_string());

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
        range: Some(index.range(entry.source_range(TextRange::new(
            column_ref.syntax().text_range().start(),
            column.text_range().end(),
        )))),
    })
}

fn respond<P: serde::de::DeserializeOwned>(
    req: Request,
    handle: impl FnOnce(RequestId, P) -> Response,
) -> Response {
    match serde_json::from_value(req.params) {
        Ok(params) => handle(req.id, params),
        Err(error) => Response::new_err(req.id, ErrorCode::InvalidParams as i32, error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use banshee_schema::{ColumnEntry, SchemaSnapshot, TableEntry, TableKind};
    use serde_json::json;

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
        let analysis =
            BansheeConfig::default()
                .lint
                .analyze(&parse, Some(p), Default::default(), None);
        DocEntry {
            version: 1,
            parse,
            native_parse: OnceCell::new(),
            index: LineIndex::new(text),
            templated: None,
            analysis,
            settings: Arc::new(DocumentSettings {
                config: BansheeConfig::default(),
                schema: Some(Arc::new(p.clone())),
            }),
        }
    }

    #[test]
    fn hover_reports_column_type() {
        let p = provider();
        let text = "SELECT id FROM patient";
        let entry = entry_for(text, &p);
        // 'id' starts at byte offset 7.
        let pos = entry.index.position(7);
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
        // Offset 0 is the SELECT keyword, not a column reference.
        let pos = entry.index.position(0);
        assert!(hover_at(&entry, pos).is_none());
    }

    #[test]
    fn completion_reports_incomplete_lists_and_item_metadata() -> anyhow::Result<()> {
        let config = BansheeConfig::from_toml("[completion]\nkeywords = false\nmax-items = 1")?;
        let mut server = Session::new(&ClientCapabilities::default(), fixed_settings(config, None));
        let uri: Uri = "file:///completion.sql".parse()?;
        server.update_doc(uri.clone(), 1, LineIndex::new("SELECT "))?;
        let CompletionResponse::List(list) = server.completions(
            server.doc(&uri).expect("analyzed document"),
            Position::new(0, 7),
        ) else {
            panic!("expected a limited completion list");
        };
        assert!(list.is_incomplete);
        assert_eq!(list.items.len(), 1);
        let item = &list.items[0];
        assert_eq!(item.kind, Some(lsp_types::CompletionItemKind::FUNCTION));
        assert!(item.sort_text.is_some());
        assert!(item.documentation.is_some());
        Ok(())
    }

    #[test]
    fn templated_document_uses_source_positions_and_configured_lints() -> anyhow::Result<()> {
        let config = BansheeConfig::from_toml(
            r#"
            [templater]
            style = "colon"
            [lint]
            select = ["CP01"]
            [lint.rules.CP01]
            policy = "lower"
            severity = "info"
            "#,
        )?;
        let mut server = Session::new(
            &ClientCapabilities::default(),
            fixed_settings(config, Some(provider())),
        );
        let uri: Uri = "file:///template.sql".parse()?;
        let source = "SELECT :customer_identifier, '😀', id FROM patient";
        server.update_doc(uri.clone(), 1, LineIndex::new(source))?;
        let entry = server.doc(&uri).expect("analyzed document");
        assert!(
            entry.parse.errors().is_empty(),
            "{:?}",
            entry.parse.errors()
        );
        let start = source.find("FROM").unwrap() as u32;
        let from = TextRange::at(start.into(), 4.into());
        let diagnostic = entry
            .analysis
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.range == Some(from))
            .expect("FROM linted at its original position");
        let published = convert::diagnostic(&entry.index, diagnostic);
        assert_eq!(
            published.severity,
            Some(lsp_types::DiagnosticSeverity::INFORMATION)
        );
        assert_eq!(diagnostic.fixes[0].edits[0].range, from);
        assert_eq!(diagnostic.fixes[0].edits[0].new_text, "from");

        let id = source.find("id FROM").unwrap() as u32;
        let pos = entry.index.position(id);
        let hover = hover_at(entry, pos).expect("column hover after the placeholder");
        assert_eq!(
            hover.range,
            Some(entry.index.range(TextRange::at(id.into(), 2.into())))
        );
        Ok(())
    }

    #[test]
    fn formatting_reuses_the_current_revision_and_preserves_native_operators() -> anyhow::Result<()>
    {
        let config = BansheeConfig::from_toml(
            "[format]\nstyle = \"compact\"\n[templater]\nstyle = \"percent\"",
        )?;
        let mut server = Session::new(&ClientCapabilities::default(), fixed_settings(config, None));
        let uri: Uri = "file:///formatting.sql".parse()?;
        let native = "select  20%s from t;";
        server.update_doc(uri.clone(), 1, LineIndex::new(native))?;
        let entry = server.doc(&uri).expect("analyzed document");
        assert!(entry.templated.is_some());
        assert!(entry.native_parse.get().is_none());
        let expected = entry.settings.config.format_edits(native);
        assert_eq!(
            entry.format_edits(None),
            expected
                .iter()
                .map(|edit| entry.index.replacement(edit.range, &edit.new_text))
                .collect::<Vec<_>>()
        );
        let original = entry.native_parse.get().expect("cached native parse");
        assert!(original.errors().is_empty());
        assert!(!entry.format_edits(None).is_empty());
        assert!(std::ptr::eq(original, entry.native_parse.get().unwrap()));

        let template = "select  '😀', %s::int;\r\nselect (";
        server.update_doc(uri.clone(), 2, LineIndex::new(template))?;
        let entry = server.doc(&uri).expect("analyzed document");
        assert!(entry.native_parse.get().is_none());
        assert!(entry.format_edits(None).is_empty());
        let start = template.find('%').expect("placeholder") as u32;
        let selection = TextRange::at(start.into(), 2.into());
        let expected = entry
            .settings
            .config
            .format_range(template, start, start + 2);
        assert_eq!(expected.len(), 1);
        assert_eq!(
            entry.format_edits(Some(selection)),
            [entry
                .index
                .replacement(expected[0].range, &expected[0].new_text)]
        );
        assert!(
            !entry
                .native_parse
                .get()
                .expect("cached original parse")
                .errors()
                .is_empty()
        );

        server.update_doc(uri.clone(), 3, LineIndex::new("select  1"))?;
        let entry = server.doc(&uri).expect("analyzed document");
        assert!(entry.templated.is_none());
        assert!(!entry.format_edits(None).is_empty());
        assert!(entry.native_parse.get().is_none());
        Ok(())
    }

    #[test]
    fn diagnostics_and_tokens_track_document_revisions() -> anyhow::Result<()> {
        let mut server = Session::new(
            &ClientCapabilities::default(),
            fixed_settings(BansheeConfig::default(), None),
        );
        let uri = "file:///query.sql";
        let publish = |server: &mut Session, method: &str, params| {
            let result = server.handle_message(Message::Notification(Notification::new(
                method.into(),
                params,
            )))?;
            let Some(Message::Notification(note)) = result else {
                anyhow::bail!("expected a diagnostics notification");
            };
            Ok::<_, anyhow::Error>(
                note.extract::<PublishDiagnosticsParams>(PublishDiagnostics::METHOD)?,
            )
        };
        let opened = publish(
            &mut server,
            DidOpenTextDocument::METHOD,
            json!({"textDocument": {
                "uri": uri, "languageId": "sql", "version": 5, "text": "select ("
            }}),
        )?;
        assert_eq!(opened.version, Some(5));
        assert!(!opened.diagnostics.is_empty());

        let changed = publish(
            &mut server,
            DidChangeTextDocument::METHOD,
            json!({
                "textDocument": {"uri": uri, "version": 8},
                "contentChanges": [{"text": "\r\nSELECT 1;"}]
            }),
        )?;
        assert_eq!(changed.version, Some(8));
        assert!(changed.diagnostics.is_empty(), "{:?}", changed.diagnostics);

        let result = server.handle_message(Message::Request(Request::new(
            1.into(),
            SemanticTokensFullRequest::METHOD.into(),
            json!({"textDocument": {"uri": uri}}),
        )))?;
        let Some(Message::Response(response)) = result else {
            anyhow::bail!("expected a semantic tokens response");
        };
        assert_eq!(
            response.response_result.expect("semantic tokens result"),
            json!({"data": [1, 0, 6, 0, 0, 0, 7, 1, 3, 0, 0, 1, 1, 1, 0]}),
        );

        let changed = publish(
            &mut server,
            DidChangeTextDocument::METHOD,
            json!({
                "textDocument": {"uri": uri, "version": 9},
                "contentChanges": [
                    {"range": {"start": {"line": 0, "character": 0}, "end": {"line": 1, "character": 0}},
                     "text": "SELECT '😀';\r\n"},
                    {"range": {"start": {"line": 0, "character": 8}, "end": {"line": 0, "character": 10}},
                     "text": "é"},
                    {"range": {"start": {"line": 1, "character": 7}, "end": {"line": 1, "character": 8}},
                     "text": "2"}
                ]
            }),
        )?;
        assert_eq!(changed.version, Some(9));
        assert!(changed.diagnostics.is_empty());
        assert!(
            server
                .handle_message(change_message(uri, 8, json!([{"text": "SELECT 8;"}])))?
                .is_none()
        );
        assert_eq!(
            server
                .doc(&uri.parse()?)
                .expect("latest document")
                .index
                .text(),
            "SELECT 'é';\r\nSELECT 2;"
        );

        let closed = publish(
            &mut server,
            DidCloseTextDocument::METHOD,
            json!({"textDocument": {"uri": uri}}),
        )?;
        assert!(closed.diagnostics.is_empty());
        assert!(server.docs.is_empty());
        assert!(
            server
                .handle_message(change_message(uri, 10, json!([{"text": "SELECT 10;"}])))?
                .is_none()
        );
        assert!(server.docs.is_empty());
        Ok(())
    }

    fn open_message(uri: &str, version: i32, text: &str) -> Message {
        Notification::new(
            DidOpenTextDocument::METHOD.to_owned(),
            json!({"textDocument": {"uri": uri, "languageId": "sql", "version": version, "text": text}}),
        )
        .into()
    }

    fn change_message(uri: &str, version: i32, changes: serde_json::Value) -> Message {
        Notification::new(
            DidChangeTextDocument::METHOD.to_owned(),
            json!({"textDocument": {"uri": uri, "version": version}, "contentChanges": changes}),
        )
        .into()
    }

    #[test]
    fn invalid_incremental_batch_requires_a_new_snapshot_for_that_document() -> anyhow::Result<()> {
        let mut session = Session::new(
            &ClientCapabilities::default(),
            fixed_settings(BansheeConfig::default(), None),
        );
        let uri = "banshee://workspace/query";
        let other = "banshee://workspace/other";
        assert!(
            session
                .handle_message(open_message(uri, 1, "SELECT '😀';"))?
                .is_some()
        );
        assert!(
            session
                .handle_message(open_message(other, 1, "SELECT 2;"))?
                .is_some()
        );
        let invalid = change_message(
            uri,
            2,
            json!([
                {"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 6}}, "text": "select"},
                {"range": {"start": {"line": 0, "character": 9}, "end": {"line": 0, "character": 10}}, "text": "x"}
            ]),
        );
        let error = session
            .handle_message(invalid)
            .expect_err("split UTF-16 surrogate pair");
        assert!(error.to_string().contains("invalid UTF-16 change range"));
        assert!(session.doc(&uri.parse()?).is_none());
        assert!(session.doc(&other.parse()?).is_some());

        let relative = change_message(
            uri,
            3,
            json!([
                {"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}, "text": "-- inserted\n"}
            ]),
        );
        assert!(
            session
                .handle_message(relative)
                .expect_err("missing full snapshot")
                .to_string()
                .contains("full document")
        );
        let recovery = change_message(
            uri,
            4,
            json!([
                {"text": "SELECT 1;"},
                {"range": {"start": {"line": 0, "character": 7}, "end": {"line": 0, "character": 8}}, "text": "2"}
            ]),
        );
        assert!(session.handle_message(recovery)?.is_some());
        let entry = session.doc(&uri.parse()?).expect("resynchronized document");
        assert_eq!(entry.version, 4);
        assert_eq!(entry.index.text(), "SELECT 2;");
        Ok(())
    }

    #[test]
    fn settings_errors_keep_the_text_needed_for_future_range_updates() -> anyhow::Result<()> {
        let mut attempts = 0;
        let mut session = Session::new(&ClientCapabilities::default(), move |_| {
            attempts += 1;
            anyhow::ensure!(attempts != 2, "configuration temporarily unavailable");
            Ok(Arc::new(DocumentSettings::default()))
        });
        let uri = "banshee://workspace/query";
        assert!(
            session
                .handle_message(open_message(uri, 1, "SELECT 1;"))?
                .is_some()
        );
        let broken = change_message(
            uri,
            2,
            json!([
                {"range": {"start": {"line": 0, "character": 7}, "end": {"line": 0, "character": 8}}, "text": "100"}
            ]),
        );
        let Some(Message::Notification(note)) = session.handle_message(broken)? else {
            anyhow::bail!("expected a configuration diagnostic");
        };
        let diagnostics = note.extract::<PublishDiagnosticsParams>(PublishDiagnostics::METHOD)?;
        assert_eq!(diagnostics.version, Some(2));
        assert_eq!(diagnostics.diagnostics.len(), 1);
        assert!(
            diagnostics.diagnostics[0]
                .message
                .contains("configuration temporarily unavailable")
        );
        assert!(session.doc(&uri.parse()?).is_none());

        let corrected = change_message(
            uri,
            3,
            json!([
                {"range": {"start": {"line": 0, "character": 9}, "end": {"line": 0, "character": 10}}, "text": "2"}
            ]),
        );
        assert!(session.handle_message(corrected)?.is_some());
        let entry = session.doc(&uri.parse()?).expect("recovered document");
        assert_eq!(entry.version, 3);
        assert_eq!(entry.index.text(), "SELECT 102;");
        Ok(())
    }

    #[test]
    fn embedded_sessions_isolate_documents_and_server_supplied_schemas() -> anyhow::Result<()> {
        fn assert_send<T: Send>() {}
        assert_send::<Session>();

        let uri = "banshee://console/query";
        let settings = Arc::new(DocumentSettings {
            config: BansheeConfig::default(),
            schema: Some(Arc::new(provider())),
        });
        let mut first = Session::new(&ClientCapabilities::default(), move |document| {
            assert_eq!(document.as_str(), uri);
            Ok(Arc::clone(&settings))
        });
        let mut second = Session::new(
            &ClientCapabilities::default(),
            fixed_settings(BansheeConfig::default(), None),
        );
        for (session, version) in [(&mut first, 1), (&mut second, 4)] {
            let opened = session.handle_message(Message::Notification(Notification::new(
                DidOpenTextDocument::METHOD.to_owned(),
                json!({"textDocument": {
                    "uri": uri, "languageId": "sql", "version": version,
                    "text": "select id from patient"
                }}),
            )))?;
            let Some(Message::Notification(note)) = opened else {
                anyhow::bail!("expected diagnostics for an embedded document");
            };
            let diagnostics =
                note.extract::<PublishDiagnosticsParams>(PublishDiagnostics::METHOD)?;
            assert_eq!(diagnostics.version, Some(version));
        }
        let hover = || {
            Message::Request(Request::new(
                7.into(),
                HoverRequest::METHOD.to_owned(),
                json!({"textDocument": {"uri": uri}, "position": {"line": 0, "character": 7}}),
            ))
        };
        let Some(Message::Response(response)) = first.handle_message(hover())? else {
            anyhow::bail!("expected a hover response");
        };
        assert!(
            response.response_result.expect("hover result")["contents"]["value"]
                .as_str()
                .is_some_and(|hover| hover.contains("integer"))
        );
        let Some(Message::Response(response)) = second.handle_message(hover())? else {
            anyhow::bail!("expected a hover response for the second session");
        };
        assert_eq!(
            response.response_result.expect("empty hover response"),
            serde_json::Value::Null
        );

        let closed = second.handle_message(Message::Notification(Notification::new(
            DidCloseTextDocument::METHOD.to_owned(),
            json!({"textDocument": {"uri": uri}}),
        )))?;
        let Some(Message::Notification(note)) = closed else {
            anyhow::bail!("expected diagnostics to clear for the closed document");
        };
        let diagnostics = note.extract::<PublishDiagnosticsParams>(PublishDiagnostics::METHOD)?;
        assert!(diagnostics.diagnostics.is_empty());
        assert_eq!(diagnostics.uri.as_str(), uri);
        assert!(second.docs.is_empty());
        assert_eq!(
            first.doc(&uri.parse()?).expect("analyzed document").version,
            1
        );
        Ok(())
    }

    #[test]
    fn embedded_requests_report_errors_with_the_original_id() -> anyhow::Result<()> {
        let mut session = Session::new(
            &ClientCapabilities::default(),
            fixed_settings(BansheeConfig::default(), None),
        );
        for (method, code) in [
            (Formatting::METHOD, ErrorCode::InvalidParams),
            ("custom/unknown", ErrorCode::MethodNotFound),
        ] {
            let id = RequestId::from("client-request".to_owned());
            let result = session.handle_message(Message::Request(Request::new(
                id.clone(),
                method.to_owned(),
                json!({}),
            )))?;
            let Some(Message::Response(response)) = result else {
                anyhow::bail!("expected an error response for {method}");
            };
            assert_eq!(response.id, id);
            assert_eq!(
                response.response_result.expect_err("protocol error").code,
                code as i32
            );
        }
        Ok(())
    }
}
