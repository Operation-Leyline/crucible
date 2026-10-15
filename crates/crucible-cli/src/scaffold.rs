//! `crucible scaffold`: a stub file for every operation that has none.

use std::collections::BTreeMap;
use std::path::Path;

use crucible_codegen_rust::{ScaffoldOptions, escape_identifier, scaffold};
use crucible_ir::ir::Service;

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
        declare(&out.join(folder).join("mod.rs"), &heading, modules, None)?;
    }
    let folders: Vec<String> = folders
        .into_keys()
        .map(|folder| escape_identifier(&folder))
        .collect();
    let placeholder =
        (options.service_type == "crate::operations::Service").then_some("pub struct Service;");
    declare(
        &out.join("mod.rs"),
        "//! Every operation, one file each, in a folder per resource.\n",
        &folders,
        placeholder,
    )?;
    println!("{written} operation file(s) written");
    Ok(())
}

/// Makes `path` declare every module in `modules`, creating it with
/// `heading` when it does not exist. When creating anew and `placeholder`
/// is supplied, it is appended to the file.
fn declare(
    path: &Path,
    heading: &str,
    modules: &[String],
    placeholder: Option<&str>,
) -> Result<(), String> {
    let existing = match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    let is_new = existing.is_none();
    let mut text = existing.clone().unwrap_or_else(|| format!("{heading}\n"));
    for module in modules {
        let declared = text.lines().any(|line| is_module_declared(line, module));
        if !declared {
            if !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(&format!("mod {module};\n"));
        }
    }
    if is_new {
        if let Some(placeholder) = placeholder {
            if !text.ends_with("\n\n") {
                if !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push('\n');
            }
            text.push_str(placeholder);
            text.push('\n');
        }
    }
    if existing.as_deref() != Some(text.as_str()) {
        write(path, &text)?;
    }
    Ok(())
}

