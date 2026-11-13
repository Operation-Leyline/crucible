//! Dating files a build script both writes and watches.
//!
//! Cargo reruns a build script when a `rerun-if-changed` path is newer than
//! the moment Cargo started the script's last run. Restoring a deleted export
//! needs the export watched, so an export this run writes would rerun the
//! next build unless it is dated no later than that moment. Cargo does not
//! pass the moment to the script; it keeps it as the modification time of
//! `invoked.timestamp` in the script's build directory, next to `OUT_DIR`.
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Where Cargo keeps the stamp, relative to `OUT_DIR`'s parent. Cargo 1.99
/// and older keep it beside `OUT_DIR` (`build/<package>-<hash>/`). From
/// 1.100, the build-directory layout gives each build script its own
/// directory (`build/<package>/<hash>/`), with the run's records under
/// `run/`.
const STAMPS: [&str; 2] = ["invoked.timestamp", "run/invoked.timestamp"];

/// When Cargo started this build script, if it can be found. Without it the
/// exports are still correct; the next build only reruns the script once.
/// A build system that sets `OUT_DIR` without Cargo's stamp gets that
/// behaviour, so it is not reported. `tests/native_exports.rs` builds with
/// the real Cargo and fails if a layout change hides the stamp again.
pub(crate) fn started() -> Option<SystemTime> {
    let out = PathBuf::from(std::env::var_os("OUT_DIR")?);
    let unit = out.parent()?;
    STAMPS.iter().find_map(|stamp| {
        std::fs::metadata(unit.join(stamp))
            .and_then(|meta| meta.modified())
            .ok()
    })
}

/// Dates `path`, which this run has just written, at `started`. Downstream
/// builds still see the change, because they ran before this script started.
/// This script's next build sees no change.
pub(crate) fn settle(path: &Path, started: SystemTime) -> Result<(), String> {
    let modified = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .map_err(|error| format!("cannot date the export {}: {error}", path.display()))?;
    if modified <= started {
        return Ok(());
    }
    std::fs::File::options()
        .write(true)
        .open(path)
        .and_then(|file| file.set_modified(started))
        .map_err(|error| format!("cannot date the export {}: {error}", path.display()))
}
