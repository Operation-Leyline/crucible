//! Shared helpers: the repository's `.cru` test packages (`tests/models`).
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use crucible_ir::ir::Service;

/// The shared package `tests/models/<name>`.
pub fn package_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
}

/// The package's lowered service.
pub fn load(name: &str) -> Service {
    crucible_lang::load_service(&package_dir(name))
        .unwrap_or_else(|rendered| panic!("{name}:\n{rendered}"))
}

/// Copies the package directory `from` to `to`.
pub fn copy_package(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_package(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// The package after `edit` rewrote its sources, lowered.
pub fn load_with(name: &str, edit: impl FnOnce(&mut crucible_lang::Package)) -> Service {
    let mut package = crucible_lang::load_package(&package_dir(name)).unwrap();
    // Checkouts may carry CRLF; edits are written against LF.
    for file in &mut package.files {
        file.text = file.text.replace("\r\n", "\n");
    }
    edit(&mut package);
    let compiled = crucible_lang::compile(&package, None);
    let rendered = compiled.render();
    compiled
        .service
        .unwrap_or_else(|| panic!("{name} (edited) compiles:\n{rendered}"))
}
