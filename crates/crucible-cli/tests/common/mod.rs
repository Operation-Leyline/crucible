//! Shared helpers for the CLI tests.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

/// The repository's shared `.cru` test packages (`tests/models/<name>`).
pub fn shared_package(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
}

/// Copies a package directory so a test can edit or build it on its own.
pub fn copy_package(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_package(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Copies a shared package to `<root>/<name>`.
pub fn install_shared(root: &Path, name: &str) {
    copy_package(&shared_package(name), &root.join(name));
}
