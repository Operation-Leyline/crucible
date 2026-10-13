//! Loading a package from disk: a directory whose `crucible.toml` has a
//! `[model]` table (decision 6).
//!
//! ```toml
//! [model]
//! namespace = "example.service"
//! sources = ["."]                 # directories holding `.cru` files
//! imports = ["crucible.errors"]
//!
//! [model.naming]
//! input = "{Operation}Input"
//! output = "{Operation}Output"
//! ```

use std::path::{Component, Path, PathBuf};

use crate::compile::{Naming, Package, SourceFile};
use crate::diagnostic::Diagnostic;
use crate::span::Span;

/// Whether `dir` is a language package: it holds a `crucible.toml` with a
/// `[model]` table. A policy-only `crucible.toml` is not a package.
pub fn is_package(dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(dir.join("crucible.toml")) else {
        return false;
    };
    text.parse::<toml::Table>()
        .ok()
        .is_some_and(|table| table.get("model").is_some_and(toml::Value::is_table))
}

/// Reads the package in `dir`: its manifest and every `.cru` file under its
/// source directories, in path order so compilation is deterministic.
pub fn load_package(dir: &Path) -> Result<Package, String> {
    let manifest = dir.join("crucible.toml");
    let text = std::fs::read_to_string(&manifest)
        .map_err(|error| format!("cannot read {}: {error}", manifest.display()))?;
    load_package_with_manifest(dir, &text)
}

/// [`load_package`] with the manifest's text supplied, as an editor holding
/// unsaved changes to it does.
pub fn load_package_with_manifest(dir: &Path, text: &str) -> Result<Package, String> {
    load_package_with_overlays(dir, text, &[])
}