fn strip_inline_comments(mut line: &str) -> String {
    if let Some((before, _)) = line.split_once("//") {
        line = before;
    }
    let mut out = String::new();
    let mut rest = line;
    while let Some((before, after)) = rest.split_once("/*") {
        out.push_str(before);
        if let Some((_, remaining)) = after.split_once("*/") {
            rest = remaining;
        } else {
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out
}

fn is_module_declared(line: &str, module: &str) -> bool {
    let line = strip_inline_comments(line);
    let line = line.trim();
    let Some(line) = line.strip_suffix(';') else {
        return false;
    };
    let line = line.trim();

    let rest = if let Some(after_pub) = line.strip_prefix("pub") {
        if let Some(after_paren) = after_pub.strip_prefix('(') {
            if let Some((_, remaining)) = after_paren.split_once(')') {
                remaining.trim_start()
            } else {
                return false;
            }
        } else if after_pub.starts_with(char::is_whitespace) {
            after_pub.trim_start()
        } else {
            return false;
        }
    } else {
        line
    };

    let Some(after_mod) = rest.strip_prefix("mod") else {
        return false;
    };
    if !after_mod.starts_with(char::is_whitespace) {
        return false;
    }
    let declared = after_mod.trim();
    declared == module
        || declared.strip_prefix("r#").unwrap_or(declared)
            == module.strip_prefix("r#").unwrap_or(module)
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    std::fs::write(path, text).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(prefix: &str) -> Self {
            let name = format!(
                "{prefix}-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            let path = std::env::temp_dir().join(name);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn is_module_declared_matches_various_declarations() {
        assert!(is_module_declared("mod city;", "city"));
        assert!(is_module_declared("pub mod city;", "city"));
        assert!(is_module_declared("pub(crate) mod city;", "city"));
        assert!(is_module_declared("pub(super) mod city;", "city"));
        assert!(is_module_declared("pub(in crate::sub) mod city;", "city"));
        assert!(is_module_declared("mod city; // inline comment", "city"));
        assert!(is_module_declared("pub mod city; /* block */", "city"));
        assert!(is_module_declared("mod city ;", "city"));
        assert!(is_module_declared("  mod city;  ", "city"));
        assert!(is_module_declared("mod r#type;", "r#type"));
        assert!(is_module_declared("mod r#type;", "type"));
        assert!(is_module_declared("mod type;", "r#type"));
        assert!(is_module_declared(
            "pub(super) mod r#type; // comment",
            "r#type"
        ));

        assert!(!is_module_declared("mod city_other;", "city"));
        assert!(!is_module_declared("// mod city;", "city"));
        assert!(!is_module_declared("/* mod city; */", "city"));
        assert!(!is_module_declared("pub struct Service;", "service"));
        assert!(!is_module_declared("use crate::city;", "city"));
        assert!(!is_module_declared("fn modify();", "city"));
        assert!(!is_module_declared("pubmod city;", "city"));
    }

    #[test]
    fn declare_creates_new_file_with_placeholder() {
        let dir = TempDir::new("crucible-test-declare");
        let path = dir.path().join("mod.rs");
        let modules = vec!["city".to_owned(), "r#type".to_owned()];
        declare(
            &path,
            "//! Operations.\n",
            &modules,
            Some("pub struct Service;"),
        )
        .unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            contents,
            "//! Operations.\n\nmod city;\nmod r#type;\n\npub struct Service;\n"
        );
    }

    #[test]
    fn declare_creates_new_file_without_placeholder() {
        let dir = TempDir::new("crucible-test-declare-none");
        let path = dir.path().join("mod.rs");
        let modules = vec!["city".to_owned()];
        declare(&path, "//! Operations.\n", &modules, None).unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "//! Operations.\n\nmod city;\n");
    }

    #[test]
    fn declare_preserves_existing_file_and_inline_comments() {
        let dir = TempDir::new("crucible-test-declare-preserve");
        let path = dir.path().join("mod.rs");
        std::fs::write(
            &path,
            "//! Custom header.\n\npub(super) mod city; // custom\n",
        )
        .unwrap();
        let modules = vec!["city".to_owned(), "status".to_owned()];
        declare(
            &path,
            "//! Header.\n",
            &modules,
            Some("pub struct Service;"),
        )
        .unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            contents,
            "//! Custom header.\n\npub(super) mod city; // custom\nmod status;\n"
        );
    }

    #[test]
    fn declare_propagates_read_errors_other_than_not_found() {
        let dir = TempDir::new("crucible-test-declare-err");
        let path = dir.path().join("mod_dir");
        std::fs::create_dir(&path).unwrap();
        let result = declare(&path, "//! Header.\n", &[], None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot read"));
    }

    #[test]
    fn run_scaffolds_keyword_operations_and_resources_with_default_service_struct() {
        let dir = TempDir::new("crucible-test-scaffold-keywords");
        let package = dir.path().join("package");
        std::fs::create_dir(&package).unwrap();
        for (name, text) in [
            (
                "crucible.toml",
                "schema_version = 1

[model]
namespace = \"example\"
language = 1
sources = [\".\"]
imports = []
",
            ),
            (
                "model.cru",
                "service KeywordService {
    version \"2026-01-01\"
    protocol json
    operations [Type, Self]
    resources [Match]
}

resource Match {
}

readonly operation Type {
    http GET \"/type\"
}

readonly operation Self {
    http GET \"/self\"
}

readonly operation Return on Match collection {
    http GET \"/return\"
}

readonly operation Move on Match collection {
    http GET \"/move\"
}
",
            ),
        ] {
            std::fs::write(package.join(name), text).unwrap();
        }
        let service = crucible_lang::load_service(&package).unwrap();
        let options = ScaffoldOptions::default();
        run(&service, dir.path(), &options).unwrap();

        // 1. Root operations/mod.rs declares escaped resource modules and Service placeholder struct
        let root_mod = std::fs::read_to_string(dir.path().join("mod.rs")).unwrap();
        assert!(root_mod.contains("mod r#match;"));
        assert!(root_mod.contains("mod service;"));
        assert!(root_mod.contains("pub struct Service;"));

        // 2. Resource match/mod.rs declares escaped operation modules
        let match_mod = std::fs::read_to_string(dir.path().join("match/mod.rs")).unwrap();
        assert!(match_mod.contains("mod r#return;"));
        assert!(match_mod.contains("mod r#move;"));

        // 3. Service service/mod.rs declares escaped operation modules
        let service_mod = std::fs::read_to_string(dir.path().join("service/mod.rs")).unwrap();
        assert!(service_mod.contains("mod r#type;"));
        assert!(service_mod.contains("mod self_;"));

        // 4. File paths on disk separate file stems from Rust identifiers
        assert!(dir.path().join("service/type.rs").exists());
        assert!(dir.path().join("service/self_.rs").exists());
        assert!(dir.path().join("match/return.rs").exists());
        assert!(dir.path().join("match/move.rs").exists());

        // 5. Running scaffold again is idempotent and doesn't duplicate lines
        run(&service, dir.path(), &options).unwrap();
        let root_mod_again = std::fs::read_to_string(dir.path().join("mod.rs")).unwrap();
        assert_eq!(root_mod, root_mod_again);
    }
}
