#[path = "../../../tests/support/cargo_target.rs"]
mod cargo_target;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn git(root: &Path, args: &[&str]) -> String {
    success(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap(),
    )
    .trim()
    .into()
}

fn probe_binary(root: &Path) -> PathBuf {
    let executable = root.join(if cfg!(windows) {
        "provenance.exe"
    } else {
        "provenance"
    });
    success(
        Command::new("rustc")
            .args([
                "--edition",
                "2024",
                "--crate-name",
                "crucible_provenance_probe",
            ])
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../crucible-revision/build.rs"))
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap(),
    );
    executable
}

fn probe(executable: &Path, root: &Path) -> String {
    success(
        Command::new(executable)
            .env(
                "CARGO_MANIFEST_DIR",
                root.join("crates/crucible-codegen-rust"),
            )
            .output()
            .unwrap(),
    )
}

fn init(root: &Path, repository: &str) {
    fs::create_dir_all(root.join("crates/crucible-codegen-rust")).unwrap();
    fs::create_dir_all(root.join("crates/crucible-client-runtime-rust")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!("[workspace.package]\nrepository = {repository:?}\n"),
    )
    .unwrap();
    fs::write(
        root.join("crates/crucible-codegen-rust/Cargo.toml"),
        "[package]\nname = 'crucible-codegen-rust'\n",
    )
    .unwrap();
    fs::write(
        root.join("crates/crucible-client-runtime-rust/Cargo.toml"),
        "[package]\nname = 'crucible-client-runtime-rust'\n",
    )
    .unwrap();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "Crucible provenance test"]);
    git(
        root,
        &["config", "user.email", "provenance@example.invalid"],
    );
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "source snapshot"]);
}

#[test]
fn provenance_watches_real_branch_worktree_and_packed_ref_files() {
    let directory = tempfile::tempdir().unwrap();
    let executable = probe_binary(directory.path());
    let root = directory.path().join("repository");
    init(&root, "https://github.com/Operation-Leyline/crucible");
    let revision = git(&root, &["rev-parse", "HEAD"]);
    let output = probe(&executable, &root);
    assert!(output.contains(&format!(
        "cargo::rustc-env=CRUCIBLE_GENERATOR_REVISION={revision}"
    )));
    assert!(output.replace('\\', "/").contains("refs/heads/main"));
    assert!(!output.contains("packed-refs"));
    git(&root, &["pack-refs", "--all", "--prune"]);
    let output = probe(&executable, &root);
    assert!(output.contains("packed-refs"));
    let worktree = directory.path().join("worktree");
    git(
        &root,
        &[
            "worktree",
            "add",
            "--detach",
            worktree.to_str().unwrap(),
            "HEAD",
        ],
    );
    let output = probe(&executable, &worktree);
    assert!(output.contains(&format!(
        "cargo::rustc-env=CRUCIBLE_GENERATOR_REVISION={revision}"
    )));
    assert!(output.contains("worktrees"));
    for path in output
        .lines()
        .filter_map(|line| line.strip_prefix("cargo::rerun-if-changed="))
    {
        if path != "build.rs" {
            assert!(Path::new(path).exists(), "{path}");
        }
    }
}

#[test]
fn a_vendored_emitter_does_not_capture_the_enclosing_consumers_revision() {
    let directory = tempfile::tempdir().unwrap();
    let executable = probe_binary(directory.path());
    let root = directory.path().join("consumer");
    init(&root, "https://example.invalid/consumer");
    assert!(!probe(&executable, &root).contains("CRUCIBLE_GENERATOR_REVISION"));
    let vendored = root.join("vendor/crucible");
    fs::create_dir_all(vendored.join("crates/crucible-codegen-rust")).unwrap();
    fs::create_dir_all(vendored.join("crates/crucible-client-runtime-rust")).unwrap();
    fs::write(
        vendored.join("Cargo.toml"),
        "repository = \"https://github.com/Operation-Leyline/crucible\"\n",
    )
    .unwrap();
    fs::write(
        vendored.join("crates/crucible-codegen-rust/Cargo.toml"),
        "package",
    )
    .unwrap();
    fs::write(
        vendored.join("crates/crucible-client-runtime-rust/Cargo.toml"),
        "runtime",
    )
    .unwrap();
    assert!(!probe(&executable, &vendored).contains("CRUCIBLE_GENERATOR_REVISION"));
}

