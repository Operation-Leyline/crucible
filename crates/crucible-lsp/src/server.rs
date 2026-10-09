//! The language server: keeps open documents, compiles the package each one
//! belongs to on every change, and answers editor requests from the latest
//! compilation. `crucible.toml` manifests are checked too.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use crucible_lang::{
    Compiled, FormatOptions, Package, Parsed, SourceFile, check_manifest, compile, format,
    is_package, load_package, load_package_with_manifest,
};
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument,
    Notification as _, PublishDiagnostics,
};
use lsp_types::request::{
    CodeActionRequest, Completion, DocumentHighlightRequest, DocumentSymbolRequest,
    FoldingRangeRequest, Formatting, GotoDefinition, HoverRequest, PrepareRenameRequest,
    References, Rename, Request as _, SemanticTokensFullRequest, WorkspaceSymbolRequest,
};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionProviderCapability,
    CompletionOptions, CompletionResponse, Diagnostic, DiagnosticSeverity, DocumentHighlight,
    DocumentHighlightKind, DocumentSymbol, DocumentSymbolResponse, FoldingRangeProviderCapability,
    GotoDefinitionResponse, Hover, HoverContents, HoverProviderCapability, InitializeParams,
    Location, MarkupContent, MarkupKind, OneOf, Position, PrepareRenameResponse,
    PublishDiagnosticsParams, Range, RenameOptions, SemanticToken, SemanticTokenModifier,
    SemanticTokenType, SemanticTokens, SemanticTokensFullOptions, SemanticTokensLegend,
    SemanticTokensOptions, SemanticTokensResult, SemanticTokensServerCapabilities,
    ServerCapabilities, SymbolInformation, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, TextDocumentSyncSaveOptions, TextEdit, Url, WorkspaceEdit,
    WorkspaceSymbolResponse,
};
use serde_json::Value;

use crate::analysis::{self, Index, Symbol, TOKEN_MODIFIERS, TOKEN_TYPES};
use crate::lines::Lines;
use crate::{actions, completion, folding};

type Error = Box<dyn std::error::Error + Send + Sync>;
/// A request's result, or an LSP error code and message.
type Reply = Result<Value, (i32, String)>;

