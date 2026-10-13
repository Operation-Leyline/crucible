//! The language server: keeps open documents, compiles the package each one
//! belongs to on every change, and answers editor requests from the latest
//! compilation. `crucible.toml` manifests are checked too.

use std::collections::{HashMap, VecDeque};
use std::path::{Component, Path, PathBuf};

use crossbeam_channel::{Receiver, TryRecvError};
use crucible_lang::{
    Compiled, FormatOptions, Package, Parsed, SourceFile, check_manifest, compile, format,
    format_lines, is_package,
};
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidChangeWatchedFiles, DidCloseTextDocument, DidOpenTextDocument,
    DidSaveTextDocument, Notification as _, PublishDiagnostics,
};
use lsp_types::request::{
    CodeActionRequest, CodeLensRequest, Completion, DocumentHighlightRequest, DocumentLinkRequest,
    DocumentSymbolRequest, FoldingRangeRequest, Formatting, GotoDefinition, GotoTypeDefinition,
    HoverRequest, InlayHintRequest, PrepareRenameRequest, RangeFormatting, References,
    RegisterCapability, Rename, Request as _, SemanticTokensFullRequest, SignatureHelpRequest,
    WorkspaceSymbolRequest,
};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionProviderCapability, CodeLens,
    CodeLensOptions, Command, CompletionOptions, CompletionResponse, Diagnostic,
    DiagnosticSeverity, DidChangeWatchedFilesRegistrationOptions, DocumentHighlight,
    DocumentHighlightKind, DocumentLink, DocumentLinkOptions, DocumentSymbol,
    DocumentSymbolResponse, FileChangeType, FileEvent, FileSystemWatcher,
    FoldingRangeProviderCapability, GlobPattern, GotoDefinitionResponse, Hover, HoverContents,
    HoverProviderCapability, InitializeParams, InlayHint, InlayHintKind, InlayHintLabel, Location,
    MarkupContent, MarkupKind, OneOf, ParameterInformation, ParameterLabel, Position,
    PrepareRenameResponse, PublishDiagnosticsParams, Range, Registration, RegistrationParams,
    RenameOptions, SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens,
    SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensOptions, SemanticTokensResult,
    SemanticTokensServerCapabilities, ServerCapabilities, SignatureHelp, SignatureHelpOptions,
    SignatureInformation, SymbolInformation, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, TextDocumentSyncSaveOptions, TextEdit,
    TypeDefinitionProviderCapability, Url, WatchKind, WorkspaceEdit, WorkspaceSymbolResponse,
};
use serde_json::Value;

use crate::analysis::{self, Index, Symbol, TOKEN_MODIFIERS, TOKEN_TYPES};
use crate::insight::{self, HintKind};
use crate::lines::Lines;
use crate::{actions, completion, folding, hover, manifest, policy};

type Error = Box<dyn std::error::Error + Send + Sync>;
/// A request's result, or an LSP error code and message.
type Reply = Result<Value, (i32, String)>;

/// Whether the client said it runs [`SHOW_REFERENCES`]: initialization
/// options `{ "showReferences": true }`. A usage lens it cannot run would
/// fail when clicked, so other clients get none.
pub fn shows_references(params: &InitializeParams) -> bool {
    params
        .initialization_options
        .as_ref()
        .and_then(|options| options.get("showReferences"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub fn capabilities(params: &InitializeParams) -> ServerCapabilities {
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
            trigger_characters: Some(vec![
                "@".into(),
                ":".into(),
                "[".into(),
                "\"".into(),
                "{".into(),
                "$".into(),
            ]),
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
        document_range_formatting_provider: Some(OneOf::Left(true)),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true)),
        inlay_hint_provider: Some(OneOf::Left(true)),
        document_link_provider: Some(DocumentLinkOptions {
            resolve_provider: Some(false),
            work_done_progress_options: Default::default(),
        }),
        code_lens_provider: shows_references(params).then_some(CodeLensOptions {
            resolve_provider: Some(false),
        }),
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".into(), ",".into()]),
            retrigger_characters: None,
            work_done_progress_options: Default::default(),
        }),
        ..Default::default()
    }
}

/// The command a usage-count code lens carries: show the references of the
/// declaration at `[uri, position]`. It runs in the editor, which knows how
/// to show usages; the server only counts them.
pub const SHOW_REFERENCES: &str = "crucible.showReferences";

/// Directories never searched for packages: build output, dependencies and
/// version control hold copies, not the user's models.
const SKIPPED_DIRS: &[&str] = &["target", "node_modules", "build", "out", "dist"];

