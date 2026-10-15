//! A `.cru` model with recursive shapes (a union recurring directly and
//! through a list, a structure recurring through a list and a map) generates
//! a server module and an SDK that compile: recursive members are boxed
//! where Rust needs them, and validation walks the recursion.
#[path = "../../../tests/support/cargo_target.rs"]
mod cargo_target;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crucible_codegen_rust::{
    ClientOptions, Options, RuntimeDependency, emit_client_package, generate,
};

const RECURSIVE: &str = include_str!("../../crucible-lang/tests/fixtures/recursive.cru");

fn service() -> crucible_ir::ir::Service {
    let package = crucible_lang::Package {
        namespace: "example.search".to_owned(),
        files: vec![crucible_lang::SourceFile {
            path: "recursive.cru".to_owned(),
            text: RECURSIVE.to_owned(),
        }],
        naming: Default::default(),
        imports: Vec::new(),
        dependencies: Vec::new(),
        dir: None,
        baseline: None,
    };
    let compiled = crucible_lang::compile(&package, None);
    let messages: Vec<_> = compiled
        .diagnostics
        .iter()
        .map(|d| &d.diagnostic.message)
        .collect();
    compiled
        .service
        .unwrap_or_else(|| panic!("recursive.cru: {messages:#?}"))
}

fn check(manifest: &Path) {
    let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args(["check", "--manifest-path"])
        .arg(manifest)
        .env("CARGO_TARGET_DIR", cargo_target::target_dir())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn crate_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(name)
        .canonicalize()
        .unwrap()
}

#[test]
fn server_code_compiles_for_recursive_shapes() {
    let directory = tempfile::tempdir().unwrap();
    let source = generate(&service(), &Options::default()).unwrap();
    // The direct self-reference is the one Rust cannot size unboxed.
    assert!(source.contains("Box<"), "a recursive member is boxed");
    fs::create_dir_all(directory.path().join("src")).unwrap();
    fs::write(directory.path().join("src/lib.rs"), source).unwrap();
    fs::write(
        directory.path().join("Cargo.toml"),
        format!(
            "[package]\nname = 'recursive-server'\nversion = '0.0.0'\nedition = '2024'\n[dependencies]\ncrucible-server-runtime-rust = {{ path = {:?} }}\n[workspace]\n",
            crate_path("crucible-server-runtime-rust")
        ),
    )
    .unwrap();
    check(&directory.path().join("Cargo.toml"));
}

#[test]
fn sdk_compiles_for_recursive_shapes() {
    let directory = tempfile::tempdir().unwrap();
    let files = emit_client_package(
        &service(),
        &ClientOptions {
            package_name: Some("recursive-sdk".into()),
            package_version: "0.0.0".into(),
            runtime: RuntimeDependency::Path(crate_path("crucible-client-runtime-rust")),
            metadata: Default::default(),
        },
    )
    .unwrap();
    for file in files {
        let path = directory.path().join(&file.path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &file.contents).unwrap();
    }
    check(&directory.path().join("Cargo.toml"));
}
