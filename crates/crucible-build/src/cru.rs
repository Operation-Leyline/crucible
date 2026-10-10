//! Crucible language packages as model sources (`docs/idl/`).
//!
//! A directory whose `crucible.toml` has a `[model]` table is compiled by
//! `crucible-lang` straight to the IR: no Smithy compiler and no JVM. A model
//! package publishes the directory through Cargo `links`, and dependents
//! compile it again, which is cheap.

use std::path::Path;

use crucible_smithy::ir::Service;
use crucible_smithy::{Protocol, ShapeId};

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
pub(crate) fn lower(
    path: &Path,
    service: Option<&ShapeId>,
    protocol: Option<Protocol>,
) -> Result<Service, String> {
    // Cargo watches a directory recursively, so any edited `.cru` file reruns
    // the build script.
    println!("cargo::rerun-if-changed={}", path.display());
    let package = crucible_lang::load_package(path)?;
    if let Some(protocol) = protocol
        && protocol != Protocol::CrucibleJson
    {
        return Err(format!(
            "{}: a .cru package declares its protocol with `protocol ..` in the service; @{} cannot be selected",
            path.display(),
            protocol.trait_id()
        ));
    }
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
    // Warnings do not fail the build; Cargo shows each one.
    for line in rendered.lines().filter(|line| line.contains(": warning: ")) {
        println!("cargo::warning={line}");
    }
    check_baseline(path, &package, &compiled)?;
    // The older versions the service still serves, for generators that
    // serve them (docs/api-versions.md).
    let versions = crucible_lang::versions(&package, &compiled, package.files.len())?;
    let current = compiled.service.as_ref().expect("checked above");
    for version in &versions {
        for operation in &version.service.operations {
            let streams = current
                .operations
                .iter()
                .find(|op| op.id == operation.id)
                .is_some_and(|op| {
                    [&op.request, &op.response].iter().any(|bindings| {
                        bindings.event_stream.is_some()
                            || bindings.payload.as_ref().is_some_and(|p| p.streaming)
                    })
                });
            if streams {
                println!(
                    "cargo::warning=API version {}: {} streams, so it is served at the current version only",
                    version.name,
                    operation.name()
                );
            }
        }
    }
    let mut service = compiled.service.expect("checked above");
    service.versions = versions;
    Ok(service)
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
    if let Some(dir) = &package.baseline {
        println!("cargo::rerun-if-changed={}", dir.display());
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
