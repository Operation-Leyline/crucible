use std::path::Path;
use std::process::Command;

pub(crate) fn read(path: &Path) -> Result<String, String> {
    if !path.is_dir() {
        return std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()));
    }
    let temporary = tempfile::tempdir().map_err(|error| error.to_string())?;
    let model = temporary.path().join("model.json");
    let output = Command::new(std::env::current_exe().map_err(|error| error.to_string())?)
        .arg("_assemble-model")
        .arg(path.canonicalize().map_err(|error| error.to_string())?)
        .arg("--out")
        .arg(&model)
        .output()
        .map_err(|error| format!("cannot assemble {}: {error}", path.display()))?;
    if !output.status.success() {
        return Err(format!(
            "cannot assemble {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    std::fs::read_to_string(model).map_err(|error| error.to_string())
}

pub(crate) fn assemble(source: &Path, out: &Path) -> Result<(), String> {
    let source = source.canonicalize().map_err(|error| error.to_string())?;
    if !source.is_dir() {
        return Err(format!("{} is not a Smithy directory", source.display()));
    }
    let temporary = tempfile::tempdir().map_err(|error| error.to_string())?;
    let root = temporary.path();
    let imports = root.join("imports");
    crucible_traits::write_models(&imports).map_err(|error| error.to_string())?;
    let config = root.join("smithy-build.json");
    let contents = serde_json::to_vec_pretty(&serde_json::json!({
        "version": "1.0", "sources": [source],
        "imports": [imports.join("crucible.smithy"), imports.join("errors.smithy")],
        "projections": {"crucible": {
            "transforms": [{"name": "flattenAndRemoveMixins"}], "plugins": {"model": {}}
        }}
    }))
    .map_err(|error| error.to_string())?;
    std::fs::write(&config, contents).map_err(|error| error.to_string())?;
    let compiled = root.join("compiled");
    smithy_tooling::build(&config, &compiled).map_err(|error| error.to_string())?;
    std::fs::copy(compiled.join("crucible/model/model.json"), out)
        .map_err(|error| error.to_string())?;
    Ok(())
}
