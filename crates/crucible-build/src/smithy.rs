use std::io;
use std::path::{Path, PathBuf};

#[cfg(feature = "rust")]
use crate::ServerOutput;
use crate::absolute;

/// Assemble and validate Smithy sources, then generate a server in `OUT_DIR`.
///
/// The pinned official compiler imports Crucible's protocols and standard errors
/// automatically. Its matching Java runtime is downloaded and checksum-verified
/// on first use, then cached outside Cargo's build directory. No separate CLI or
/// Java installation is required. Sources and imports are never written into the
/// project. The service must declare a fillable validation error, or use
/// [`ServerOutput::custom_rejections`] and supply its own rejection handler.
#[cfg(all(feature = "smithy", feature = "rust"))]
#[deprecated(
    since = "0.5.0",
    note = "use crucible_build::model(..).server(), .client(), or .publish()"
)]
#[allow(deprecated)]
pub fn rust_service_from_smithy(sources: impl AsRef<Path>) -> ServerOutput {
    crate::Model::from_sources(sources).server()
}

/// Use an explicitly selected compiler dependency instead of the default pin.
///
/// Disable `crucible-build`'s default features, depend on an immutable
/// `smithy-tooling` version, and pass `|config, out| smithy_tooling::build(config, out)`.
/// The compiler must support the `flattenAndRemoveMixins` transform. Assembly and
/// validation remain the official compiler's responsibility.
#[cfg(feature = "rust")]
#[deprecated(
    since = "0.5.0",
    note = "use crucible_build::model(..).server(), .client(), or .publish()"
)]
#[allow(deprecated)]
pub fn rust_service_from_smithy_with(
    sources: impl AsRef<Path>,
    compiler: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> ServerOutput {
    let mut codegen = crate::model(compile(sources.as_ref(), compiler)).server();
    codegen.options.require_rejection = true;
    codegen
}

/// Assemble Smithy sources and generate a Rust client in `OUT_DIR`.
///
/// Uses the same pinned compiler, bundled imports and managed Java runtime as
/// [`rust_service_from_smithy`]. Client generation does not require server rejection errors.
#[cfg(all(feature = "smithy", feature = "rust"))]
#[deprecated(
    since = "0.5.0",
    note = "use crucible_build::model(..).server(), .client(), or .publish()"
)]
#[allow(deprecated)]
pub fn client_from_smithy(sources: impl AsRef<Path>) -> ServerOutput {
    crate::Model::from_sources(sources).client().into_legacy()
}

/// Generate a Rust client with an explicitly selected official compiler.
///
/// See [`rust_service_from_smithy_with`] for compiler dependency setup.
#[cfg(feature = "rust")]
#[deprecated(
    since = "0.5.0",
    note = "use crucible_build::model(..).server(), .client(), or .publish()"
)]
#[allow(deprecated)]
pub fn client_from_smithy_with(
    sources: impl AsRef<Path>,
    compiler: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> ServerOutput {
    crate::model(compile(sources.as_ref(), compiler))
        .client()
        .into_legacy()
}

/// Compile and publish Smithy sources from a shared model package's `build.rs`.
///
/// The package must declare `links`. Dependents use [`crate::rust_service`] or
/// [`crate::client`] with that name. The published AST lives under `OUT_DIR`
/// and is rewritten only when its contents change.
#[cfg(feature = "smithy")]
#[deprecated(
    since = "0.5.0",
    note = "use crucible_build::model(..).server(), .client(), or .publish()"
)]
#[allow(deprecated)]
pub fn publish_model_from_smithy(sources: impl AsRef<Path>) {
    crate::Model::from_sources(sources).publish();
}

/// Compile and publish a shared model using an explicitly selected compiler.
///
/// See [`rust_service_from_smithy_with`] for compiler dependency setup.
#[deprecated(
    since = "0.5.0",
    note = "use crucible_build::model(..).server(), .client(), or .publish()"
)]
#[allow(deprecated)]
pub fn publish_model_from_smithy_with(
    sources: impl AsRef<Path>,
    compiler: impl FnOnce(&Path, &Path) -> io::Result<()>,
) {
    crate::model(compile(sources.as_ref(), compiler)).publish();
}

fn compile(sources: &Path, compiler: impl FnOnce(&Path, &Path) -> io::Result<()>) -> PathBuf {
    let source = absolute(sources);
    println!("cargo::rerun-if-changed={}", source.display());
    let root =
        PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR: run from a build script"))
            .join("smithy");
    compile_smithy_with(source, root, compiler).unwrap_or_else(|error| panic!("crucible: {error}"))
}

/// Assemble a Smithy source file or directory into a validated JSON AST.
///
/// The output directory holds compiler-owned intermediate files and a stable
/// `model.json`. It must be separate from the model sources. Uses the pinned
/// official compiler, bundled Crucible imports, and `flattenAndRemoveMixins`.
/// Unlike the Cargo helpers, paths resolve from the current working directory
/// and no Cargo environment variables are required.
#[cfg(feature = "smithy")]
pub fn compile_smithy(sources: impl AsRef<Path>, output: impl AsRef<Path>) -> io::Result<PathBuf> {
    compile_smithy_with(sources, output, |config, compiled| {
        smithy_tooling::build(config, compiled)
    })
}

/// Assemble sources with an explicitly selected official Smithy compiler.
///
/// See [`rust_service_from_smithy_with`] for compiler dependency setup. Compiler diagnostics
/// and filesystem errors are returned to the caller.
pub fn compile_smithy_with(
    sources: impl AsRef<Path>,
    output: impl AsRef<Path>,
    compiler: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> io::Result<PathBuf> {
    let source = std::fs::canonicalize(sources.as_ref()).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "cannot read Smithy sources {}: {error}",
                sources.as_ref().display()
            ),
        )
    })?;
    let output = std::path::absolute(output)?;
    let ancestor = output
        .ancestors()
        .find_map(|path| std::fs::canonicalize(path).ok());
    if source.is_dir() && ancestor.is_some_and(|path| path.starts_with(&source)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Smithy compilation output must be outside the source directory",
        ));
    }
    std::fs::create_dir_all(&output)?;
    let root = std::fs::canonicalize(&output)?;
    let imports = root.join("imports");
    crucible_traits::write_models(&imports)?;
    let config = root.join("smithy-build.json");
    let contents = serde_json::to_vec_pretty(&serde_json::json!({
        "version": "1.0", "sources": [source],
        "imports": [imports.join("crucible.smithy"), imports.join("errors.smithy"), imports.join("auth.smithy")],
        "projections": {"crucible": {
            "transforms": [{"name": "flattenAndRemoveMixins"}], "plugins": {"model": {}}
        }}
    }))?;
    write_changed(&config, &contents)?;
    let output = root.join("compiled");
    compiler(&config, &output)?;
    // Smithy rewrites its output even when the model is unchanged. Publish a
    // stable copy so Cargo dependents can reuse their generated contracts.
    let model = root.join("model.json");
    let contents = std::fs::read(output.join("crucible/model/model.json"))?;
    write_changed(&model, &contents)?;
    Ok(model)
}

fn write_changed(path: &Path, contents: &[u8]) -> io::Result<()> {
    match std::fs::read(path) {
        Ok(existing) if existing == contents => return Ok(()),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    std::fs::write(path, contents)
}