#[test]
fn automatic_runtime_revision_survives_an_actual_cargo_git_source_checkout() {
    let directory = tempfile::tempdir().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let repository = directory.path().join("repository");
    fs::create_dir(&repository).unwrap();
    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["ls-files", "-z"])
        .output()
        .unwrap();
    assert!(output.status.success());
    for relative in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let relative = std::str::from_utf8(relative).unwrap();
        let source = root.join(relative);
        if source.is_file() {
            let target = repository.join(relative);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::copy(source, target).unwrap();
        }
    }
    // The feature under test may still be uncommitted in the caller checkout.
    fn copy_tree(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            if matches!(
                entry.file_name().to_str(),
                Some("target" | ".codex-target" | ".git")
            ) {
                continue;
            }
            let target = destination.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_tree(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    for crate_name in [
        "crucible-codegen-rust",
        "crucible-codegen-common",
        "crucible-revision",
    ] {
        // The tracked copy may be removed (a moved build script).
        let _ = fs::remove_dir_all(repository.join("crates").join(crate_name));
        copy_tree(
            &root.join("crates").join(crate_name),
            &repository.join("crates").join(crate_name),
        );
    }
    git(&repository, &["init", "-q", "-b", "main"]);
    git(
        &repository,
        &["config", "user.name", "Crucible source test"],
    );
    git(
        &repository,
        &["config", "user.email", "source@example.invalid"],
    );
    git(&repository, &["add", "."]);
    git(&repository, &["commit", "-qm", "Cargo Git source snapshot"]);
    let revision = git(&repository, &["rev-parse", "HEAD"]);
    let url = url::Url::from_directory_path(&repository)
        .unwrap()
        .to_string();
    let consumer = directory.path().join("consumer");
    fs::create_dir_all(consumer.join("src")).unwrap();
    fs::write(consumer.join("Cargo.toml"), format!("[package]\nname = 'git-source-sdk-emitter'\nversion = '0.1.0'\nedition = '2024'\n[dependencies]\ncrucible-codegen-rust = {{ git = {url:?}, rev = {revision:?} }}\ncrucible-smithy = {{ git = {url:?}, rev = {revision:?} }}\ncrucible-lang = {{ git = {url:?}, rev = {revision:?} }}\n[workspace]\n")).unwrap();
    copy_tree(
        &root.join("tests/models/weather"),
        &consumer.join("weather"),
    );
    fs::write(consumer.join("src/main.rs"), "fn main() { let service = crucible_lang::load_service(&std::path::Path::new(env!(\"CARGO_MANIFEST_DIR\")).join(\"weather\")).unwrap(); let files = crucible_codegen_rust::emit_client_package(&service, &Default::default()).unwrap(); print!(\"{}\", files[0].contents); }\n").unwrap();
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let target = cargo_target::target_dir();
    let output = success(
        Command::new(&cargo)
            .args(["run", "--quiet", "--manifest-path"])
            .arg(consumer.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", target)
            .env("CARGO_BUILD_JOBS", "1")
            .output()
            .unwrap(),
    );
    let manifest: toml::Value = toml::from_str(&output).unwrap();
    let dependency = &manifest["dependencies"]["crucible-client-runtime-rust"];
    assert_eq!(dependency["rev"].as_str(), Some(revision.as_str()));
    assert_eq!(
        dependency["git"].as_str(),
        Some("https://github.com/Operation-Leyline/crucible")
    );
    assert!(dependency.get("path").is_none());
}

/// A checkout whose tracked files differ from HEAD has no verified
/// revision: the build script marks it dirty, and watches the sources so
/// the mark never goes stale.
#[test]
fn uncommitted_changes_mark_the_revision_dirty() {
    let directory = tempfile::tempdir().unwrap();
    let executable = probe_binary(directory.path());
    let root = directory.path().join("repository");
    init(&root, "https://github.com/Operation-Leyline/crucible");
    let clean = probe(&executable, &root);
    assert!(clean.contains("CRUCIBLE_GENERATOR_REVISION="));
    assert!(!clean.contains("CRUCIBLE_GENERATOR_DIRTY"));
    assert!(clean.lines().any(|line| {
        line.starts_with("cargo::rerun-if-changed=")
            && Path::new(line.trim_start_matches("cargo::rerun-if-changed=")).ends_with("crates")
    }));
    // Only the compiled source trees count, and only they are watched: an
    // edit to the docs neither marks the build dirty nor reruns the script.
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(root.join("docs/guide.md"), "guide").unwrap();
    git(&root, &["add", "docs/guide.md"]);
    git(&root, &["commit", "-qm", "docs"]);
    fs::write(root.join("docs/guide.md"), "edited").unwrap();
    let docs = probe(&executable, &root);
    assert!(!docs.contains("CRUCIBLE_GENERATOR_DIRTY"), "{docs}");
    assert!(
        !docs
            .lines()
            .any(|line| line.starts_with("cargo::rerun-if-changed=")
                && Path::new(line.trim_start_matches("cargo::rerun-if-changed="))
                    .ends_with("docs")),
        "{docs}"
    );
    fs::write(
        root.join("crates/crucible-codegen-rust/Cargo.toml"),
        "[package]\nname = 'changed'\n",
    )
    .unwrap();
    assert!(probe(&executable, &root).contains("cargo::rustc-env=CRUCIBLE_GENERATOR_DIRTY=1"));
}
