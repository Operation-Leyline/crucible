//! Shared helpers: the repository's `.cru` test packages (`tests/models`).
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use crucible_ir::ir::Service;
use crucible_lang::{Package, SourceFile};

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

/// The package after `edit` rewrote its sources, lowered.
pub fn load_with(name: &str, edit: impl FnOnce(&mut Package)) -> Service {
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

/// The package with `text` added as one more source file.
pub fn load_plus(name: &str, text: &str) -> Service {
    load_with(name, |package| {
        package.files.push(SourceFile {
            path: "added.cru".into(),
            text: text.into(),
        })
    })
}

/// The package with `old` replaced by `new` in whichever file holds it.
pub fn load_replacing(name: &str, old: &str, new: &str) -> Service {
    load_with(name, |package| {
        let mut found = false;
        for file in &mut package.files {
            if file.text.contains(old) {
                file.text = file.text.replace(old, new);
                found = true;
            }
        }
        assert!(found, "{name} has no `{old}`");
    })
}

/// Service-local renames of shapes, by id.
pub fn rename(service: &mut Service, renames: &[(&str, &str)]) {
    for (id, name) in renames {
        service
            .renames
            .insert(id.parse().unwrap(), (*name).to_owned());
    }
}

/// A package written in the test: `namespace` and its `.cru` sources.
pub fn from_source(namespace: &str, sources: &[&str]) -> Service {
    let files = sources
        .iter()
        .enumerate()
        .map(|(index, text)| SourceFile {
            path: format!("source{index}.cru"),
            text: (*text).to_owned(),
        })
        .collect();
    let package = Package::new(namespace, files);
    let compiled = crucible_lang::compile(&package, None);
    let rendered = compiled.render();
    compiled
        .service
        .unwrap_or_else(|| panic!("the source compiles:\n{rendered}"))
}
