//! The examples are the house style: formatting leaves every one unchanged.

use std::path::Path;

use crucible_lang::{FormatOptions, format};

fn cru_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            cru_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "cru") {
            out.push(path);
        }
    }
}

#[test]
fn every_example_is_already_formatted() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/idl/examples");
    let mut files = Vec::new();
    cru_files(&root, &mut files);
    files.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("bundled/crucible.errors.cru"));
    assert!(files.len() >= 6);
    for path in files {
        let source = std::fs::read_to_string(&path).unwrap();
        let formatted = format(&source, &FormatOptions::default()).unwrap();
        assert!(
            formatted == source,
            "{} is not formatted:\n{formatted}",
            path.display()
        );
    }
}
