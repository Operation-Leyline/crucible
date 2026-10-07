use std::io;
use std::path::{Path, PathBuf};

use crate::{Codegen, absolute};

/// Assemble and validate Smithy sources, then generate a server in `OUT_DIR`.
///
/// The pinned official compiler imports Crucible's protocols and standard errors
/// automatically. Its matching Java runtime is downloaded and checksum-verified
/// on first use, then cached outside Cargo's build directory. No separate CLI or
/// Java installation is required. Sources and imports are never written into the
/// project. The service must declare a fillable validation error, or use
/// [`Codegen::custom_rejections`] and supply its own rejection handler.
#[cfg(feature = "smithy")]
pub fn from_smithy(sources: impl AsRef<Path>) -> Codegen {
    from_smithy_with(sources, |config, output| {
        smithy_tooling::build(config, output)
    })
}

/// Use an explicitly selected compiler dependency instead of the default pin.
///
/// Disable `crucible-build`'s default features, depend on an immutable
/// `smithy-tooling` version, and pass `|config, out| smithy_tooling::build(config, out)`.
/// The compiler must support the `flattenAndRemoveMixins` transform. Assembly and
/// validation remain the official compiler's responsibility.
pub fn from_smithy_with(
    sources: impl AsRef<Path>,
    compiler: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> Codegen {
    let source = absolute(sources.as_ref());
    if !source.is_dir() {
        panic!(
            "crucible: Smithy source directory does not exist: {}",
            source.display()
        );
    }
    println!("cargo::rerun-if-changed={}", source.display());
    let root =
        PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR: run from a build script"))
            .join("smithy");
    let imports = root.join("imports");
    crucible_traits::write_models(&imports).expect("crucible: cannot write bundled Smithy imports");
    let config = root.join("smithy-build.json");
    let contents = serde_json::to_vec_pretty(&serde_json::json!({
        "version": "1.0", "sources": [source],
        "imports": [imports.join("crucible.smithy"), imports.join("errors.smithy")],
        "projections": {"crucible": {
            "transforms": [{"name": "flattenAndRemoveMixins"}], "plugins": {"model": {}}
        }}
    }))
    .expect("crucible: cannot encode Smithy configuration");
    if std::fs::read(&config).ok().as_deref() != Some(&contents) {
        std::fs::write(&config, contents).expect("crucible: cannot write Smithy configuration");
    }
    let output = root.join("compiled");
    compiler(&config, &output).unwrap_or_else(|error| panic!("crucible: {error}"));
    let mut codegen = Codegen::from_model(output.join("crucible/model/model.json"));
    codegen.options.require_rejection = true;
    codegen
}
