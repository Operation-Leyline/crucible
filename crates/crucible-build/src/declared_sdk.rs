use std::path::{Path, PathBuf};

/// A build script may not configure an SDK the service declares in
/// `crucible-sdk.toml` (docs/sdk-packaging.md, "Build scripts"): the
/// declaration is the one configuration `crucible sdk` generates and
/// releases, so the build script and the release cannot drift.
///
/// Only the producer's own repository is searched, and only a file whose
/// `model` is this builder's model source counts. The file is read
/// leniently, for its `model` and SDK names: whether it is otherwise valid
/// is `crucible sdk`'s to report, never a reason to fail an unrelated build.
pub(crate) fn refuse(name: &str, source: &Path) -> Result<(), String> {
    let start =
        std::env::var_os("CARGO_MANIFEST_DIR").map_or_else(|| PathBuf::from("."), PathBuf::from);
    let Some(path) = crucible_codegen::sdk::find_in_repository(&start) else {
        return Ok(());
    };
    println!("cargo::rerun-if-changed={}", path.display());
    let Some(declared) = crucible_codegen::sdk::declared(&path) else {
        return Ok(());
    };
    if !declared.names.iter().any(|declared| declared == name) {
        return Ok(());
    }
    let same = |a: &Path, b: &Path| match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    let declares_this_model = declared
        .model
        .is_some_and(|model| same(&path.parent().unwrap_or(Path::new(".")).join(model), source));
    if declares_this_model {
        return Err(format!(
            "output {name:?} is declared in {} as [sdk.{name}]: `crucible sdk generate` writes a declared SDK, not a build script. Remove this builder call, or name a builder-only SDK differently",
            path.display()
        ));
    }
    Ok(())
}
