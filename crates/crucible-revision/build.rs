use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The source trees compiled into Crucible's binaries: the crates, the
/// traits package, the vendored compiler integration and the manifests.
const SOURCE_TREES: &[&str] = &["crates", "smithy", "vendor", "Cargo.toml", "Cargo.lock"];

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest"));
    let candidate = manifest.join("../..");
    let Ok(workspace) = candidate.canonicalize() else {
        return;
    };
    // A packaged or vendored emitter must not inherit a consumer repository's
    // revision. Only the actual Crucible source workspace supplies provenance.
    let repository_matches = fs::read_to_string(workspace.join("Cargo.toml"))
        .ok()
        .is_some_and(|manifest| {
            manifest.lines().any(|line| {
                line.trim() == "repository = \"https://github.com/Operation-Leyline/crucible\""
            })
        });
    if !repository_matches
        || !workspace
            .join("crates/crucible-client-runtime-rust/Cargo.toml")
            .is_file()
        || !workspace
            .join("crates/crucible-codegen-rust/Cargo.toml")
            .is_file()
        || !workspace.join("Cargo.toml").is_file()
    {
        return;
    }
    let Some(top) = git(&workspace, &["rev-parse", "--show-toplevel"]) else {
        return;
    };
    if Path::new(&top).canonicalize().ok().as_ref() != Some(&workspace) {
        return;
    }
    let Some(revision) = git(&workspace, &["rev-parse", "HEAD"]) else {
        return;
    };
    if revision.len() != 40 || !revision.bytes().all(|c| c.is_ascii_hexdigit()) {
        return;
    }
    // --git-path resolves linked worktree HEADs and their shared branch refs.
    watch(&workspace, "HEAD");
    watch(&workspace, "packed-refs");
    if let Some(reference) = git(&workspace, &["symbolic-ref", "--quiet", "HEAD"]) {
        watch(&workspace, &reference);
    }
    println!("cargo::rustc-env=CRUCIBLE_GENERATOR_REVISION={revision}");
    // A tracked file that differs from HEAD means HEAD does not reproduce
    // this generator. Only the trees compiled into the binaries count (docs
    // or CI files do not), and exactly those are watched, so an edit that
    // flips the mark always reruns this script; the index catches staged
    // changes.
    watch(&workspace, "index");
    let trees: Vec<&str> = SOURCE_TREES
        .iter()
        .copied()
        .filter(|tree| workspace.join(tree).exists())
        .collect();
    for tree in &trees {
        println!("cargo::rerun-if-changed={}", workspace.join(tree).display());
    }
    let mut arguments = vec!["status", "--porcelain", "--untracked-files=no", "--"];
    arguments.extend(trees.iter().copied());
    let dirty = git(&workspace, &arguments).is_none_or(|status| !status.is_empty());
    if dirty {
        println!("cargo::rustc-env=CRUCIBLE_GENERATOR_DIRTY=1");
    }
}

fn watch(workspace: &Path, name: &str) {
    if let Some(path) = git(workspace, &["rev-parse", "--git-path", name]) {
        let path = PathBuf::from(path);
        let absolute = if path.is_absolute() {
            path
        } else {
            workspace.join(path)
        };
        if absolute.is_file() {
            println!("cargo::rerun-if-changed={}", absolute.display());
        }
    }
}

fn git(workspace: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(arguments)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let result = String::from_utf8(output.stdout).ok()?;
    Some(result.trim().into())
}
