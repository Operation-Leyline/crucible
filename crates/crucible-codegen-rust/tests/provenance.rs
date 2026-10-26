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
            .env("GIT_CONFIG_COUNT", "0")
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
    fs::write(root.join(".gitignore"), "").unwrap();
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

fn copy_checkout(root: &Path, repository: &Path, build_target: Option<&Path>) {
    let root = root.canonicalize().unwrap();
    let build_target = build_target
        .and_then(|target| target.canonicalize().ok())
        .filter(|target| target != &root && target.starts_with(&root));
    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
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
        if build_target
            .as_ref()
            .is_some_and(|target| source.starts_with(target))
        {
            continue;
        }
        if source.is_file() {
            let target = repository.join(relative);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::copy(source, target).unwrap();
        }
    }
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
    // Cargo identifies its active output directory even when target-dir is
    // configured outside Git's ignore rules. Other nonignored paths are sources.
    copy_checkout(
        &root,
        &repository,
        Path::new(env!("CARGO_TARGET_TMPDIR")).parent(),
    );
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
    fs::write(consumer.join("Cargo.toml"), format!("[package]\nname = 'git-source-sdk-emitter'\nversion = '0.1.0'\nedition = '2024'\n[dependencies]\ncrucible-codegen-rust = {{ git = {url:?}, rev = {revision:?} }}\ncrucible-ir = {{ git = {url:?}, rev = {revision:?} }}\ncrucible-lang = {{ git = {url:?}, rev = {revision:?} }}\n[workspace]\n")).unwrap();
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

/// A checkout whose source files differ from HEAD has no verified
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

#[test]
fn scratch_checkout_includes_untracked_files_but_excludes_ignored_files() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("repository");
    init(&root, "https://github.com/Operation-Leyline/crucible");
    for name in [
        "crucible-codegen-common",
        "crucible-revision",
        "crucible-ir",
    ] {
        fs::create_dir_all(root.join("crates").join(name).join("src")).unwrap();
        fs::write(
            root.join("crates").join(name).join("src/lib.rs"),
            "original",
        )
        .unwrap();
    }
    fs::write(root.join(".gitignore"), "*.ignored\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "tracked fixture"]);
    fs::write(
        root.join("crates/crucible-ir/src/lib.rs"),
        "mod projection;",
    )
    .unwrap();
    fs::remove_file(root.join("crates/crucible-codegen-common/src/lib.rs")).unwrap();
    fs::write(
        root.join("crates/crucible-ir/src/projection.rs"),
        "// new module",
    )
    .unwrap();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(root.join("docs/new guide.md"), "new guide").unwrap();
    fs::write(
        root.join("crates/crucible-revision/src/cache.ignored"),
        "ignored",
    )
    .unwrap();
    let copy = directory.path().join("copy");
    fs::create_dir(&copy).unwrap();
    copy_checkout(&root, &copy, None);
    assert_eq!(
        fs::read_to_string(copy.join("crates/crucible-ir/src/lib.rs")).unwrap(),
        "mod projection;"
    );
    assert!(
        !copy
            .join("crates/crucible-codegen-common/src/lib.rs")
            .exists()
    );
    assert_eq!(
        fs::read_to_string(copy.join("crates/crucible-ir/src/projection.rs")).unwrap(),
        "// new module"
    );
    assert_eq!(
        fs::read_to_string(copy.join("docs/new guide.md")).unwrap(),
        "new guide"
    );
    assert!(
        !copy
            .join("crates/crucible-revision/src/cache.ignored")
            .exists()
    );
}

#[test]
fn untracked_source_files_mark_revision_dirty_but_ignored_files_and_docs_do_not() {
    let directory = tempfile::tempdir().unwrap();
    let executable = probe_binary(directory.path());
    let root = directory.path().join("repository");
    init(&root, "https://github.com/Operation-Leyline/crucible");
    fs::write(root.join(".gitignore"), "*.ignored\n").unwrap();
    git(&root, &["add", ".gitignore"]);
    git(&root, &["commit", "-qm", "ignore build artifacts"]);
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(root.join("docs/new guide.md"), "guide").unwrap();
    fs::write(
        root.join("crates/crucible-codegen-rust/cache.ignored"),
        "cache",
    )
    .unwrap();
    let clean = probe(&executable, &root);
    assert!(clean.contains("CRUCIBLE_GENERATOR_REVISION="));
    assert!(!clean.contains("CRUCIBLE_GENERATOR_DIRTY"), "{clean}");
    let source = root.join("crates/crucible-ir/src/projection.rs");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, "// new module").unwrap();
    let dirty = probe(&executable, &root);
    assert!(
        dirty.contains("cargo::rustc-env=CRUCIBLE_GENERATOR_DIRTY=1"),
        "{dirty}"
    );
    fs::remove_file(source).unwrap();
    let clean_again = probe(&executable, &root);
    assert!(
        !clean_again.contains("CRUCIBLE_GENERATOR_DIRTY"),
        "{clean_again}"
    );
}

