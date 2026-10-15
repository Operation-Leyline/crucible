//! Crucible language packages as model sources (`docs/idl/`).
//!
//! A directory whose `crucible.toml` has a `[model]` table is compiled by
//! `crucible-lang` straight to the IR: no Smithy compiler and no JVM. A model
//! package publishes the directory through Cargo `links`, and dependents
//! compile it again, which is cheap.

use std::path::Path;

use crucible_smithy::ShapeId;
use crucible_smithy::ir::Service;

/// Whether `path` is a language package rather than Smithy sources or a JSON AST.
pub(crate) fn is_package(path: &Path) -> bool {
    path.is_dir() && crucible_lang::is_package(path)
}

/// The package's own `crucible.toml`, when it carries policy beside
/// `[model]`: the policy that package's builds apply.
pub(crate) fn manifest_policy(path: &Path) -> Option<std::path::PathBuf> {
    let manifest = path.join("crucible.toml");
    let text = std::fs::read_to_string(&manifest).ok()?;
    crucible_policy::Config::declares_policy(&text).then_some(manifest)
}

/// Compiles the package at `path` and lowers `service`, or its only service.
pub(crate) fn lower(path: &Path, service: Option<&ShapeId>) -> Result<Service, String> {
    // Cargo watches a directory recursively, so any edited `.cru` file reruns
    // the build script.
    println!("cargo::rerun-if-changed={}", path.display());
    let package = crucible_lang::load_package(path)?;
    if let Some(id) = service
        && id.namespace() != package.namespace
    {
        return Err(format!(
            "{}: the package namespace is {}, so it has no service {id}",
            path.display(),
            package.namespace
        ));
    }
    let compiled = crucible_lang::compile(&package, service.map(ShapeId::name));
    let rendered = compiled.render();
    if compiled.service.is_none() {
        return Err(format!("{} does not compile:\n{rendered}", path.display()));
    }
    // Warnings and advice do not fail the build; Cargo shows each one.
    for line in rendered
        .lines()
        .filter(|line| line.contains(": warning: ") || line.contains(": advice: "))
    {
        println!("cargo::warning={line}");
    }
    check_baseline(path, &package, &compiled)?;
    // The older versions the service still serves, for generators that
    // serve them (docs/api-versions.md).
    let versions = crucible_lang::versions(&package, &compiled, package.files.len())?;
    let mut service = compiled.service.expect("checked above");
    service.versions = versions;
    Ok(service)
}

/// Writes the package's compiled model to `out` in the release snapshot
/// format, with documentation and custom metadata: what a tool reading the
/// contract needs and `generate ts`'s `crucible-model.json` leaves out. The
/// file is rewritten only when it changes, so a watched export does not
/// rerun the build that wrote it.
pub(crate) fn export(path: &Path, service: Option<&ShapeId>, out: &Path) -> Result<(), String> {
    let model = lower(path, service)?;
    let compiler = format!("crucible-build {}", env!("CARGO_PKG_VERSION"));
    let text = crucible_smithy::snapshot::write(&model, &compiler);
    if std::fs::read_to_string(out).is_ok_and(|current| current == text) {
        return Ok(());
    }
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    std::fs::write(out, text)
        .map_err(|error| format!("cannot export the model to {}: {error}", out.display()))
}

/// The compatibility guard (decision 47d): the package against its frozen
/// baseline, with the `[compatibility]` settings of its manifest. A breaking
/// change no migration declares fails the build, as does a migration step
/// that does not match what changed; a finding the policy makes a warning
/// is shown.
fn check_baseline(
    path: &Path,
    package: &crucible_lang::Package,
    compiled: &crucible_lang::Compiled,
) -> Result<(), String> {
    let config = match manifest_policy(path) {
        Some(manifest) => crucible_policy::Config::load(&manifest)?,
        None => crucible_policy::Config::default(),
    };
    let Some(guard) = crucible_lang::guard(package, compiled, package.files.len(), &config) else {
        return Ok(());
    };
    // The releases directory: the baseline, older releases and their lock.
    if let Some(releases) = package
        .baseline
        .as_deref()
        .and_then(std::path::Path::parent)
    {
        println!("cargo::rerun-if-changed={}", releases.display());
    }
    let guard = guard?;
    if guard.has_errors() {
        return Err(format!(
            "{} breaks compatibility with its baseline {}:\n{}",
            path.display(),
            guard.version,
            guard.render(compiled)
        ));
    }
    for line in guard.render(compiled).lines() {
        println!("cargo::warning={line}");
    }
    Ok(())
}

/// The error for an output the language cannot feed yet.
#[cfg(any(
    feature = "rust",
    feature = "docs",
    feature = "cdk",
    feature = "terraform"
))]
pub(crate) fn unsupported(what: &str, path: &Path) -> String {
    format!(
        "{what} from a .cru package ({}) is not supported yet: the language does not lower resources for documentation or infrastructure",
        path.display()
    )
}