/// Whether discovery skips a directory with this name. Discovery and the
/// file watcher ask the same question about the same spelling, so a package
/// discovery finds is one whose changes are seen.
fn skipped(name: &str) -> bool {
    name.starts_with('.') || SKIPPED_DIRS.contains(&name)
}

/// How deep below a workspace folder packages are looked for, and how many
/// directories at most in each folder: a folder as large as a home
/// directory is searched in bounded time, in the background.
const DISCOVERY_DEPTH: usize = 12;
const DISCOVERY_BUDGET: usize = 20_000;

/// Walks `dir` for packages, calling `found` for each; stops when `found`
/// returns false (nobody is listening) or the budget of directories is spent.
fn discover(
    dir: &Path,
    depth: usize,
    budget: &mut usize,
    found: &mut dyn FnMut(&Path) -> bool,
) -> bool {
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    if is_package(dir) && !found(dir) {
        return false;
    }
    if depth == 0 {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return true;
    };
    let mut children: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| !skipped(name))
        })
        .collect();
    children.sort();
    children
        .iter()
        .all(|child| discover(child, depth - 1, budget, found))
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
    /// How often each declaration is referred to in the unit's own files.
    usages: HashMap<String, usize>,
    /// The package policy's findings, at the names they concern, and the
    /// breaking changes against its baseline.
    policy: Vec<policy::Finding>,
    /// Baseline problems with no place in the sources: shown on the
    /// manifest's `[model.baseline]`.
    baseline_notes: Vec<(bool, String)>,
}