#[test]
fn scratch_checkout_excludes_only_the_active_cargo_target() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("repository");
    init(&root, "https://github.com/Operation-Leyline/crucible");
    let target = root.join("custom build output");
    fs::create_dir_all(target.join("debug")).unwrap();
    fs::write(target.join("debug/artifact"), "build output").unwrap();
    fs::create_dir_all(root.join(".codex-target")).unwrap();
    fs::write(root.join(".codex-target/source.rs"), "// nonignored source").unwrap();
    let copy = directory.path().join("copy");
    fs::create_dir(&copy).unwrap();
    copy_checkout(&root, &copy, Some(&target));
    assert!(!copy.join("custom build output").exists());
    assert_eq!(
        fs::read_to_string(copy.join(".codex-target/source.rs")).unwrap(),
        "// nonignored source"
    );
}

#[test]
fn scratch_checkout_preserves_sources_with_external_cargo_targets() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("repository");
    init(&root, "https://github.com/Operation-Leyline/crucible");
    let sibling = directory.path().join("sibling target");
    fs::create_dir(&sibling).unwrap();
    fs::write(root.join("new module.rs"), "// untracked source").unwrap();
    for (index, target) in [directory.path(), sibling.as_path(), root.as_path()]
        .into_iter()
        .enumerate()
    {
        let copy = directory.path().join(format!("copy-{index}"));
        fs::create_dir(&copy).unwrap();
        copy_checkout(&root, &copy, Some(target));
        assert_eq!(
            fs::read_to_string(copy.join("Cargo.toml")).unwrap(),
            fs::read_to_string(root.join("Cargo.toml")).unwrap()
        );
        assert_eq!(
            fs::read_to_string(copy.join("new module.rs")).unwrap(),
            "// untracked source"
        );
    }
}

fn incremental_repository(root: &Path) {
    init(root, "https://github.com/Operation-Leyline/crucible");
    fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = ['crates/crucible-revision', 'probe']\nresolver = '3'\n[workspace.package]\nrepository = \"https://github.com/Operation-Leyline/crucible\"\n").unwrap();
    let revision = root.join("crates/crucible-revision");
    fs::create_dir_all(revision.join("src")).unwrap();
    fs::write(
        revision.join("Cargo.toml"),
        "[package]\nname = 'crucible-revision'\nversion = '0.0.0'\nedition = '2024'\n",
    )
    .unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../crucible-revision");
    for file in ["build.rs", "src/lib.rs"] {
        fs::copy(source.join(file), revision.join(file)).unwrap();
    }
    fs::create_dir_all(root.join("probe/src")).unwrap();
    fs::write(root.join("probe/Cargo.toml"), "[package]\nname = 'incremental-revision-probe'\nversion = '0.0.0'\nedition = '2024'\n[dependencies]\ncrucible-revision = { path = '../crates/crucible-revision' }\n").unwrap();
    fs::write(
        root.join("probe/src/main.rs"),
        "fn main() { println!(\"{}\", crucible_revision::recorded_revision().unwrap()); }\n",
    )
    .unwrap();
    fs::write(root.join(".gitignore"), "").unwrap();
    success(
        Command::new(env!("CARGO"))
            .args(["generate-lockfile", "--offline", "--manifest-path"])
            .arg(root.join("Cargo.toml"))
            .output()
            .unwrap(),
    );
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "revision consumer"]);
}

fn incremental_build(root: &Path, target: &Path) -> (String, String) {
    incremental_build_with_env(root, target, &[])
}

fn incremental_build_with_env(
    root: &Path,
    target: &Path,
    environment: &[(&str, &str)],
) -> (String, String) {
    let output = Command::new(env!("CARGO"))
        .args([
            "run",
            "--verbose",
            "--offline",
            "--color",
            "never",
            "--manifest-path",
        ])
        .arg(root.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", target)
        .env("CARGO_BUILD_JOBS", "1")
        // Git status must not rewrite a watched input after Cargo timestamps it.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_CONFIG_COUNT", "0")
        .envs(environment.iter().copied())
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&output.stderr).into_owned();
    (success(output).trim().to_owned(), log)
}