/// Loads an editor's package, replacing disk contents with open buffers and
/// including new buffers under the same source directories walked on disk.
/// Nested packages and linked directories stay outside that walk.
pub fn load_package_with_overlays(
    dir: &Path,
    text: &str,
    overlays: &[SourceFile],
) -> Result<Package, String> {
    let manifest = dir.join("crucible.toml");
    // The build and the editor apply the same checks, so a manifest the
    // editor marks wrong never builds.
    let problems = check_manifest_with_overlays(dir, text, overlays);
    if !problems.is_empty() {
        let lines = crate::span::LineIndex::new(text);
        let rendered: Vec<String> = problems
            .iter()
            .map(|problem| {
                let (line, column) = lines.line_col(text, problem.span.start);
                format!(
                    "{}:{line}:{column}: {}",
                    manifest.display(),
                    problem.message
                )
            })
            .collect();
        return Err(rendered.join("\n"));
    }
    let table: toml::Table = text
        .parse()
        .map_err(|error| format!("{}: {error}", manifest.display()))?;
    let model = table
        .get("model")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| format!("{} has no [model] table", manifest.display()))?;
    let field = |name: &str| format!("{}: [model].{name}", manifest.display());
    let namespace = model
        .get("namespace")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| format!("{} must name the package namespace", field("namespace")))?
        .to_owned();
    let strings = |name: &str, default: &[&str]| -> Result<Vec<String>, String> {
        match model.get(name) {
            None => Ok(default.iter().map(|s| (*s).to_owned()).collect()),
            Some(value) => value
                .as_array()
                .and_then(|items| {
                    items
                        .iter()
                        .map(|item| item.as_str().map(str::to_owned))
                        .collect::<Option<Vec<_>>>()
                })
                .ok_or_else(|| format!("{} must be a list of strings", field(name))),
        }
    };
    let sources = strings("sources", &["."])?;
    let imports = strings("imports", &[])?;
    let mut naming = Naming::default();
    if let Some(table) = model.get("naming").and_then(toml::Value::as_table) {
        for (key, slot) in [("input", &mut naming.input), ("output", &mut naming.output)] {
            if let Some(value) = table.get(key) {
                let value = value
                    .as_str()
                    .filter(|value| value.contains("{Operation}"))
                    .ok_or_else(|| {
                        format!(
                            "{} must be a string containing {{Operation}}",
                            field(&format!("naming.{key}"))
                        )
                    })?;
                *slot = value.to_owned();
            }
        }
    }
    let baseline = match model.get("baseline") {
        None => None,
        Some(value) => {
            let path = value
                .as_table()
                .and_then(|table| table.get("path"))
                .and_then(toml::Value::as_str)
                .ok_or_else(|| {
                    format!(
                        "{} must be a table naming the frozen release: path = \"releases/2026-09-04.json\"",
                        field("baseline")
                    )
                })?;
            if let Some(problem) = escapes(path).or_else(|| outside(dir, path)) {
                return Err(format!("{}: `{path}` {problem}", field("baseline.path")));
            }
            let frozen = dir.join(path);
            if !path.ends_with(".json") || !frozen.is_file() {
                return Err(format!(
                    "{}: {} is not a frozen release; `crucible freeze` writes one as `releases/<version>.json`",
                    field("baseline.path"),
                    frozen.display()
                ));
            }
            Some(frozen)
        }
    };
    let mut paths = Vec::new();
    for source in &sources {
        if let Some(problem) = escapes(source) {
            return Err(format!("{}: `{source}` {problem}", field("sources")));
        }
        // Joined as written, `sources = ["."]` makes `dir/.`, which would
        // print in every diagnostic (`dir\.\ops\x.cru`). Only the source's
        // `.` components go; the directory keeps its spelling and separators.
        let source_path: PathBuf = Path::new(source)
            .components()
            .filter(|component| !matches!(component, Component::CurDir))
            .collect();
        let root = if source_path.as_os_str().is_empty() {
            dir.to_path_buf()
        } else {
            dir.join(source_path)
        };
        let buffers: Vec<_> = overlays
            .iter()
            .filter(|file| overlay_in_source(dir, &root, file))
            .collect();
        if root.is_dir() {
            collect_cru(&root, &mut paths)?;
        } else if buffers.is_empty() {
            return Err(format!(
                "{}: {} is not a directory",
                field("sources"),
                root.display()
            ));
        }
        for file in buffers {
            let path = Path::new(&file.path);
            if !paths.iter().any(|existing| same_path(existing, path)) {
                paths.push(path.to_path_buf());
            }
        }
    }
    paths.sort();
    paths.dedup();
    if paths.is_empty() {
        return Err(format!(
            "{} declares no .cru files under {sources:?}",
            manifest.display()
        ));
    }
    let files = paths
        .into_iter()
        .map(|path| {
            let text = match overlays
                .iter()
                .find(|file| same_path(Path::new(&file.path), &path))
            {
                Some(file) => file.text.clone(),
                None => std::fs::read_to_string(&path)
                    .map_err(|error| format!("cannot read {}: {error}", path.display()))?,
            };
            Ok(SourceFile {
                path: path.display().to_string(),
                text,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Package {
        namespace,
        files,
        naming,
        imports,
        baseline,
    })
}

/// Why a `sources` entry is not allowed, if it is not. Sources stay inside
/// the package directory: the build reruns when that directory changes, so a
/// source elsewhere would leave generated code stale without an error.
fn escapes(source: &str) -> Option<&'static str> {
    let path = Path::new(source);
    if path.is_absolute() || path.has_root() {
        return Some("must be a path relative to the package directory");
    }
    // `C:foo` on Windows is neither absolute nor rooted, but its prefix
    // makes `join` replace the package directory.
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::Prefix(_)))
    {
        return Some("must be a path relative to the package directory");
    }
    path.components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
        .then_some("must stay inside the package directory")
}

/// Why `path`, written inside the package, resolves outside it: a symlink
/// that `escapes` cannot see in the text. A path that does not exist yet is
/// left to the existence checks.
fn outside(dir: &Path, path: &str) -> Option<&'static str> {
    let root = std::fs::canonicalize(dir).ok()?;
    let target = std::fs::canonicalize(dir.join(path)).ok()?;
    (!target.starts_with(&root)).then_some("must stay inside the package directory")
}

fn same_path(left: &Path, right: &Path) -> bool {
    let clean = |path: &Path| -> PathBuf {
        path.components()
            .filter(|component| !matches!(component, Component::CurDir))
            .collect()
    };
    let (left, right) = (clean(left), clean(right));
    if cfg!(windows) {
        left.to_string_lossy().to_lowercase() == right.to_string_lossy().to_lowercase()
    } else {
        left == right
    }
}