pub struct Server {
    connection: Connection,
    snippets: bool,
    /// The client watches files for us and reports changes made outside the
    /// editor: a checkout, a generator, a file created in a terminal.
    watch: bool,
    /// The workspace folders, searched for packages at startup.
    roots: Vec<PathBuf>,
    /// The packages found under `roots`. They stay compiled with no file
    /// open, so references, rename and workspace symbols see all of them.
    packages: Vec<PathBuf>,
    /// Packages found by the background search, still to compile: the
    /// editor's requests are answered first, so a large workspace never
    /// holds them up.
    queue: VecDeque<PathBuf>,
    /// The background search's results; `None` once it has finished.
    discovery: Option<Receiver<PathBuf>>,
    /// A unit key's spelling on disk. Keys are normalized (lowercase on
    /// Windows) for comparison; locations sent to the editor use this.
    spelling: HashMap<PathBuf, PathBuf>,
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
        let watch = params
            .capabilities
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.did_change_watched_files.as_ref())
            .and_then(|watched| watched.dynamic_registration)
            .unwrap_or(false);
        #[allow(deprecated)] // `root_uri` is how older clients name the workspace.
        let roots = match &params.workspace_folders {
            Some(folders) => folders
                .iter()
                .filter_map(|folder| folder.uri.to_file_path().ok())
                .collect(),
            None => params
                .root_uri
                .iter()
                .filter_map(|uri| uri.to_file_path().ok())
                .collect(),
        };
        Self {
            connection,
            snippets,
            watch,
            roots,
            packages: Vec::new(),
            queue: VecDeque::new(),
            discovery: None,
            spelling: HashMap::new(),
            documents: HashMap::new(),
            units: HashMap::new(),
        }
    }

    /// Asks the client to report changes to model files and manifests, and
    /// compiles every package in the workspace.
    fn start(&mut self) -> Result<(), Error> {
        if self.watch {
            // Releases too: freezing or switching branches changes the
            // baseline a package is compared against.
            let watchers = [
                "**/*.cru",
                "**/crucible.toml",
                "**/releases/*.json",
                "**/crucible.lock",
            ]
            .into_iter()
            .map(|pattern| FileSystemWatcher {
                glob_pattern: GlobPattern::String(pattern.to_owned()),
                kind: None,
            })
            // A deleted folder is reported as the folder, which the globs
            // above do not match: deletions of anything are wanted.
            .chain(std::iter::once(FileSystemWatcher {
                glob_pattern: GlobPattern::String("**/*".to_owned()),
                kind: Some(WatchKind::Delete),
            }))
            .collect::<Vec<_>>();
            let params = RegistrationParams {
                registrations: vec![Registration {
                    id: "crucible-watched-files".into(),
                    method: DidChangeWatchedFiles::METHOD.into(),
                    register_options: Some(serde_json::to_value(
                        DidChangeWatchedFilesRegistrationOptions { watchers },
                    )?),
                }],
            };
            self.send(Message::Request(Request::new(
                RequestId::from("crucible-register-watchers".to_owned()),
                RegisterCapability::METHOD.into(),
                params,
            )))?;
        }
        // Searching a large folder takes a while: do it on another thread
        // and compile what it finds between the editor's requests.
        let (sender, receiver) = crossbeam_channel::unbounded();
        let roots = self.roots.clone();
        std::thread::spawn(move || {
            // Each folder gets its own budget: a large first folder must not
            // leave the others unsearched.
            for root in roots {
                let mut budget = DISCOVERY_BUDGET;
                let listening = discover(&root, DISCOVERY_DEPTH, &mut budget, &mut |dir| {
                    sender.send(dir.to_path_buf()).is_ok()
                });
                if !listening && budget > 0 {
                    break;
                }
            }
        });
        self.discovery = Some(receiver);
        Ok(())
    }

    /// A package the background search found: remember it and queue it.
    fn found(&mut self, dir: &Path) {
        // The search may report a package a watched change has removed since.
        if !is_package(dir) {
            return;
        }
        let key = normalize(dir);
        self.spelling.insert(key.clone(), dir.to_path_buf());
        if !self.packages.contains(&key) {
            self.packages.push(key.clone());
            self.queue.push_back(key);
        }
    }

    /// Compiles the next queued package nothing has compiled yet.
    fn compile_queued(&mut self) -> Result<(), Error> {
        while let Some(key) = self.queue.pop_front() {
            if !self.units.contains_key(&key) {
                return self.recompile(&key);
            }
        }
        Ok(())
    }

    /// The nearest compiled unit around `path`.
    fn enclosing_unit(&self, path: &Path) -> Option<PathBuf> {
        path.ancestors()
            .skip(1)
            .map(normalize)
            .find(|dir| self.units.contains_key(dir))
    }

    /// Records where an open document's unit is on disk, as it is spelled.
    fn remember_spelling(&mut self, uri: &Url) {
        let Some(document) = self.documents.get(uri) else {
            return;
        };
        let (Some(path), Some(unit)) = (document.path.as_deref(), document.unit.as_ref()) else {
            return;
        };
        if let Some(dir) = path.ancestors().find(|dir| normalize(dir) == *unit) {
            self.spelling.insert(unit.clone(), dir.to_path_buf());
        }
    }

    /// The packages to recompile after files changed on disk. Only the
    /// packages a change touches are looked at again; the workspace is never
    /// searched twice.
    fn files_changed(&mut self, changes: &[FileEvent]) -> Vec<PathBuf> {
        let mut dirty: Vec<PathBuf> = Vec::new();
        let push = |dirty: &mut Vec<PathBuf>, unit: PathBuf| {
            if !dirty.contains(&unit) {
                dirty.push(unit);
            }
        };
        let mut manifests = false;
        for change in changes {
            let Ok(path) = change.uri.to_file_path() else {
                continue;
            };
            let key = normalize(&path);
            // Build output and dependencies below a workspace folder hold
            // copies, as discovery knows; above it, names are the user's.
            // Compared as spelled, as discovery compares them.
            let depth = self
                .roots
                .iter()
                .find(|root| key.starts_with(normalize(root)))
                .map(|root| root.components().count());
            if let Some(depth) = depth
                && path
                    .components()
                    .skip(depth)
                    .any(|part| part.as_os_str().to_str().is_some_and(skipped))
            {
                continue;
            }
            let is_file = path
                .extension()
                .is_some_and(|ext| ext == "cru" || ext == "toml" || ext == "json" || ext == "lock");
            if change.typ == FileChangeType::DELETED && !is_file {
                // A directory went: every package in it goes with it, and an
                // outer package that held its files is compiled again.
                let gone: Vec<PathBuf> = self
                    .units
                    .iter()
                    .filter(|(unit, compiled)| {
                        unit.starts_with(&key)
                            || compiled.compiled.files[..compiled.own]
                                .iter()
                                .any(|file| normalize(Path::new(&file.path)).starts_with(&key))
                    })
                    .map(|(unit, _)| unit)
                    .chain(self.packages.iter().filter(|unit| unit.starts_with(&key)))
                    .cloned()
                    .collect();
                self.packages.retain(|package| !package.starts_with(&key));
                for unit in gone {
                    push(&mut dirty, unit);
                }
                // The package around it too: a release or an empty source
                // directory may have gone with it.
                if let Some(unit) = self.enclosing_unit(&path) {
                    push(&mut dirty, unit);
                }
                manifests = true;
                continue;
            }
            if path.file_name().is_some_and(|name| name == "crucible.toml") {
                manifests = true;
                if let Some(dir) = path.parent() {
                    let unit = normalize(dir);
                    if is_package(dir) {
                        self.spelling.insert(unit.clone(), dir.to_path_buf());
                        if !self.packages.contains(&unit) {
                            self.packages.push(unit.clone());
                        }
                    } else {
                        self.packages.retain(|package| *package != unit);
                    }
                    push(&mut dirty, unit);
                    // A package leaves out a nested package's directory, so a
                    // manifest appearing or going changes what the packages
                    // around it compile.
                    for outer in dir.ancestors().skip(1).map(normalize) {
                        if self.units.contains_key(&outer) || self.packages.contains(&outer) {
                            push(&mut dirty, outer);
                        }
                    }
                }
                continue;
            }
            // Every package that compiled this file (one whose `sources`
            // name another's directory holds it too), or the nearest one for
            // a new file.
            let mut found = false;
            for (unit, compiled) in &self.units {
                let holds = compiled.compiled.files[..compiled.own]
                    .iter()
                    .any(|file| normalize(Path::new(&file.path)) == key);
                if holds {
                    push(&mut dirty, unit.clone());
                    found = true;
                }
            }
            if !found && let Some(unit) = self.enclosing_unit(&path) {
                push(&mut dirty, unit);
            }
        }
        if manifests {
            // Which package an open document belongs to may have changed.
            let updates: Vec<(Url, Option<PathBuf>)> = self
                .documents
                .iter()
                .map(|(uri, document)| {
                    (
                        uri.clone(),
                        unit_of(document.kind, document.path.as_deref(), uri, &document.text),
                    )
                })
                .collect();
            for (uri, unit) in updates {
                let Some(document) = self.documents.get_mut(&uri) else {
                    continue;
                };
                if document.unit != unit {
                    if let Some(old) = document.unit.take() {
                        push(&mut dirty, old);
                    }
                    if let Some(new) = &unit {
                        push(&mut dirty, new.clone());
                    }
                    document.unit = unit;
                    self.remember_spelling(&uri);
                }
            }
        }
        dirty
    }

    pub fn run(mut self) -> Result<(), Error> {
        self.start()?;
        let mut next: Option<Message> = None;
        loop {
            let message = match next.take() {
                Some(message) => message,
                None => match self.next_message()? {
                    Some(message) => message,
                    None => break,
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
                        for unit in self.notification(notification)? {
                            if !dirty.contains(&unit) {
                                dirty.push(unit);
                            }
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

    /// The next message from the editor, doing background work while there
    /// is none: taking in the packages discovery found and compiling them
    /// one at a time. `None` when the editor has gone.
    fn next_message(&mut self) -> Result<Option<Message>, Error> {
        loop {
            let receiver = self.connection.receiver.clone();
            if !self.queue.is_empty() {
                match receiver.try_recv() {
                    Ok(message) => return Ok(Some(message)),
                    Err(TryRecvError::Disconnected) => return Ok(None),
                    Err(TryRecvError::Empty) => {}
                }
                if let Some(discovery) = &self.discovery {
                    let found: Vec<PathBuf> = discovery.try_iter().collect();
                    for dir in found {
                        self.found(&dir);
                    }
                }
                self.compile_queued()?;
                continue;
            }
            let Some(discovery) = self.discovery.clone() else {
                return Ok(receiver.recv().ok());
            };
            crossbeam_channel::select! {
                recv(receiver) -> message => return Ok(message.ok()),
                recv(discovery) -> dir => match dir {
                    Ok(dir) => self.found(&dir),
                    Err(_) => self.discovery = None,
                },
            }
        }
    }

    fn send(&self, message: Message) -> Result<(), Error> {
        self.connection.sender.send(message)?;
        Ok(())
    }

    // ---- documents ---------------------------------------------------------

    /// Applies one notification to the open documents. Returns the packages
    /// to recompile; the caller compiles once per burst of edits.
    fn notification(&mut self, notification: Notification) -> Result<Vec<PathBuf>, Error> {
        let uri = match notification.method.as_str() {
            DidChangeWatchedFiles::METHOD => {
                let params: lsp_types::DidChangeWatchedFilesParams =
                    serde_json::from_value(notification.params)?;
                return Ok(self.files_changed(&params.changes));
            }
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
                self.remember_spelling(&document.uri);
                document.uri
            }
            DidChangeTextDocument::METHOD => {
                let params: lsp_types::DidChangeTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                let Some(document) = self.documents.get_mut(&params.text_document.uri) else {
                    return Ok(Vec::new());
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
                    // Adding `[model]` makes a package discovery never saw.
                    self.remember_spelling(&params.text_document.uri);
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
                    return Ok(Vec::new());
                };
                self.publish(params.text_document.uri, Vec::new())?;
                return Ok(document.unit.into_iter().collect());
            }
            _ => return Ok(Vec::new()),
        };
        let Some(document) = self.documents.get(&uri) else {
            return Ok(Vec::new());
        };
        match document.unit.clone() {
            Some(unit) => Ok(vec![unit]),
            // A policy-only manifest: its syntax is all there is to check.
            None => {
                let diagnostics = self.manifest_diagnostics(&uri);
                self.publish(uri, diagnostics)?;
                Ok(Vec::new())
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
        let models_open = open
            .iter()
            .any(|uri| self.documents[uri].kind == Kind::Model);
        // A package stays compiled with nothing open while it is still a
        // package; a lone file only while it is open.
        let keep = models_open || (self.packages.iter().any(|p| p == unit) && is_package(unit));
        if !keep {
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

    fn compile_unit(&self, key: &Path, open: &[Url]) -> Unit {
        // Read from the directory as spelled on disk, so the locations sent
        // back to the editor keep their case.
        let unit = self.spelling.get(key).map_or(key, PathBuf::as_path);
        let standalone = !unit.is_dir();
        let models: Vec<&Url> = open
            .iter()
            .filter(|uri| self.documents[*uri].kind == Kind::Model)
            .collect();
        let manifest = open
            .iter()
            .find(|uri| self.documents[*uri].kind == Kind::Manifest)
            .map(|uri| self.documents[uri].text.as_str());
        let overlays: Vec<SourceFile> = models
            .iter()
            .filter_map(|uri| {
                let document = &self.documents[*uri];
                Some(SourceFile {
                    path: document.path.as_ref()?.display().to_string(),
                    text: document.text.clone(),
                })
            })
            .collect();
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
            let disk_manifest;
            let text = match manifest {
                Some(text) => Ok(text),
                None => {
                    disk_manifest = std::fs::read_to_string(unit.join("crucible.toml"));
                    disk_manifest.as_deref().map_err(|error| {
                        format!(
                            "cannot read {}: {error}",
                            unit.join("crucible.toml").display()
                        )
                    })
                }
            };
            text.and_then(|text| {
                crucible_lang::package::load_package_with_overlays(unit, text, &overlays)
            })
        };
        let (package, load_error) = match loaded {
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
        let own = package.files.len();
        let compiled = compile(&package, None);
        let index = Index::new(&compiled.parsed);
        let mut usages: HashMap<String, usize> = HashMap::new();
        for (parsed, file) in compiled.parsed.iter().zip(&compiled.files).take(own) {
            for (name, count) in analysis::reference_counts(parsed, &file.text, &index) {
                *usages.entry(name).or_default() += count;
            }
        }
        // The policy the build applies: the package manifest's, or one above
        // it. A policy that does not parse shows in the manifest instead.
        let policy_dir = if standalone {
            unit.parent()
        } else {
            Some(unit)
        };
        let config = policy_dir
            .and_then(|dir| policy::source(dir, manifest))
            .and_then(|text| crucible_policy::Config::parse(&text).ok());
        let mut policy = config
            .as_ref()
            .map(|config| policy::findings(&compiled, own, config))
            .unwrap_or_default();
        // The compatibility guard, against the frozen baseline, as the build
        // applies it.
        let mut baseline_notes = Vec::new();
        let config = config.unwrap_or_default();
        match crucible_lang::guard(&package, &compiled, own, &config) {
            None => {}
            Some(Err(problem)) => baseline_notes.push((true, problem)),
            Some(Ok(guard)) => {
                let (placed, unplaced) = policy::compatibility(&compiled, own, guard);
                policy.extend(placed);
                baseline_notes.extend(unplaced);
            }
        }
        Unit {
            compiled,
            index,
            own,
            standalone: standalone || load_error.is_some(),
            load_error,
            usages,
            policy,
            baseline_notes,
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
        for finding in &unit.policy {
            if Some(finding.file) != own_index {
                continue;
            }
            out.push(Diagnostic {
                range: lines.range(finding.start, finding.end),
                severity: Some(if finding.error {
                    DiagnosticSeverity::ERROR
                } else {
                    DiagnosticSeverity::WARNING
                }),
                code: Some(lsp_types::NumberOrString::String(finding.rule.clone())),
                source: Some("crucible policy".into()),
                message: finding.message.clone(),
                data: finding
                    .rename
                    .as_ref()
                    .map(|name| serde_json::json!({ "rename": name })),
                ..Default::default()
            });
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
        let mut out: Vec<Diagnostic> = check_manifest(dir, &document.text)
            .into_iter()
            .map(|diagnostic| {
                at_severity(
                    lines.range(diagnostic.span.start, diagnostic.span.end),
                    diagnostic.message,
                    diagnostic.severity,
                )
            })
            .collect();
        // The policy beside [model], or a policy-only file: the same checks
        // the build applies, at the value they are about.
        if crucible_policy::Config::declares_policy(&document.text)
            && let Err(problem) = crucible_policy::Config::parse_located(&document.text)
        {
            let range = problem.span.map_or(lines.first_line(), |span| {
                lines.range(span.start as u32, span.end as u32)
            });
            // TOML syntax errors are already reported by the manifest check.
            if !out.iter().any(|existing| existing.range == range) {
                out.push(error(range, problem.message));
            }
        }
        // What the baseline guard could not place in a model file.
        let unit = document.unit.as_ref().and_then(|unit| self.units.get(unit));
        if let Some(unit) = unit.filter(|unit| !unit.baseline_notes.is_empty()) {
            let at = document
                .text
                .find("[model.baseline]")
                .map_or(lines.first_line(), |start| {
                    lines.range(start as u32, (start + "[model.baseline]".len()) as u32)
                });
            for (is_error, message) in &unit.baseline_notes {
                out.push(Diagnostic {
                    severity: Some(if *is_error {
                        DiagnosticSeverity::ERROR
                    } else {
                        DiagnosticSeverity::WARNING
                    }),
                    ..error(at, message.clone())
                });
            }
        }
        out
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
                let trigger = p.context.and_then(|context| context.trigger_character);
                json(self.completion(&at.text_document.uri, at.position, trigger.as_deref()))
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
                json(self.formatting(&p.text_document.uri, &p.options, None))
            }
            RangeFormatting::METHOD => {
                let p: lsp_types::DocumentRangeFormattingParams = params(request.params)?;
                json(self.formatting(&p.text_document.uri, &p.options, Some(p.range)))
            }
            GotoTypeDefinition::METHOD => {
                let p: lsp_types::request::GotoTypeDefinitionParams = params(request.params)?;
                let at = p.text_document_position_params;
                json(self.type_definition(&at.text_document.uri, at.position))
            }
            InlayHintRequest::METHOD => {
                let p: lsp_types::InlayHintParams = params(request.params)?;
                json(self.inlay_hints(&p.text_document.uri, p.range))
            }
            DocumentLinkRequest::METHOD => {
                let p: lsp_types::DocumentLinkParams = params(request.params)?;
                json(self.links(&p.text_document.uri))
            }
            CodeLensRequest::METHOD => {
                let p: lsp_types::CodeLensParams = params(request.params)?;
                json(self.code_lenses(&p.text_document.uri))
            }
            SignatureHelpRequest::METHOD => {
                let p: lsp_types::SignatureHelpParams = params(request.params)?;
                let at = p.text_document_position_params;
                json(self.signature_help(&at.text_document.uri, at.position))
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

    fn completion(
        &self,
        uri: &Url,
        position: Position,
        trigger: Option<&str>,
    ) -> Option<CompletionResponse> {
        if let Some(document) = self.documents.get(uri)
            && document.kind == Kind::Manifest
        {
            let offset = Lines::new(&document.text).offset(position);
            return Some(CompletionResponse::Array(manifest::complete(
                &document.text,
                offset,
            )));
        }
        let (document, parsed, unit) = self.model(uri)?;
        let lines = Lines::new(&document.text);
        let offset = lines.offset(position);
        let context = completion::context(&parsed, &document.text, offset);
        if context == completion::Context::Binding {
            // Before the package compiles, the open file's own resources.
            let package = unit.map_or(&[][..], |unit| &unit.compiled.parsed);
            return Some(CompletionResponse::Array(completion::bindings(
                &parsed,
                package,
                &document.text,
                offset,
                &lines,
            )));
        }
        // `$` opens the list only where it binds: a `$` in prose or a
        // value would otherwise pop keywords up.
        if trigger == Some("$") {
            return None;
        }
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
        self.location_of(unit, declaration, uri)
            .map(GotoDefinitionResponse::Scalar)
    }

    /// Where a declaration's name is, if it is in a file the editor can open.
    fn location_of(
        &self,
        unit: &Unit,
        declaration: &analysis::Declaration,
        fallback: &Url,
    ) -> Option<Location> {
        if declaration.file >= unit.own {
            // A bundled import has no file to open; hover shows its text.
            return None;
        }
        let target = self.uri_of(unit, declaration.file, fallback)?;
        let lines = Lines::new(&unit.compiled.files[declaration.file].text);
        Some(Location {
            uri: target,
            range: lines.range(declaration.name_start, declaration.name_end),
        })
    }

    /// From a member's name, its type; from a type's name, the type itself.
    fn type_definition(&self, uri: &Url, position: Position) -> Option<GotoDefinitionResponse> {
        let (document, parsed, unit) = self.model(uri)?;
        let unit = unit?;
        let offset = Lines::new(&document.text).offset(position) as u32;
        let package = &unit.compiled.parsed;
        if let Some(name) = insight::type_name_at(&parsed, package, offset) {
            let declaration = unit.index.declarations.get(&name)?;
            return self
                .location_of(unit, declaration, uri)
                .map(GotoDefinitionResponse::Scalar);
        }
        self.definition(uri, position)
    }

    fn inlay_hints(&self, uri: &Url, range: Range) -> Option<Vec<InlayHint>> {
        let (document, parsed, unit) = self.model(uri)?;
        let lines = Lines::new(&document.text);
        let package = unit.map_or(&[][..], |unit| &unit.compiled.parsed);
        let (start, end) = (lines.offset(range.start), lines.offset(range.end));
        Some(
            insight::inlay_hints(&parsed, package, start as u32, end as u32)
                .into_iter()
                .map(|hint| InlayHint {
                    position: lines.position(hint.offset as usize),
                    label: InlayHintLabel::String(hint.label),
                    kind: Some(match hint.kind {
                        HintKind::Type => InlayHintKind::TYPE,
                        HintKind::Value => InlayHintKind::PARAMETER,
                    }),
                    text_edits: None,
                    tooltip: None,
                    padding_left: Some(hint.kind == HintKind::Value),
                    padding_right: None,
                    data: None,
                })
                .collect(),
        )
    }

    fn links(&self, uri: &Url) -> Option<Vec<DocumentLink>> {
        let (document, parsed, _) = self.model(uri)?;
        let lines = Lines::new(&document.text);
        Some(
            insight::links(&parsed, &document.text)
                .into_iter()
                .filter_map(|(start, end, url)| {
                    Some(DocumentLink {
                        range: lines.range(start, end),
                        target: Some(Url::parse(&url).ok()?),
                        tooltip: None,
                        data: None,
                    })
                })
                .collect(),
        )
    }

    /// A usage count above each declaration in the file.
    fn code_lenses(&self, uri: &Url) -> Option<Vec<CodeLens>> {
        let (document, parsed, unit) = self.model(uri)?;
        let unit = unit?;
        let lines = Lines::new(&document.text);
        Some(
            analysis::outline(&parsed)
                .into_iter()
                .map(|symbol| {
                    let count = unit.usages.get(&symbol.name).copied().unwrap_or(0);
                    let title = match count {
                        0 => "no usages".to_owned(),
                        1 => "1 usage".to_owned(),
                        n => format!("{n} usages"),
                    };
                    let range = lines.range(symbol.name_start, symbol.name_end);
                    CodeLens {
                        range,
                        command: Some(Command {
                            title,
                            command: SHOW_REFERENCES.to_owned(),
                            arguments: Some(vec![
                                Value::String(uri.to_string()),
                                serde_json::to_value(range.start).unwrap_or(Value::Null),
                            ]),
                        }),
                        data: None,
                    }
                })
                .collect(),
        )
    }

    fn signature_help(&self, uri: &Url, position: Position) -> Option<SignatureHelp> {
        let (document, parsed, unit) = self.model(uri)?;
        let offset = Lines::new(&document.text).offset(position);
        let package = unit.map_or(&[][..], |unit| &unit.compiled.parsed);
        let signature = insight::signature_at(&parsed, package, &document.text, offset)?;
        Some(SignatureHelp {
            signatures: vec![SignatureInformation {
                label: signature.label,
                documentation: (!signature.documentation.is_empty())
                    .then_some(lsp_types::Documentation::String(signature.documentation)),
                parameters: Some(
                    signature
                        .parameters
                        .into_iter()
                        .map(|parameter| ParameterInformation {
                            label: ParameterLabel::Simple(parameter),
                            documentation: None,
                        })
                        .collect(),
                ),
                active_parameter: Some(signature.active),
            }],
            active_signature: Some(0),
            active_parameter: Some(signature.active),
        })
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
        if analysis::BUILTIN_TYPES.contains(&new_name) {
            return Err(failed(format!(
                "`{new_name}` is a built-in type and cannot be declared"
            )));
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
        if let Some(document) = self.documents.get(uri)
            && document.kind == Kind::Manifest
        {
            let offset = Lines::new(&document.text).offset(position);
            return Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: manifest::hover(&document.text, offset)?,
                }),
                range: None,
            });
        }
        let (document, parsed, unit) = self.model(uri)?;
        let lines = Lines::new(&document.text);
        // Before the package first compiles, the open file still explains
        // its own keywords and declarations.
        let own;
        let (index, package) = match unit {
            Some(unit) => (&unit.index, &unit.compiled.parsed[..]),
            None => {
                own = Index::new(std::slice::from_ref(&parsed));
                (&own, &[][..])
            }
        };
        let (start, end, value) = hover::hover(
            &parsed,
            &document.text,
            index,
            package,
            lines.offset(position),
        )?;
        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: Some(lines.range(start, end)),
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
        for (key, unit) in &self.units {
            // A lone file's URI is its document's; a package's files have
            // their own paths.
            let fallback = self
                .documents
                .iter()
                .find(|(_, document)| document.unit.as_ref() == Some(key))
                .map(|(uri, _)| uri.clone())
                .or_else(|| {
                    let dir = self.spelling.get(key).unwrap_or(key);
                    Url::from_directory_path(clean(dir)).ok()
                });
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

    /// The whole document formatted, or with `range` only the lines it
    /// touches.
    fn formatting(
        &self,
        uri: &Url,
        options: &lsp_types::FormattingOptions,
        range: Option<Range>,
    ) -> Option<Vec<TextEdit>> {
        let (document, _, _) = self.model(uri)?;
        let text = &document.text;
        let indent = if options.insert_spaces {
            " ".repeat(options.tab_size.clamp(1, 16) as usize)
        } else {
            "\t".to_owned()
        };
        let options = FormatOptions { indent };
        let lines = Lines::new(text);
        let (start, end, formatted) = match range {
            None => (0, text.len(), format(text, &options)),
            Some(range) => {
                let formatted = format_lines(text, &options);
                // Byte offset of each line's start, and the text's end.
                let mut starts: Vec<usize> = std::iter::once(0)
                    .chain(text.match_indices('\n').map(|(at, _)| at + 1))
                    .collect();
                starts.push(text.len());
                let first = (range.start.line as usize).min(formatted.len() - 1);
                // A selection ending at a line's start does not touch it.
                let last = if range.end.character == 0 && range.end.line > range.start.line {
                    range.end.line as usize - 1
                } else {
                    range.end.line as usize
                }
                .clamp(first, formatted.len() - 1);
                (
                    starts[first],
                    starts[last + 1],
                    formatted[first..=last].concat(),
                )
            }
        };
        if formatted == text[start..end] {
            return Some(Vec::new());
        }
        Some(vec![TextEdit {
            range: lines.range(start as u32, end as u32),
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
            // A naming finding renames the declaration everywhere, or a
            // member or enum value where it is declared.
            if let Some(new_name) = diagnostic
                .data
                .as_ref()
                .and_then(|data| data.get("rename"))
                .and_then(Value::as_str)
            {
                let edit = match self.rename(uri, diagnostic.range.start, new_name) {
                    Ok(Some(edit)) => edit,
                    // A member or enum value is not a declaration rename
                    // knows. Edit it in place only when nothing else in the
                    // package spells its name (a URI label, `paginated(..)`,
                    // an example), and never into a name the language
                    // cannot hold, such as kebab-case.
                    // A rename the server refused (a clash, an import)
                    // never becomes a single-site edit.
                    Ok(None)
                        if valid_name(new_name)
                            && unit.is_some_and(|unit| {
                                let start = lines.offset(diagnostic.range.start);
                                let end = lines.offset(diagnostic.range.end);
                                // A client may send any range; never panic on one.
                                document
                                    .text
                                    .get(start..end)
                                    .is_some_and(|old| name_occurrences(unit, old) == 1)
                            }) =>
                    {
                        WorkspaceEdit {
                            changes: Some(HashMap::from([(
                                uri.clone(),
                                vec![TextEdit {
                                    range: diagnostic.range,
                                    new_text: new_name.to_owned(),
                                }],
                            )])),
                            ..Default::default()
                        }
                    }
                    _ => continue,
                };
                out.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: format!("Rename to `{new_name}`"),
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![diagnostic.clone()]),
                    edit: Some(edit),
                    is_preferred: Some(true),
                    ..Default::default()
                }));
                continue;
            }
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

/// Whether `name` can name a declaration or member.
fn valid_name(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// How often `name` is written as a whole word in the unit's own files,
/// strings and comments included.
fn name_occurrences(unit: &Unit, name: &str) -> usize {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    unit.compiled.files[..unit.own]
        .iter()
        .map(|file| {
            file.text
                .match_indices(name)
                .filter(|(at, _)| {
                    !file.text[..*at].ends_with(word)
                        && !file.text[at + name.len()..].starts_with(word)
                })
                .count()
        })
        .sum()
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