fn configuration_input(input: &str) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("repository");
    incremental_repository(&root);
    let config = directory.path().join("global config");
    let included = directory.path().join("included config");
    fs::write(&included, "").unwrap();
    fs::write(
        &config,
        if input == "empty include" {
            "[include]\npath = \"included config\"\n".to_owned()
        } else {
            String::new()
        },
    )
    .unwrap();
    let rules = directory.path().join("global excludes");
    fs::write(&rules, "/crates/new.rs\n").unwrap();
    fs::write(root.join("crates/new.rs"), "// source\n").unwrap();
    let target = directory.path().join("target");
    let mut environment = vec![
        ("GIT_CONFIG_GLOBAL", config.to_str().unwrap()),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_CONFIG_COUNT", "0"),
    ];
    let build =
        |environment: &[(&str, &str)]| incremental_build_with_env(&root, &target, environment);
    let revision = git(&root, &["rev-parse", "HEAD"]);
    assert_eq!(build(&environment).0, format!("{revision}+dirty"));
    assert!(build(&environment).1.contains("Fresh crucible-revision"));
    if input == "environment" {
        environment[2].1 = "1";
        environment.extend([
            ("GIT_CONFIG_KEY_0", "core.excludesFile"),
            ("GIT_CONFIG_VALUE_0", rules.to_str().unwrap()),
        ]);
        assert_eq!(build(&environment).0, revision, "config count activation");
        let empty = directory.path().join("empty excludes");
        fs::write(&empty, "").unwrap();
        environment[4].1 = empty.to_str().unwrap();
        assert_eq!(
            build(&environment).0,
            format!("{revision}+dirty"),
            "config value change"
        );
        environment[4].1 = rules.to_str().unwrap();
        assert_eq!(build(&environment).0, revision);
        environment[3].1 = "provenance.unused";
        assert_eq!(
            build(&environment).0,
            format!("{revision}+dirty"),
            "config key change"
        );
        environment[3].1 = "core.excludesFile";
        assert_eq!(build(&environment).0, revision);
        environment[2].1 = "0";
        assert_eq!(
            build(&environment).0,
            format!("{revision}+dirty"),
            "config count removal"
        );
    } else {
        let selected = if input == "empty include" {
            &included
        } else {
            &config
        };
        fs::write(
            selected,
            format!(
                "[core]\nexcludesFile = {:?}\n",
                rules.to_str().unwrap().replace('\\', "/")
            ),
        )
        .unwrap();
        assert_eq!(
            build(&environment).0,
            revision,
            "{input}: add excludes setting"
        );
        fs::write(selected, "").unwrap();
        assert_eq!(
            build(&environment).0,
            format!("{revision}+dirty"),
            "{input}: remove excludes setting"
        );
    }
}

#[test]
fn incremental_cargo_rechecks_git_environment() {
    configuration_input("environment");
}
#[test]
fn incremental_cargo_rechecks_empty_global_config() {
    configuration_input("empty global");
}
#[test]
fn incremental_cargo_rechecks_empty_included_config() {
    configuration_input("empty include");
}

