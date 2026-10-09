//! Compiling a package: parse every file, resolve names across them and the
//! packages it imports, check the model, and lower one service to
//! `crucible_smithy::ir`.

use crucible_smithy::ir;

use crate::diagnostic::Diagnostic;
use crate::span::{LineIndex, Span};
use crate::{Parsed, parse};

/// One `.cru` file of a package.
#[derive(Debug, Clone)]
pub struct SourceFile {
    /// How diagnostics name the file.
    pub path: String,
    pub text: String,
}

/// A package: one namespace and the files declaring it (decision 5).
#[derive(Debug, Clone)]
pub struct Package {
    pub namespace: String,
    pub files: Vec<SourceFile>,
    pub naming: Naming,
    /// Packages whose declarations this one uses: the bundled
    /// `crucible.errors` today (decision 60).
    pub imports: Vec<String>,
}

impl Package {
    pub fn new(namespace: impl Into<String>, files: Vec<SourceFile>) -> Self {
        Self {
            namespace: namespace.into(),
            files,
            naming: Naming::default(),
            imports: Vec::new(),
        }
    }
}

/// Packages bundled with the compiler, by namespace.
pub(crate) const BUNDLED: &[(&str, &str)] = &[(
    "crucible.errors",
    include_str!("../bundled/crucible.errors.cru"),
)];

/// How anonymous operation inputs and outputs are named (decision 25).
#[derive(Debug, Clone)]
pub struct Naming {
    /// A template with `{Operation}`, `{Operation}Input` by default.
    pub input: String,
    pub output: String,
}

impl Default for Naming {
    fn default() -> Self {
        Self {
            input: "{Operation}Input".to_owned(),
            output: "{Operation}Output".to_owned(),
        }
    }
}

impl Naming {
    pub(crate) fn input(&self, operation: &str) -> String {
        self.input.replace("{Operation}", operation)
    }

    pub(crate) fn output(&self, operation: &str) -> String {
        self.output.replace("{Operation}", operation)
    }
}

/// Where something in the IR was declared (decision 61).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    /// Index into [`Compiled::files`].
    pub file: usize,
    pub span: Span,
}

/// Declaration sites of the service, its operations, shapes and members, by
/// IR id (`namespace#Shape` or `namespace#Shape$member`). The IR stays free of
/// positions; rules and compatibility checks look them up here.
pub type SourceMap = std::collections::BTreeMap<crucible_smithy::ShapeId, Location>;

/// A diagnostic and the file it belongs to; `None` is the package itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiagnostic {
    pub file: Option<usize>,
    pub diagnostic: Diagnostic,
}

/// The result of compiling a package.
#[derive(Debug)]
pub struct Compiled {
    pub namespace: String,
    /// Every file compiled: the package's own, then its imports'.
    pub files: Vec<SourceFile>,
    pub parsed: Vec<Parsed>,
    /// Present when the package compiled without a diagnostic.
    pub service: Option<ir::Service>,
    pub source_map: SourceMap,
    pub diagnostics: Vec<FileDiagnostic>,
}

impl Compiled {
    /// Whether any diagnostic is an error, which withholds the service.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|item| item.diagnostic.is_error())
    }

    /// Every diagnostic, rendered with file, line, column and the source line.
    pub fn render(&self) -> String {
        let mut out = String::new();
        let mut indexes: Vec<Option<LineIndex>> = vec![None; self.files.len()];
        for item in &self.diagnostics {
            match item
                .file
                .and_then(|index| self.files.get(index).map(|file| (index, file)))
            {
                Some((index, file)) => {
                    let lines = indexes[index].get_or_insert_with(|| LineIndex::new(&file.text));
                    out.push_str(&item.diagnostic.render(&file.path, &file.text, lines));
                }
                None => {
                    let prefix = if item.diagnostic.is_error() {
                        ""
                    } else {
                        "warning: "
                    };
                    out.push_str(&format!(
                        "{}: {prefix}{}\n",
                        self.namespace, item.diagnostic.message
                    ));
                }
            }
        }
        out
    }
}

/// Why `namespace` cannot name a package, if it cannot: it is dot-separated
/// identifiers, and the Smithy prelude's and bundled packages' namespaces are
/// taken.
pub(crate) fn namespace_problem(namespace: &str) -> Option<String> {
    let valid = !namespace.is_empty()
        && namespace.split('.').all(|part| {
            part.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
    if !valid {
        return Some(format!(
            "`{namespace}` is not a namespace: use dot-separated identifiers, such as `example.service`"
        ));
    }
    if namespace == "smithy.api" || BUNDLED.iter().any(|(bundled, _)| *bundled == namespace) {
        return Some(format!(
            "the namespace `{namespace}` is reserved for {}",
            if namespace == "smithy.api" {
                "the prelude"
            } else {
                "a bundled package"
            }
        ));
    }
    None
}

/// Compiles `package` and lowers `service` (or its only service).
///
/// Every file is checked: a syntax error or an unknown import does not hide
/// the problems in the rest of the package (AGENTS.md: one run lists every
/// problem). Semantic diagnostics inside a file with syntax errors are
/// dropped, since its tree has placeholders where the source is broken.
pub fn compile(package: &Package, service: Option<&str>) -> Compiled {
    let mut files = package.files.clone();
    let mut namespaces = vec![package.namespace.clone(); files.len()];
    let mut diagnostics = Vec::new();
    if let Some(problem) = namespace_problem(&package.namespace) {
        diagnostics.push(FileDiagnostic {
            file: None,
            diagnostic: Diagnostic::new(Span::default(), problem),
        });
    }
    let mut imported = Vec::new();
    for import in &package.imports {
        if imported.contains(import) {
            continue;
        }
        imported.push(import.clone());
        match BUNDLED.iter().find(|(namespace, _)| namespace == import) {
            Some((namespace, text)) => {
                files.push(SourceFile {
                    path: format!("<bundled {namespace}>"),
                    text: (*text).to_owned(),
                });
                namespaces.push((*namespace).to_owned());
            }
            None => {
                let known: Vec<&str> = BUNDLED.iter().map(|(namespace, _)| *namespace).collect();
                diagnostics.push(FileDiagnostic {
                    file: None,
                    diagnostic: Diagnostic::new(
                        Span::default(),
                        format!(
                            "unknown import `{import}`; the bundled packages are {}",
                            known.join(", ")
                        ),
                    ),
                });
            }
        }
    }
    let parsed: Vec<Parsed> = files.iter().map(|file| parse(&file.text)).collect();
    let broken: Vec<bool> = parsed
        .iter()
        .map(|parsed| parsed.diagnostics.iter().any(Diagnostic::is_error))
        .collect();
    for (index, parsed) in parsed.iter().enumerate() {
        diagnostics.extend(parsed.diagnostics.iter().map(|diagnostic| FileDiagnostic {
            file: Some(index),
            diagnostic: diagnostic.clone(),
        }));
    }
    let (lowered, source_map, problems) =
        crate::lower::lower(package, &parsed, &namespaces, service);
    diagnostics.extend(
        problems
            .into_iter()
            .filter(|problem| !problem.file.is_some_and(|file| broken[file])),
    );
    let failed = diagnostics.iter().any(|item| item.diagnostic.is_error());
    Compiled {
        namespace: package.namespace.clone(),
        files,
        parsed,
        service: if failed { None } else { lowered },
        source_map,
        diagnostics,
    }
}
