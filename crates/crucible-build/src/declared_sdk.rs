use std::path::PathBuf;

/// A build script may not configure an SDK the service declares in
/// `crucible-sdk.toml` (docs/sdk-packaging.md, "Build scripts"): the
/// declaration is the one configuration `crucible sdk` generates and
/// releases, so the build script and the release cannot drift. The file is
/// the one `crucible sdk` finds from the producer's directory.
pub(crate) fn refuse(name: &str) -> Result<(), String> {
    let start =
        std::env::var_os("CARGO_MANIFEST_DIR").map_or_else(|| PathBuf::from("."), PathBuf::from);
    let Some(path) = crucible_codegen::sdk::find(&start) else {
        return Ok(());
    };
    println!("cargo::rerun-if-changed={}", path.display());
    let file = crucible_codegen::sdk::load(&path).map_err(|errors| errors.to_string())?;
    if file.sdks.contains_key(name) {
        return Err(format!(
            "output {name:?} is declared in {} as [sdk.{name}]: `crucible sdk generate` writes a declared SDK, not a build script. Remove this builder call, or name a builder-only SDK differently",
            path.display()
        ));
    }
    Ok(())
}