#[test]
fn incremental_cargo_rechecks_ignore_decisions_without_source_edits() {
    for input in [".gitignore", ".git/info/exclude", "global excludes"] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("repository");
        incremental_repository(&root);
        let rules = if input == "global excludes" {
            let rules = directory.path().join(input);
            git(
                &root,
                &["config", "core.excludesFile", rules.to_str().unwrap()],
            );
            rules
        } else {
            // A host global exclude must not hide the file after the repository
            // rule is cleared. Select an existing controlled empty rules file.
            let empty = directory.path().join("empty global excludes");
            fs::write(&empty, "").unwrap();
            git(
                &root,
                &["config", "core.excludesFile", empty.to_str().unwrap()],
            );
            root.join(input)
        };
        fs::write(&rules, "/crates/new.rs\n").unwrap();
        fs::write(root.join("crates/new.rs"), "// source\n").unwrap();
        let target = directory.path().join("target");
        let revision = git(&root, &["rev-parse", "HEAD"]);
        assert_eq!(incremental_build(&root, &target).0, revision, "{input}");
        let (_, log) = incremental_build(&root, &target);
        assert!(log.contains("Fresh crucible-revision"), "{input}: {log}");
        fs::write(&rules, "").unwrap();
        assert_eq!(
            incremental_build(&root, &target).0,
            format!("{revision}+dirty"),
            "{input}: unignore"
        );
        fs::write(&rules, "/crates/new.rs\n").unwrap();
        assert_eq!(
            incremental_build(&root, &target).0,
            revision,
            "{input}: ignore again"
        );
        if input == ".git/info/exclude" {
            fs::remove_file(&rules).unwrap();
            assert_eq!(
                incremental_build(&root, &target).0,
                format!("{revision}+dirty"),
                "remove repository rules"
            );
            let (_, log) = incremental_build(&root, &target);
            assert!(log.contains("Fresh crucible-revision"), "{log}");
            fs::write(&rules, "/crates/new.rs\n").unwrap();
            assert_eq!(
                incremental_build(&root, &target).0,
                revision,
                "recreate repository rules"
            );
        }
        if input == "global excludes" {
            let other = directory.path().join("other excludes");
            fs::write(&other, "").unwrap();
            git(
                &root,
                &["config", "core.excludesFile", other.to_str().unwrap()],
            );
            assert_eq!(
                incremental_build(&root, &target).0,
                format!("{revision}+dirty"),
                "new config selects empty rules"
            );
            git(
                &root,
                &["config", "core.excludesFile", rules.to_str().unwrap()],
            );
            assert_eq!(
                incremental_build(&root, &target).0,
                revision,
                "config restores ignore rules"
            );
        }
    }
}

/// A packed branch gains a loose ref on its next commit. Cargo must notice
/// that creation even though the loose ref did not exist on the previous run.
#[test]
fn incremental_cargo_notices_a_packed_branch_gaining_a_loose_ref() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("repository");
    init(&root, "https://github.com/Operation-Leyline/crucible");
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = ['crates/crucible-revision', 'probe']\nresolver = '3'\n[workspace.package]\nrepository = \"https://github.com/Operation-Leyline/crucible\"\n",
    )
    .unwrap();
    let revision_crate = root.join("crates/crucible-revision");
    fs::create_dir_all(revision_crate.join("src")).unwrap();
    fs::write(
        revision_crate.join("Cargo.toml"),
        "[package]\nname = 'crucible-revision'\nversion = '0.0.0'\nedition = '2024'\n",
    )
    .unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../crucible-revision");
    for file in ["build.rs", "src/lib.rs"] {
        fs::copy(source.join(file), revision_crate.join(file)).unwrap();
    }
    fs::create_dir_all(root.join("probe/src")).unwrap();
    fs::write(
        root.join("probe/Cargo.toml"),
        "[package]\nname = 'incremental-revision-probe'\nversion = '0.0.0'\nedition = '2024'\n[dependencies]\ncrucible-revision = { path = '../crates/crucible-revision' }\n",
    )
    .unwrap();
    fs::write(
        root.join("probe/src/main.rs"),
        "fn main() { println!(\"{}\", crucible_revision::recorded_revision().unwrap()); }\n",
    )
    .unwrap();
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    success(
        Command::new(&cargo)
            .args(["generate-lockfile", "--offline", "--manifest-path"])
            .arg(root.join("Cargo.toml"))
            .output()
            .unwrap(),
    );
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "revision consumer"]);
    // An isolated target avoids another scratch checkout's Cargo fingerprints.
    let target = directory.path().join("target");
    let build = || {
        success(
            Command::new(&cargo)
                .args(["run", "--quiet", "--offline", "--manifest-path"])
                .arg(root.join("Cargo.toml"))
                .env("CARGO_TARGET_DIR", &target)
                .env("CARGO_BUILD_JOBS", "1")
                .output()
                .unwrap(),
        )
        .trim()
        .to_owned()
    };
    let initial = git(&root, &["rev-parse", "HEAD"]);
    assert_eq!(build(), initial);
    git(&root, &["pack-refs", "--all", "--prune"]);
    assert_eq!(build(), initial);
    git(
        &root,
        &["commit", "--allow-empty", "-qm", "advance packed branch"],
    );
    let next = git(&root, &["rev-parse", "HEAD"]);
    assert_ne!(next, initial);
    assert_eq!(build(), next);
    assert_eq!(build(), next);
}
