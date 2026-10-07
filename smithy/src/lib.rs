//! Crucible's Smithy definitions, shared with the `crucible-traits` model JAR.
//!
//! A Cargo dependency downloads the definitions. For IDEs that do not resolve
//! Smithy files in Cargo libraries, a model package's build script can write
//! them into its own source root:
//!
//! ```no_run
//! crucible_traits::write_model("src/smithy-ide/crucible.smithy")?;
//! # Ok::<(), std::io::Error>(())
//! ```

use std::io;
use std::path::Path;

/// The Cargo package's Crucible version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The complete `crucible.protocols` model, including `json` and `strict`.
pub const MODEL: &str = include_str!("../crucible.smithy");

/// Writes the bundled model to `path`, creating parent directories as needed.
///
/// Existing files are replaced when their content differs. An unchanged model
/// is left alone so subsequent builds do not trigger needless IDE reindexing.
/// Use a path under the model package's source root for IDE indexing, or pass
/// the resulting file to a Smithy build as a model import.
pub fn write_model(path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref();
    match std::fs::read(path) {
        Ok(existing) if existing == MODEL.as_bytes() => return Ok(()),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, MODEL)
}
