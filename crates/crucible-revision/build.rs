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
        packaged();
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
        packaged();
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
    watch_refs(&workspace);
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

/// Not inside the Crucible source workspace: a registry download, a vendored
/// copy or `cargo package` verification. Such a build has no revision to pin,
/// but its sources are exactly its published version. A checkout whose Git
/// metadata is unreadable is not this case: it keeps no provenance at all.
fn packaged() {
    println!("cargo::rustc-env=CRUCIBLE_GENERATOR_PACKAGED=1");
}

fn watch_refs(workspace: &Path) {
    // --git-path resolves the linked worktree's own HEAD and ref store.
    watch(workspace, "HEAD");
    if git(workspace, &["rev-parse", "--show-ref-format"]).as_deref() == Some("reftable") {
        // Ref transactions replace tables.list atomically. Watching that file
        // covers new tables and compaction without depending on transient names.
        watch(workspace, "reftable/tables.list");
        if let Some(common) = git(workspace, &["rev-parse", "--git-common-dir"]) {
            let shared = workspace.join(common).join("reftable/tables.list");
            if shared.is_file() {
                // A linked worktree stores HEAD locally and branches in the
                // common stack; updating either must invalidate its revision.
                println!("cargo::rerun-if-changed={}", shared.display());
            }
        }
    } else {
        watch(workspace, "packed-refs");
        if let Some(reference) = git(workspace, &["symbolic-ref", "--quiet", "HEAD"]) {
            watch(workspace, &reference);
        }
    }
}

fn watch_ignores(workspace: &Path) {
    for name in [
        "HOME",
        "XDG_CONFIG_HOME",
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_SYSTEM",
        "GIT_CONFIG_NOSYSTEM",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
    ] {
        println!("cargo::rerun-if-env-changed={name}");
    }
    if let Some(count) = std::env::var("GIT_CONFIG_COUNT")
        .ok()
        .and_then(|count| count.parse::<usize>().ok())
    {
        for index in 0..count {
            println!("cargo::rerun-if-env-changed=GIT_CONFIG_KEY_{index}");
            println!("cargo::rerun-if-env-changed=GIT_CONFIG_VALUE_{index}");
        }
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
    // Empty config files have no --list entries. Git supplies its global/system
    // locations; include paths are resolved relative to their declaring file.
    for variable in ["GIT_CONFIG_GLOBAL", "GIT_CONFIG_SYSTEM"] {
        if let Some(paths) = git(workspace, &["var", variable]) {
            for path in paths.lines() {
                watch_ignore_file(&workspace.join(path));
            }
        }
    }
    watch(workspace, "config");
    watch(workspace, "config.worktree");
    if let Some(includes) = git(
        workspace,
        &[
            "config",
            "--null",
            "--show-origin",
            "--path",
            "--get-regexp",
            r"^include(if\..*)?\.path$",
        ],
    ) {
        let entries: Vec<_> = includes.split('\0').collect();
        for pair in entries.chunks_exact(2) {
            if let Some((_, path)) = pair[1].split_once('\n') {
                let base = pair[0]
                    .strip_prefix("file:")
                    .and_then(|origin| Path::new(origin).parent())
                    .unwrap_or(workspace);
                watch_ignore_file(&workspace.join(base).join(path));
            }
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
        let watched = if absolute.is_file() {
            Some(absolute.as_path())
        } else if name.starts_with("refs/") {
            // A packed branch has no loose ref until its next commit. Watch
            // its nearest existing parent, including when nested dirs are absent.
            absolute.ancestors().skip(1).find(|path| path.is_dir())
        } else {
            None
        };
        if let Some(path) = watched {
            println!("cargo::rerun-if-changed={}", path.display());
        }
    }
}

fn git(workspace: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(arguments)
        // These read-only queries must not refresh the index that Cargo watches.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let result = String::from_utf8(output.stdout).ok()?;
    Some(result.trim().into())
}
