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
    match compiled.service {
        Some(service) => {
            // Warnings do not fail the build; Cargo shows each one.
            for line in rendered.lines().filter(|line| line.contains(": warning: ")) {
                println!("cargo::warning={line}");
            }
            Ok(service)
        }
        None => Err(format!(
            "{} does not compile:\n{}",
            path.display(),
            rendered
        )),
    }
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
