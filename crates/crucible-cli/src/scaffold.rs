//! `crucible scaffold`: a stub file for every operation that has none.

use std::collections::BTreeMap;
use std::path::Path;

use crucible_codegen_rust::{ScaffoldOptions, scaffold};
use crucible_smithy::ir::Service;

/// Writes the missing operation files under `out`, and the `mod` lines that
/// reach them. An existing file is never touched; an existing `mod.rs` only
/// gains the lines it lacks.
pub fn run(service: &Service, out: &Path, options: &ScaffoldOptions) -> Result<(), String> {
    let mut folders: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut written = 0;
    for stub in scaffold(service, options) {
        let path = out.join(stub.path());
        if !path.exists() {
            write(&path, &stub.contents)?;
            println!("wrote {}", path.display());
            written += 1;
        }
        folders.entry(stub.folder).or_default().push(stub.module);
    }
    for (folder, modules) in &folders {
        let heading = if folder == "service" {
            "//! Operations bound to the service itself, one file each.\n".to_owned()
        } else {
            format!("//! The `{folder}` operations, one file each.\n")
        };
        declare(&out.join(folder).join("mod.rs"), &heading, modules)?;
    }
    let folders: Vec<String> = folders.into_keys().collect();
    declare(
        &out.join("mod.rs"),
        "//! Every operation, one file each, in a folder per resource.\n",
        &folders,
    )?;
    println!("{written} operation file(s) written");
    Ok(())
}

/// Makes `path` declare every module in `modules`, creating it with
/// `heading` when it does not exist.
fn declare(path: &Path, heading: &str, modules: &[String]) -> Result<(), String> {
    let existing = std::fs::read_to_string(path).ok();
    let mut text = existing.clone().unwrap_or_else(|| format!("{heading}\n"));
    for module in modules {
        let declared = text.lines().any(|line| {
            let line = line.trim();
            let line = line
                .strip_prefix("pub(crate) ")
                .or_else(|| line.strip_prefix("pub "))
                .unwrap_or(line);
            line == format!("mod {module};")
        });
        if !declared {
            if !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(&format!("mod {module};\n"));
        }
    }
    if existing.as_deref() != Some(text.as_str()) {
        write(path, &text)?;
    }
    Ok(())
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    std::fs::write(path, text).map_err(|error| format!("cannot write {}: {error}", path.display()))
}