pub fn capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::FULL),
                save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                ..Default::default()
            },
        )),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                legend: SemanticTokensLegend {
                    token_types: TOKEN_TYPES
                        .iter()
                        .map(|name| SemanticTokenType::new(name))
                        .collect(),
                    token_modifiers: TOKEN_MODIFIERS
                        .iter()
                        .map(|name| SemanticTokenModifier::new(name))
                        .collect(),
                },
                full: Some(SemanticTokensFullOptions::Bool(true)),
                range: None,
                ..Default::default()
            },
        )),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec!["@".into(), ":".into(), "[".into()]),
            ..Default::default()
        }),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        document_highlight_provider: Some(OneOf::Left(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        ..Default::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Model,
    Manifest,
}

struct Document {
    text: String,
    /// The file on disk, when the URI names one.
    path: Option<PathBuf>,
    /// The package directory, or the file itself when it belongs to none. A
    /// manifest without `[model]` belongs to none.
    unit: Option<PathBuf>,
    kind: Kind,
}

/// The latest compilation of one package (or of one file outside any).
struct Unit {
    compiled: Compiled,
    index: Index,
    /// How many of `compiled.files` are the unit's own; the rest are imports.
    own: usize,
    /// A file outside any package: problems with the package itself, such
    /// as having no service, do not apply.
    standalone: bool,
    /// Why the package could not be read, if it could not.
    load_error: Option<String>,
}

pub struct Server {
    connection: Connection,
    snippets: bool,
    documents: HashMap<Url, Document>,
    units: HashMap<PathBuf, Unit>,
}

impl Server {
    pub fn new(connection: Connection, params: &InitializeParams) -> Self {
        let snippets = params
            .capabilities
            .text_document
            .as_ref()
            .and_then(|text| text.completion.as_ref())
            .and_then(|completion| completion.completion_item.as_ref())
            .and_then(|item| item.snippet_support)
            .unwrap_or(false);
        Self {
            connection,
            snippets,
            documents: HashMap::new(),
            units: HashMap::new(),
        }
    }

    pub fn run(mut self) -> Result<(), Error> {
        let mut next: Option<Message> = None;
        loop {
            let message = match next.take() {
                Some(message) => message,
                None => match self.connection.receiver.recv() {
                    Ok(message) => message,
                    Err(_) => break,
                },
            };
            match message {
                Message::Request(request) => {
                    if self.connection.handle_shutdown(&request)? {
                        return Ok(());
                    }
                    self.request(request)?;
                }
                Message::Notification(notification) => {
                    // Edits arrive in bursts while typing: apply every queued
                    // notification first, then compile each package once.
                    let mut dirty: Vec<PathBuf> = Vec::new();
                    let mut pending = Some(notification);
                    while let Some(notification) = pending.take() {
                        if let Some(unit) = self.notification(notification)?
                            && !dirty.contains(&unit)
                        {
                            dirty.push(unit);
                        }
                        match self.connection.receiver.try_recv() {
                            Ok(Message::Notification(queued)) => pending = Some(queued),
                            Ok(other) => next = Some(other),
                            Err(_) => {}
                        }
                    }
                    for unit in dirty {
                        self.recompile(&unit)?;
                    }
                }
                Message::Response(_) => {}
            }
        }
        Ok(())
    }

    fn send(&self, message: Message) -> Result<(), Error> {
        self.connection.sender.send(message)?;
        Ok(())
    }

    // ---- documents ---------------------------------------------------------

    /// Applies one notification to the open documents. Returns the package to
    /// recompile, if any; the caller compiles once per burst of edits.
    fn notification(&mut self, notification: Notification) -> Result<Option<PathBuf>, Error> {
        let uri = match notification.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let params: lsp_types::DidOpenTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                let document = params.text_document;
                let path = document.uri.to_file_path().ok();
                let kind = if path
                    .as_deref()
                    .and_then(Path::file_name)
                    .is_some_and(|name| name == "crucible.toml")
                {
                    Kind::Manifest
                } else {
                    Kind::Model
                };
                let unit = unit_of(kind, path.as_deref(), &document.uri, &document.text);
                self.documents.insert(
                    document.uri.clone(),
                    Document {
                        text: document.text,
                        path,
                        unit,
                        kind,
                    },
                );
                document.uri
            }
            DidChangeTextDocument::METHOD => {
                let params: lsp_types::DidChangeTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                let Some(document) = self.documents.get_mut(&params.text_document.uri) else {
                    return Ok(None);
                };
                // Full synchronisation: the last change is the whole text.
                if let Some(change) = params.content_changes.into_iter().last() {
                    document.text = change.text;
                }
                if document.kind == Kind::Manifest {
                    document.unit = unit_of(
                        Kind::Manifest,
                        document.path.as_deref(),
                        &params.text_document.uri,
                        &document.text,
                    );
                }
                params.text_document.uri
            }
            DidSaveTextDocument::METHOD => {
                let params: lsp_types::DidSaveTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                params.text_document.uri
            }
            DidCloseTextDocument::METHOD => {
                let params: lsp_types::DidCloseTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                let Some(document) = self.documents.remove(&params.text_document.uri) else {
                    return Ok(None);
                };
                self.publish(params.text_document.uri, Vec::new())?;
                return Ok(document.unit);
            }
            _ => return Ok(None),
        };
        let Some(document) = self.documents.get(&uri) else {
            return Ok(None);
        };
        match document.unit.clone() {
            Some(unit) => Ok(Some(unit)),
            // A policy-only manifest: its syntax is all there is to check.
            None => {
                let diagnostics = self.manifest_diagnostics(&uri);
                self.publish(uri, diagnostics)?;
                Ok(None)
            }
        }
    }

    fn open_in(&self, unit: &Path) -> Vec<Url> {
        self.documents
            .iter()
            .filter(|(_, document)| document.unit.as_deref() == Some(unit))
            .map(|(uri, _)| uri.clone())
            .collect()
    }

    fn recompile(&mut self, unit: &Path) -> Result<(), Error> {
        let open = self.open_in(unit);
        if !open
            .iter()
            .any(|uri| self.documents[uri].kind == Kind::Model)
        {
            self.units.remove(unit);
        } else {
            let compiled = self.compile_unit(unit, &open);
            self.units.insert(unit.to_path_buf(), compiled);
        }
        for uri in open {
            let diagnostics = match self.documents[&uri].kind {
                Kind::Model => self.diagnostics(&uri),
                Kind::Manifest => self.manifest_diagnostics(&uri),
            };
            self.publish(uri, diagnostics)?;
        }
        Ok(())
    }

    fn compile_unit(&self, unit: &Path, open: &[Url]) -> Unit {
        let standalone = !unit.is_dir();
        let models: Vec<&Url> = open
            .iter()
            .filter(|uri| self.documents[*uri].kind == Kind::Model)
            .collect();
        let manifest = open
            .iter()
            .find(|uri| self.documents[*uri].kind == Kind::Manifest)
            .map(|uri| self.documents[uri].text.as_str());
        let loaded = if standalone {
            let document = &self.documents[models[0]];
            Ok(Package::new(
                "local",
                vec![SourceFile {
                    path: unit.display().to_string(),
                    text: document.text.clone(),
                }],
            ))
        } else {
            match manifest {
                Some(text) => load_package_with_manifest(unit, text),
                None => load_package(unit),
            }
        };
        let (mut package, load_error) = match loaded {
            Ok(package) => (package, None),
            // Still check the open files on their own, so a broken manifest
            // does not hide every other problem.
            Err(error) => {
                let files = models
                    .iter()
                    .map(|uri| {
                        let document = &self.documents[*uri];
                        SourceFile {
                            path: document
                                .path
                                .as_ref()
                                .map_or_else(|| uri.to_string(), |path| path.display().to_string()),
                            text: document.text.clone(),
                        }
                    })
                    .collect();
                (Package::new("local", files), Some(error))
            }
        };
        // Unsaved editor contents win over the files on disk.
        for file in &mut package.files {
            let key = normalize(Path::new(&file.path));
            if let Some(document) =
                models
                    .iter()
                    .map(|uri| &self.documents[*uri])
                    .find(|document| {
                        document
                            .path
                            .as_deref()
                            .is_some_and(|path| normalize(path) == key)
                    })
            {
                file.text.clone_from(&document.text);
            }
        }
        let own = package.files.len();
        let compiled = compile(&package, None);
        let index = Index::new(&compiled.parsed);
        Unit {
            compiled,
            index,
            own,
            standalone: standalone || load_error.is_some(),
            load_error,
        }
    }

    /// The file index of an open document within its unit's compilation.
    fn file_of(&self, uri: &Url) -> Option<(&Unit, Option<usize>)> {
        let document = self.documents.get(uri)?;
        let unit = self.units.get(document.unit.as_ref()?)?;
        let index = match document.path.as_deref() {
            Some(path) => {
                let key = normalize(path);
                unit.compiled.files[..unit.own]
                    .iter()
                    .position(|file| normalize(Path::new(&file.path)) == key)
            }
            // An untitled buffer is the only file of its standalone unit.
            None if unit.standalone => Some(0),
            None => None,
        };
        Some((unit, index))
    }

    fn is_open(&self, path: &str) -> bool {
        let key = normalize(Path::new(path));
        self.documents
            .values()
            .any(|open| open.path.as_deref().map(normalize) == Some(key.clone()))
    }

    /// The URI of a file in a compilation: the open document's own URI when it
    /// is open, so the editor recognises it.
    fn uri_of(&self, unit: &Unit, file: usize, fallback: &Url) -> Option<Url> {
        let path = &unit.compiled.files[file].path;
        if unit.standalone && file == 0 && !self.is_open(path) {
            return Some(fallback.clone());
        }
        let key = normalize(Path::new(path));
        if let Some(uri) = self.documents.iter().find_map(|(uri, document)| {
            (document.path.as_deref().map(normalize) == Some(key.clone())).then(|| uri.clone())
        }) {
            return Some(uri);
        }
        Url::from_file_path(clean(Path::new(path))).ok()
    }

    fn diagnostics(&self, uri: &Url) -> Vec<Diagnostic> {
        let Some(document) = self.documents.get(uri) else {
            return Vec::new();
        };
        let Some((unit, own_index)) = self.file_of(uri) else {
            return Vec::new();
        };
        let lines = Lines::new(&document.text);
        let top = lines.first_line();
        let mut out = Vec::new();
        if let Some(load_error) = &unit.load_error {
            out.push(error(top, load_error.clone()));
        }
        for item in &unit.compiled.diagnostics {
            let diagnostic = &item.diagnostic;
            match item.file {
                Some(file) if Some(file) == own_index => out.push(at_severity(
                    lines.range(diagnostic.span.start, diagnostic.span.end),
                    diagnostic.message.clone(),
                    diagnostic.severity,
                )),
                // A problem in another file of the package (an import
                // included) still fails the build: show where it is, unless
                // that file is open and shows it itself.
                Some(file) => {
                    let other = &unit.compiled.files[file];
                    // Only errors elsewhere fail the build; warnings show in
                    // their own file when it is open.
                    if !diagnostic.is_error() || (file < unit.own && self.is_open(&other.path)) {
                        continue;
                    }
                    let (line, column) = crucible_lang::LineIndex::new(&other.text)
                        .line_col(&other.text, diagnostic.span.start);
                    let name = Path::new(&other.path)
                        .file_name()
                        .map_or(other.path.clone(), |name| {
                            name.to_string_lossy().into_owned()
                        });
                    out.push(error(
                        top,
                        format!("{name}:{line}:{column}: {}", diagnostic.message),
                    ));
                }
                None if unit.standalone => {}
                None => out.push(at_severity(
                    top,
                    diagnostic.message.clone(),
                    diagnostic.severity,
                )),
            }
        }
        out
    }

    fn manifest_diagnostics(&self, uri: &Url) -> Vec<Diagnostic> {
        let Some(document) = self.documents.get(uri) else {
            return Vec::new();
        };
        let Some(dir) = document.path.as_deref().and_then(Path::parent) else {
            return Vec::new();
        };
        let lines = Lines::new(&document.text);
        check_manifest(dir, &document.text)
            .into_iter()
            .map(|diagnostic| {
                at_severity(
                    lines.range(diagnostic.span.start, diagnostic.span.end),
                    diagnostic.message,
                    diagnostic.severity,
                )
            })
            .collect()
    }

    fn publish(&self, uri: Url, diagnostics: Vec<Diagnostic>) -> Result<(), Error> {
        let params = PublishDiagnosticsParams {
            uri,
            diagnostics,
            version: None,
        };
        self.send(Message::Notification(Notification::new(
            PublishDiagnostics::METHOD.to_owned(),
            params,
        )))
    }

    // ---- requests ----------------------------------------------------------

    fn request(&self, request: Request) -> Result<(), Error> {
        let id = request.id.clone();
        let reply = self.dispatch(request);
        let response = match reply {
            Ok(result) => Response::new_ok(id, result),
            Err((code, message)) => Response::new_err(id, code, message),
        };
        self.send(Message::Response(response))
    }

    fn dispatch(&self, request: Request) -> Reply {
        fn params<P: serde::de::DeserializeOwned>(value: Value) -> Result<P, (i32, String)> {
            serde_json::from_value(value)
                .map_err(|error| (ErrorCode::InvalidParams as i32, error.to_string()))
        }
        fn json(value: impl serde::Serialize) -> Reply {
            serde_json::to_value(value)
                .map_err(|error| (ErrorCode::InternalError as i32, error.to_string()))
        }
        match request.method.as_str() {
            SemanticTokensFullRequest::METHOD => {
                let p: lsp_types::SemanticTokensParams = params(request.params)?;
                json(self.semantic_tokens(&p.text_document.uri))
            }
            Completion::METHOD => {
                let p: lsp_types::CompletionParams = params(request.params)?;
                let at = p.text_document_position;
                json(self.completion(&at.text_document.uri, at.position))
            }
            GotoDefinition::METHOD => {
                let p: lsp_types::GotoDefinitionParams = params(request.params)?;
                let at = p.text_document_position_params;
                json(self.definition(&at.text_document.uri, at.position))
            }
            References::METHOD => {
                let p: lsp_types::ReferenceParams = params(request.params)?;
                let at = p.text_document_position;
                json(self.references(
                    &at.text_document.uri,
                    at.position,
                    p.context.include_declaration,
                ))
            }
            PrepareRenameRequest::METHOD => {
                let p: lsp_types::TextDocumentPositionParams = params(request.params)?;
                json(self.prepare_rename(&p.text_document.uri, p.position))
            }
            Rename::METHOD => {
                let p: lsp_types::RenameParams = params(request.params)?;
                let at = p.text_document_position;
                json(self.rename(&at.text_document.uri, at.position, &p.new_name)?)
            }
            DocumentHighlightRequest::METHOD => {
                let p: lsp_types::DocumentHighlightParams = params(request.params)?;
                let at = p.text_document_position_params;
                json(self.highlights(&at.text_document.uri, at.position))
            }
            HoverRequest::METHOD => {
                let p: lsp_types::HoverParams = params(request.params)?;
                let at = p.text_document_position_params;
                json(self.hover(&at.text_document.uri, at.position))
            }
            DocumentSymbolRequest::METHOD => {
                let p: lsp_types::DocumentSymbolParams = params(request.params)?;
                json(self.symbols(&p.text_document.uri))
            }
            WorkspaceSymbolRequest::METHOD => {
                let p: lsp_types::WorkspaceSymbolParams = params(request.params)?;
                json(self.workspace_symbols(&p.query))
            }
            FoldingRangeRequest::METHOD => {
                let p: lsp_types::FoldingRangeParams = params(request.params)?;
                json(self.folding(&p.text_document.uri))
            }
            Formatting::METHOD => {
                let p: lsp_types::DocumentFormattingParams = params(request.params)?;
                json(self.formatting(&p.text_document.uri, &p.options))
            }
            CodeActionRequest::METHOD => {
                let p: lsp_types::CodeActionParams = params(request.params)?;
                json(self.code_actions(&p))
            }
            _ => Err((
                ErrorCode::MethodNotFound as i32,
                format!("unsupported request {}", request.method),
            )),
        }
    }

    /// A model document's text, parse and its unit's index (empty when the
    /// package has not compiled yet).
    fn model(&self, uri: &Url) -> Option<(&Document, Parsed, Option<&Unit>)> {
        let document = self.documents.get(uri)?;
        if document.kind != Kind::Model {
            return None;
        }
        let unit = document.unit.as_ref().and_then(|unit| self.units.get(unit));
        Some((document, crucible_lang::parse(&document.text), unit))
    }

    fn semantic_tokens(&self, uri: &Url) -> Option<SemanticTokensResult> {
        let (document, parsed, unit) = self.model(uri)?;
        let empty = Index::default();
        let index = unit.map_or(&empty, |unit| &unit.index);
        let lines = Lines::new(&document.text);
        let mut data = Vec::new();
        let (mut last_line, mut last_start) = (0, 0);
        for highlight in analysis::highlights(&parsed, index) {
            // A token may not span lines in LSP; split it at each line end.
            let text = &document.text[highlight.start as usize..highlight.end as usize];
            let mut offset = highlight.start as usize;
            for piece in text.split('\n') {
                let trimmed = piece.trim_end_matches('\r');
                let length: usize = trimmed.chars().map(char::len_utf16).sum();
                if length > 0 {
                    let position = lines.position(offset);
                    let delta_line = position.line - last_line;
                    let delta_start = if delta_line == 0 {
                        position.character - last_start
                    } else {
                        position.character
                    };
                    data.push(SemanticToken {
                        delta_line,
                        delta_start,
                        length: length as u32,
                        token_type: highlight.role as u32,
                        token_modifiers_bitset: highlight.modifiers,
                    });
                    last_line = position.line;
                    last_start = position.character;
                }
                offset += piece.len() + 1;
            }
        }
        Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data,
        }))
    }

    fn completion(&self, uri: &Url, position: Position) -> Option<CompletionResponse> {
        let (document, parsed, unit) = self.model(uri)?;
        let offset = Lines::new(&document.text).offset(position);
        let context = completion::context(&parsed, &document.text, offset);
        let empty = Index::default();
        let index = unit.map_or(&empty, |unit| &unit.index);
        Some(CompletionResponse::Array(completion::complete(
            context,
            index,
            self.snippets,
        )))
    }

    /// The declaration the name under the caret refers to, with the name's
    /// range in the document.
    fn declaration_at(
        &self,
        uri: &Url,
        position: Position,
    ) -> Option<(&Unit, String, &analysis::Declaration, Range)> {
        let (document, parsed, unit) = self.model(uri)?;
        let unit = unit?;
        let lines = Lines::new(&document.text);
        let (start, end, name) =
            analysis::ident_at(&parsed, &document.text, lines.offset(position))?;
        let declaration = unit.index.declarations.get(&name)?;
        // Only a name used as a reference or declaration counts: `id` the
        // keyword does not refer to a type that happens to be called `id`.
        let refers = analysis::occurrences(&parsed, &document.text, &unit.index, &name)
            .iter()
            .any(|(s, _, _)| *s == start);
        refers.then(|| (unit, name, declaration, lines.range(start, end)))
    }

    fn definition(&self, uri: &Url, position: Position) -> Option<GotoDefinitionResponse> {
        let (unit, _, declaration, _) = self.declaration_at(uri, position)?;
        if declaration.file >= unit.own {
            // A bundled import has no file to open; hover shows its text.
            return None;
        }
        let target = self.uri_of(unit, declaration.file, uri)?;
        let lines = Lines::new(&unit.compiled.files[declaration.file].text);
        Some(GotoDefinitionResponse::Scalar(Location {
            uri: target,
            range: lines.range(declaration.name_start, declaration.name_end),
        }))
    }

    /// Every occurrence of `name` across the unit's own files.
    fn occurrences(&self, unit: &Unit, name: &str, fallback: &Url) -> Vec<(Url, Range, bool)> {
        let mut out = Vec::new();
        for file in 0..unit.own {
            let source = &unit.compiled.files[file];
            let found =
                analysis::occurrences(&unit.compiled.parsed[file], &source.text, &unit.index, name);
            if found.is_empty() {
                continue;
            }
            let Some(uri) = self.uri_of(unit, file, fallback) else {
                continue;
            };
            let lines = Lines::new(&source.text);
            for (start, end, declaration) in found {
                out.push((uri.clone(), lines.range(start, end), declaration));
            }
        }
        out
    }

    fn references(
        &self,
        uri: &Url,
        position: Position,
        declaration: bool,
    ) -> Option<Vec<Location>> {
        let (unit, name, _, _) = self.declaration_at(uri, position)?;
        Some(
            self.occurrences(unit, &name, uri)
                .into_iter()
                .filter(|(_, _, is_declaration)| declaration || !is_declaration)
                .map(|(uri, range, _)| Location { uri, range })
                .collect(),
        )
    }

    fn prepare_rename(&self, uri: &Url, position: Position) -> Option<PrepareRenameResponse> {
        let (unit, name, declaration, range) = self.declaration_at(uri, position)?;
        // Imported and built-in declarations are not the package's to rename.
        (declaration.file < unit.own).then_some(PrepareRenameResponse::RangeWithPlaceholder {
            range,
            placeholder: name,
        })
    }

    fn rename(
        &self,
        uri: &Url,
        position: Position,
        new_name: &str,
    ) -> Result<Option<WorkspaceEdit>, (i32, String)> {
        let failed = |message: String| (ErrorCode::RequestFailed as i32, message);
        let Some((unit, name, declaration, _)) = self.declaration_at(uri, position) else {
            return Ok(None);
        };
        if declaration.file >= unit.own {
            return Err(failed(format!(
                "`{name}` is declared in an imported package"
            )));
        }
        let valid = new_name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && new_name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid {
            return Err(failed(format!("`{new_name}` is not a valid name")));
        }
        if new_name != name && unit.index.declarations.contains_key(new_name) {
            return Err(failed(format!("`{new_name}` is already declared")));
        }
        let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
        for (uri, range, _) in self.occurrences(unit, &name, uri) {
            changes.entry(uri).or_default().push(TextEdit {
                range,
                new_text: new_name.to_owned(),
            });
        }
        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }))
    }

    fn highlights(&self, uri: &Url, position: Position) -> Option<Vec<DocumentHighlight>> {
        let (unit, name, _, _) = self.declaration_at(uri, position)?;
        let (document, parsed, _) = self.model(uri)?;
        let lines = Lines::new(&document.text);
        Some(
            analysis::occurrences(&parsed, &document.text, &unit.index, &name)
                .into_iter()
                .map(|(start, end, declaration)| DocumentHighlight {
                    range: lines.range(start, end),
                    kind: Some(if declaration {
                        DocumentHighlightKind::WRITE
                    } else {
                        DocumentHighlightKind::READ
                    }),
                })
                .collect(),
        )
    }

    fn hover(&self, uri: &Url, position: Position) -> Option<Hover> {
        let (_, name, declaration, range) = self.declaration_at(uri, position)?;
        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: analysis::hover_markdown(&name, declaration),
            }),
            range: Some(range),
        })
    }

    fn symbols(&self, uri: &Url) -> Option<DocumentSymbolResponse> {
        let (document, parsed, _) = self.model(uri)?;
        let lines = Lines::new(&document.text);
        Some(DocumentSymbolResponse::Nested(
            analysis::outline(&parsed)
                .into_iter()
                .map(|symbol| to_lsp(symbol, &lines))
                .collect(),
        ))
    }

    #[allow(deprecated)] // `SymbolInformation::deprecated` must still be written.
    fn workspace_symbols(&self, query: &str) -> Option<WorkspaceSymbolResponse> {
        let query = query.to_lowercase();
        let matches = |name: &str| {
            let mut wanted = query.chars().peekable();
            for c in name.to_lowercase().chars() {
                if wanted.peek() == Some(&c) {
                    wanted.next();
                }
            }
            wanted.peek().is_none()
        };
        let mut out = Vec::new();
        for unit in self.units.values() {
            let fallback = self
                .documents
                .iter()
                .find(|(_, document)| {
                    document.unit.as_ref().is_some_and(|key| {
                        self.units.get(key).is_some_and(|u| std::ptr::eq(u, unit))
                    })
                })
                .map(|(uri, _)| uri.clone());
            let Some(fallback) = fallback else { continue };
            for file in 0..unit.own {
                let source = &unit.compiled.files[file];
                let lines = Lines::new(&source.text);
                let Some(uri) = self.uri_of(unit, file, &fallback) else {
                    continue;
                };
                for symbol in analysis::outline(&unit.compiled.parsed[file]) {
                    if matches(&symbol.name) {
                        out.push(SymbolInformation {
                            name: symbol.name,
                            kind: symbol.kind,
                            tags: None,
                            deprecated: None,
                            location: Location {
                                uri: uri.clone(),
                                range: lines.range(symbol.name_start, symbol.name_end),
                            },
                            container_name: Some(unit.compiled.namespace.clone()),
                        });
                    }
                }
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Some(WorkspaceSymbolResponse::Flat(out))
    }

    fn folding(&self, uri: &Url) -> Option<Vec<lsp_types::FoldingRange>> {
        let (document, parsed, _) = self.model(uri)?;
        Some(folding::folding_ranges(
            &parsed,
            &Lines::new(&document.text),
        ))
    }

    fn formatting(
        &self,
        uri: &Url,
        options: &lsp_types::FormattingOptions,
    ) -> Option<Vec<TextEdit>> {
        let (document, _, _) = self.model(uri)?;
        let indent = if options.insert_spaces {
            " ".repeat(options.tab_size.clamp(1, 16) as usize)
        } else {
            "\t".to_owned()
        };
        let formatted = format(&document.text, &FormatOptions { indent })?;
        if formatted == document.text {
            return Some(Vec::new());
        }
        let lines = Lines::new(&document.text);
        Some(vec![TextEdit {
            range: lines.range(0, document.text.len() as u32),
            new_text: formatted,
        }])
    }

    fn code_actions(
        &self,
        params: &lsp_types::CodeActionParams,
    ) -> Option<Vec<CodeActionOrCommand>> {
        let uri = &params.text_document.uri;
        let (document, _, unit) = self.model(uri)?;
        let empty = Index::default();
        let index = unit.map_or(&empty, |unit| &unit.index);
        let lines = Lines::new(&document.text);
        let mut out = Vec::new();
        for diagnostic in &params.context.diagnostics {
            let Some(name) = actions::unknown_type(&diagnostic.message) else {
                continue;
            };
            let edit = |range: Range, text: String| WorkspaceEdit {
                changes: Some(HashMap::from([(
                    uri.clone(),
                    vec![TextEdit {
                        range,
                        new_text: text,
                    }],
                )])),
                ..Default::default()
            };
            for (rank, candidate) in actions::similar_types(name, index).into_iter().enumerate() {
                out.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: format!("Change to `{candidate}`"),
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![diagnostic.clone()]),
                    edit: Some(edit(diagnostic.range, candidate)),
                    is_preferred: Some(rank == 0),
                    ..Default::default()
                }));
            }
            let end = lines.position(document.text.len());
            let separator = if document.text.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            };
            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: format!("Declare structure `{name}`"),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![diagnostic.clone()]),
                edit: Some(edit(
                    Range::new(end, end),
                    format!("{separator}structure {name} {{\n}}\n"),
                )),
                ..Default::default()
            }));
        }
        Some(out)
    }
}

