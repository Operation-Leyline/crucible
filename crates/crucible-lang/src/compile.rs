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
    /// Every diagnostic, rendered with file, line, column and the source line.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for item in &self.diagnostics {
            match item.file.and_then(|index| self.files.get(index)) {
                Some(file) => {
                    let lines = LineIndex::new(&file.text);
                    out.push_str(&item.diagnostic.render(&file.path, &file.text, &lines));
                }
                None => {
                    out.push_str(&format!(
                        "{}: {}\n",
                        self.namespace, item.diagnostic.message
                    ));
                }
            }
        }
        out
    }
}

/// Compiles `package` and lowers `service` (or its only service).
pub fn compile(package: &Package, service: Option<&str>) -> Compiled {
    let mut files = package.files.clone();
    let mut namespaces = vec![package.namespace.clone(); files.len()];
    let mut diagnostics = Vec::new();
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
    for (index, parsed) in parsed.iter().enumerate() {
        diagnostics.extend(parsed.diagnostics.iter().map(|diagnostic| FileDiagnostic {
            file: Some(index),
            diagnostic: diagnostic.clone(),
        }));
    }
    // A tree with syntax errors has placeholder names; lowering it would only
    // add noise on top of the real problems.
    if !diagnostics.is_empty() {
        return Compiled {
            namespace: package.namespace.clone(),
            files,
            parsed,
            service: None,
            source_map: SourceMap::new(),
            diagnostics,
        };
    }
    let (lowered, source_map, mut problems) =
        crate::lower::lower(package, &parsed, &namespaces, service);
    diagnostics.append(&mut problems);
    let service = if diagnostics.is_empty() {
        lowered
    } else {
        None
    };
    Compiled {
        namespace: package.namespace.clone(),
        files,
        parsed,
        service,
        source_map,
        diagnostics,
    }
}