/// A new buffer can have missing parent directories, but every existing
/// ancestor must obey the disk walk's directory and package boundaries.
fn overlay_in_source(package: &Path, root: &Path, file: &SourceFile) -> bool {
    let path = Path::new(&file.path);
    if path.extension().is_none_or(|extension| extension != "cru")
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        || path.is_dir()
    {
        return false;
    }
    let source_exists = root.is_dir();
    let mut within_source = false;
    let mut at = path.parent();
    while let Some(parent) = at {
        if same_path(parent, root) {
            within_source = true;
        }
        if same_path(parent, package) {
            return within_source && parent.is_dir();
        }
        match std::fs::symlink_metadata(parent) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                // The disk walk follows an explicitly selected existing
                // root, including its parents, but never links below it.
                // A virtual root cannot opt into following an ancestor link.
                if !source_exists || !within_source || !parent.is_dir() {
                    return false;
                }
            }
            Ok(metadata) if !metadata.is_dir() => {
                return false;
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return false,
            _ => {}
        }
        if !within_source && is_package(parent) {
            return false;
        }
        at = parent.parent();
    }
    false
}

fn collect_cru(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|error| format!("cannot read {}: {error}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        // `file_type` does not follow symlinks: a link back to an ancestor
        // would otherwise recurse forever. Linked directories are not walked.
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        // A directory with its own package manifest is a package of its own.
        if kind.is_dir() && !is_package(&path) {
            collect_cru(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "cru") && path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

/// The problems in a manifest's text, each at the place it concerns: TOML
/// syntax, unknown or mistyped `[model]` keys, a missing namespace, missing
/// source directories, unknown imports and naming patterns without
/// `{Operation}`. A manifest without `[model]` is a policy file: only its
/// syntax is checked here.
/// The top-level policy keys `crucible-policy` reads beside `[model]`.
const POLICY_KEYS: &[&str] = &["schema_version", "rules", "compatibility", "exceptions"];

/// Where a top-level key or table is written: `key =`, `[key` or `[[key`.
fn top_level_key(text: &str, key: &str) -> Span {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let after = trimmed.trim_start_matches('[').trim_start();
        if let Some(rest) = after.strip_prefix(key)
            && rest.starts_with([' ', '=', '.', ']', '\t'])
        {
            let start = offset + (line.len() - after.len());
            return Span::new(start, start + key.len());
        }
        offset += line.len();
    }
    Span::default()
}

pub fn check_manifest(dir: &Path, text: &str) -> Vec<Diagnostic> {
    check_manifest_with_overlays(dir, text, &[])
}

/// [`check_manifest`] for an editor with open model buffers. A missing source
/// directory is valid while an eligible buffer lives under it, using the
/// same source selection as [`load_package_with_overlays`].
pub fn check_manifest_with_overlays(
    dir: &Path,
    text: &str,
    overlays: &[SourceFile],
) -> Vec<Diagnostic> {
    #[derive(serde::Deserialize)]
    struct Manifest {
        model: Option<toml::Spanned<Model>>,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Model {
        namespace: Option<toml::Spanned<String>>,
        sources: Option<Vec<toml::Spanned<String>>>,
        imports: Option<Vec<toml::Spanned<String>>>,
        naming: Option<NamingTable>,
        /// The language version the package is written in (decision 6).
        language: Option<toml::Spanned<toml::Value>>,
        /// Decided (decision 60) but not implemented: reported as such
        /// rather than as unknown keys.
        dependencies: Option<toml::Spanned<toml::Table>>,
        /// The last release's snapshot (decision 47d).
        baseline: Option<Baseline>,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Baseline {
        path: toml::Spanned<String>,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct NamingTable {
        input: Option<toml::Spanned<String>>,
        output: Option<toml::Spanned<String>>,
    }
    let span = |range: std::ops::Range<usize>| Span::new(range.start, range.end);
    let manifest: Manifest = match toml::from_str(text) {
        Ok(manifest) => manifest,
        Err(error) => {
            let at = error.span().map_or(Span::default(), span);
            return vec![Diagnostic::new(at, error.message().to_owned())];
        }
    };
    let mut out = Vec::new();
    // Beside [model] are the policy keys `crucible-policy` reads; anything
    // else is a typo that would otherwise be ignored. (Read on its own:
    // `flatten` would lose the spans the checks below report at.)
    let keys: Vec<String> = text
        .parse::<toml::Table>()
        .map(|table| table.keys().cloned().collect())
        .unwrap_or_default();
    for key in keys.iter().filter(|key| *key != "model") {
        if !POLICY_KEYS.contains(&key.as_str()) {
            out.push(Diagnostic::new(
                top_level_key(text, key),
                format!(
                    "unknown key `{key}` in crucible.toml; it holds [model] and the policy keys {}",
                    POLICY_KEYS.join(", ")
                ),
            ));
        }
    }
    let Some(model) = manifest.model else {
        return out;
    };
    let header = span(model.span());
    let model = model.into_inner();
    match &model.namespace {
        None => out.push(Diagnostic::new(
            header,
            "[model] must name the package namespace: namespace = \"example.service\"",
        )),
        Some(namespace) => {
            if let Some(problem) = crate::compile::namespace_problem(namespace.get_ref()) {
                out.push(Diagnostic::new(span(namespace.span()), problem));
            }
        }
    }
    if let Some(language) = &model.language
        && language.get_ref().as_integer() != Some(1)
    {
        out.push(Diagnostic::new(
            span(language.span()),
            "this compiler reads language version 1: language = 1",
        ));
    }
    let sources = model.sources.unwrap_or_default();
    for source in &sources {
        if let Some(problem) = escapes(source.get_ref()) {
            out.push(Diagnostic::new(
                span(source.span()),
                format!("`{}` {problem}", source.get_ref()),
            ));
        } else if !dir.join(source.get_ref()).is_dir()
            && !overlays
                .iter()
                .any(|file| overlay_in_source(dir, &dir.join(source.get_ref()), file))
        {
            out.push(Diagnostic::new(
                span(source.span()),
                format!(
                    "`{}` is not a directory next to this manifest",
                    source.get_ref()
                ),
            ));
        }
    }
    if let Some(at) = model.dependencies.as_ref().map(|d| d.span()) {
        out.push(Diagnostic::new(
            span(at),
            "`[model.dependencies]` is not supported by the compiler yet",
        ));
    }
    if let Some(Baseline { path }) = &model.baseline {
        let at = span(path.span());
        if let Some(problem) = escapes(path.get_ref()).or_else(|| outside(dir, path.get_ref())) {
            out.push(Diagnostic::new(
                at,
                format!("`{}` {problem}", path.get_ref()),
            ));
        } else if !path.get_ref().ends_with(".json") || !dir.join(path.get_ref()).is_file() {
            out.push(Diagnostic::new(
                at,
                format!(
                    "`{}` is not a frozen release; `crucible freeze` writes one as `releases/<version>.json`",
                    path.get_ref()
                ),
            ));
        }
    }
    let dependencies: Vec<String> = model
        .dependencies
        .as_ref()
        .map(|d| d.get_ref().keys().cloned().collect())
        .unwrap_or_default();
    for import in model.imports.unwrap_or_default() {
        if dependencies.contains(import.get_ref()) {
            out.push(Diagnostic::new(
                span(import.span()),
                format!(
                    "`{}` is a dependency package; importing one is not supported by the compiler yet",
                    import.get_ref()
                ),
            ));
        } else if !crate::compile::BUNDLED
            .iter()
            .any(|(namespace, _)| namespace == import.get_ref())
        {
            let known: Vec<&str> = crate::compile::BUNDLED
                .iter()
                .map(|(namespace, _)| *namespace)
                .collect();
            out.push(Diagnostic::new(
                span(import.span()),
                format!(
                    "unknown import `{}`; the bundled packages are {}",
                    import.get_ref(),
                    known.join(", ")
                ),
            ));
        }
    }
    if let Some(naming) = model.naming {
        for pattern in [&naming.input, &naming.output].into_iter().flatten() {
            let name = pattern.get_ref().replace("{Operation}", "Op");
            let identifier = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !pattern.get_ref().contains("{Operation}") {
                out.push(Diagnostic::new(
                    span(pattern.span()),
                    "a naming pattern must contain {Operation}, such as \"{Operation}Input\"",
                ));
            } else if !identifier {
                out.push(Diagnostic::new(
                    span(pattern.span()),
                    "a naming pattern must make a name: letters, digits and `_` around {Operation}",
                ));
            }
        }
        // Equal patterns would give an operation's input and output one name.
        let input = naming
            .input
            .as_ref()
            .map_or("{Operation}Input", |p| p.get_ref().as_str());
        let output = naming
            .output
            .as_ref()
            .map_or("{Operation}Output", |p| p.get_ref().as_str());
        if input == output {
            let at = naming
                .output
                .as_ref()
                .or(naming.input.as_ref())
                .map_or(header, |p| span(p.span()));
            out.push(Diagnostic::new(
                at,
                "the input and output naming patterns must differ, or both bodies get one name",
            ));
        }
    }
    out
}