fn at_severity(range: Range, message: String, severity: crucible_lang::Severity) -> Diagnostic {
    Diagnostic {
        severity: Some(match severity {
            crucible_lang::Severity::Error => DiagnosticSeverity::ERROR,
            crucible_lang::Severity::Warning => DiagnosticSeverity::WARNING,
        }),
        ..error(range, message)
    }
}

fn error(range: Range, message: String) -> Diagnostic {
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("crucible".into()),
        message,
        ..Default::default()
    }
}

#[allow(deprecated)] // `DocumentSymbol::deprecated` must still be written.
fn to_lsp(symbol: Symbol, lines: &Lines) -> DocumentSymbol {
    DocumentSymbol {
        name: symbol.name,
        detail: (!symbol.detail.is_empty()).then_some(symbol.detail),
        kind: symbol.kind,
        tags: None,
        deprecated: None,
        range: lines.range(symbol.start, symbol.end),
        selection_range: lines.range(symbol.name_start, symbol.name_end),
        children: Some(
            symbol
                .children
                .into_iter()
                .map(|child| to_lsp(child, lines))
                .collect(),
        ),
    }
}

/// The package directory a document belongs to: for a model file, the
/// nearest enclosing package, or the file itself; for a manifest, its
/// directory when it declares `[model]`.
fn unit_of(kind: Kind, path: Option<&Path>, uri: &Url, text: &str) -> Option<PathBuf> {
    match kind {
        Kind::Manifest => {
            let declares_model = text
                .lines()
                .any(|line| line.trim_start().starts_with("[model]"));
            declares_model
                .then(|| path.and_then(Path::parent).map(normalize))
                .flatten()
        }
        Kind::Model => Some(match path {
            None => PathBuf::from(uri.as_str()),
            Some(path) => path
                .ancestors()
                .skip(1)
                .find(|dir| is_package(dir))
                .map_or_else(|| normalize(path), normalize),
        }),
    }
}

/// `path` without `.` components.
fn clean(path: &Path) -> PathBuf {
    path.components()
        .filter(|component| !matches!(component, Component::CurDir))
        .collect()
}

/// One spelling per file for comparisons: no `.` components, and on Windows
/// one case and separator, so editor URIs and paths read from disk match.
fn normalize(path: &Path) -> PathBuf {
    let clean = clean(path);
    if cfg!(windows) {
        PathBuf::from(clean.to_string_lossy().replace('/', "\\").to_lowercase())
    } else {
        clean
    }
}
