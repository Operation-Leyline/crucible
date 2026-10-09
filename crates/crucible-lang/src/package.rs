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

use std::path::{Path, PathBuf};

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
    let manifest = dir.join("crucible.toml");
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
    let mut paths = Vec::new();
    for source in &sources {
        let root = dir.join(source);
        if !root.is_dir() {
            return Err(format!(
                "{}: {} is not a directory",
                field("sources"),
                root.display()
            ));
        }
        collect_cru(&root, &mut paths)?;
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
            let text = std::fs::read_to_string(&path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
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
    })
}

fn collect_cru(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|error| format!("cannot read {}: {error}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            collect_cru(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "cru") {
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
pub fn check_manifest(dir: &Path, text: &str) -> Vec<Diagnostic> {
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
        // Reserved for the language version; accepted so manifests can pin it.
        #[allow(dead_code)]
        language: Option<toml::Value>,
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
    let Some(model) = manifest.model else {
        return Vec::new();
    };
    let header = span(model.span());
    let model = model.into_inner();
    let mut out = Vec::new();
    match &model.namespace {
        None => out.push(Diagnostic::new(
            header,
            "[model] must name the package namespace: namespace = \"example.service\"",
        )),
        Some(namespace) => {
            let valid = namespace.get_ref().split('.').all(|part| {
                part.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                    && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            });
            if !valid {
                out.push(Diagnostic::new(
                    span(namespace.span()),
                    "a namespace is dot-separated identifiers, such as `example.service`",
                ));
            }
        }
    }
    let sources = model.sources.unwrap_or_default();
    for source in &sources {
        if !dir.join(source.get_ref()).is_dir() {
            out.push(Diagnostic::new(
                span(source.span()),
                format!(
                    "`{}` is not a directory next to this manifest",
                    source.get_ref()
                ),
            ));
        }
    }
    for import in model.imports.unwrap_or_default() {
        if !crate::compile::BUNDLED
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
        for pattern in [naming.input, naming.output].into_iter().flatten() {
            if !pattern.get_ref().contains("{Operation}") {
                out.push(Diagnostic::new(
                    span(pattern.span()),
                    "a naming pattern must contain {Operation}, such as \"{Operation}Input\"",
                ));
            }
        }
    }
    out
}
