use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The source trees compiled into Crucible's binaries: the crates and the
/// manifests.
const SOURCE_TREES: &[&str] = &["crates", "Cargo.toml", "Cargo.lock"];

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
    // A source edit or untracked source file means HEAD does not reproduce
    // this generator. Only the trees compiled into the binaries count (docs
    // or CI files do not), and exactly those are watched, so an edit that
    // flips the mark always reruns this script; the index catches staged
    // changes.
    watch(&workspace, "index");
    watch_ignores(&workspace);
    let trees: Vec<&str> = SOURCE_TREES
        .iter()
        .copied()
        .filter(|tree| workspace.join(tree).exists())
        .collect();
    for tree in &trees {
        println!("cargo::rerun-if-changed={}", workspace.join(tree).display());
    }
    let mut arguments = vec!["status", "--porcelain", "--untracked-files=normal", "--"];
    arguments.extend(trees.iter().copied());
    let dirty = git(&workspace, &arguments).is_none_or(|status| !status.is_empty());
    if dirty {
        println!("cargo::rustc-env=CRUCIBLE_GENERATOR_DIRTY=1");
    }
}

fn watch_ignores(workspace: &Path) {
    for name in [
        "HOME",
        "XDG_CONFIG_HOME",
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_SYSTEM",
        "GIT_CONFIG_NOSYSTEM",
    ] {
        println!("cargo::rerun-if-env-changed={name}");
    }
    // Ignore rules can reclassify an existing untracked source without changing
    // any source-tree mtime. Let Git resolve repository and configured paths;
    // nested .gitignore files are already covered by the source-tree watches.
    watch_ignore_file(&workspace.join(".gitignore"));
    if let Some(path) = git(workspace, &["rev-parse", "--git-path", "info/exclude"]) {
        let path = workspace.join(path);
        if path.is_file() {
            watch_ignore_file(&path);
        } else if let Some(parent) = path.parent().filter(|parent| parent.is_dir()) {
            // info/ is bounded Git metadata, unlike the checkout or home.
            println!("cargo::rerun-if-changed={}", parent.display());
        }
    }
    // Config includes and per-worktree overrides can select another excludes
    // file. NUL delimiters preserve paths containing spaces without Git quoting.
    if let Some(config) = git(workspace, &["config", "--null", "--show-origin", "--list"]) {
        let paths: BTreeSet<_> = config
            .split('\0')
            .step_by(2)
            .filter_map(|entry| entry.strip_prefix("file:"))
            .collect();
        for path in paths {
            watch_ignore_file(&workspace.join(path));
        }
    }
    if let Some(path) = git(
        workspace,
        &["config", "--path", "--get", "core.excludesFile"],
    ) {
        watch_ignore_file(&workspace.join(path));
    } else {
        let default = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|path| !path.is_empty())
            .map(|path| PathBuf::from(path).join("git/ignore"))
            .or_else(|| {
                git(
                    workspace,
                    &[
                        "-c",
                        "core.excludesFile=~/.config/git/ignore",
                        "config",
                        "--path",
                        "--get",
                        "core.excludesFile",
                    ],
                )
                .map(PathBuf::from)
            });
        if let Some(path) = default {
            watch_ignore_file(&path);
        }
    }
}

fn watch_ignore_file(path: &Path) {
    // Cargo recursively scans directory watches, and missing-file watches force
    // every build to rerun. Activation of an absent root/global ignore file is
    // therefore outside this bounded watch (issue #400); edits and
    // deletion of existing rules are covered without scanning checkout/home.
    if path.is_file() {
        println!("cargo::rerun-if-changed={}", path.display());
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
