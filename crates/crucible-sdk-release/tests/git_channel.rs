//! The git-tag channel against real local bare repositories, with Git
//! isolated from the user's configuration.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crucible_sdk_release::archive::write_tree_archive;
use crucible_sdk_release::command::{CommandLine, CommandOutput, CommandRunner, SystemRunner};
use crucible_sdk_release::legacy::LegacyRecord;
use crucible_sdk_release::publish::{Mode, PublishError, PublishItem, Step, publish};
use crucible_sdk_release::version::{Inputs, choose};
use crucible_sdk_release::{
    Channel, ChannelError, ChannelRead, GitChannelConfig, GitTagChannel, Journal, PublishRequest,
    Published, Version,
};
use tempfile::TempDir;

/// Runs Git with an empty global configuration and no system one, so a
/// developer's `commit.gpgsign` or identity cannot leak into the tests.
struct Isolated {
    global: PathBuf,
}

impl Isolated {
    fn env(&self) -> [(&'static str, String); 2] {
        [
            (
                "GIT_CONFIG_GLOBAL",
                self.global.to_string_lossy().into_owned(),
            ),
            ("GIT_CONFIG_NOSYSTEM", "1".into()),
        ]
    }
}

impl CommandRunner for Isolated {
    fn run(&self, command: &CommandLine) -> io::Result<CommandOutput> {
        let mut command = command.clone();
        for (name, value) in self.env() {
            command = command.env(name, value);
        }
        SystemRunner.run(&command)
    }
}

/// A scratch area: a bare "remote" and helpers to seed it.
struct Fixture {
    root: TempDir,
    runner: Isolated,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let global = root.path().join("gitconfig");
        fs::write(&global, "").unwrap();
        let fixture = Self {
            root,
            runner: Isolated { global },
        };
        fixture.git(None, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
        fixture
    }

    fn remote(&self) -> PathBuf {
        self.root.path().join("remote.git")
    }

    fn url(&self) -> String {
        url::Url::from_directory_path(self.remote())
            .unwrap()
            .to_string()
    }

    fn git(&self, cwd: Option<&Path>, args: &[&str]) -> String {
        let mut command = Command::new("git");
        command
            .current_dir(cwd.unwrap_or(self.root.path()))
            .args([
                "-c",
                "user.name=Seed",
                "-c",
                "user.email=seed@example.invalid",
            ])
            .args(["-c", "core.autocrlf=false"])
            .args(args);
        for (name, value) in self.runner.env() {
            command.env(name, value);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    /// Commits `files` onto the remote's `main` (on top of what is there)
    /// and tags it; `annotated` makes an annotated tag.
    fn seed(&self, files: &[(&str, &str)], tag: &str, annotated: bool) -> String {
        let work = tempfile::tempdir_in(self.root.path()).unwrap();
        let dir = work.path();
        self.git(Some(dir), &["init", "-q", "-b", "main"]);
        let remote = self.url();
        if !self
            .git(Some(dir), &["ls-remote", "--heads", &remote])
            .is_empty()
        {
            self.git(Some(dir), &["fetch", "-q", &remote, "main"]);
            self.git(Some(dir), &["reset", "-q", "--hard", "FETCH_HEAD"]);
        }
        for (path, contents) in files {
            let path = dir.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        self.git(Some(dir), &["add", "-A", "-f"]);
        self.git(Some(dir), &["commit", "-q", "--allow-empty", "-m", "seed"]);
        if annotated {
            self.git(Some(dir), &["tag", "-a", "-m", "seed", tag]);
        } else {
            self.git(Some(dir), &["tag", tag]);
        }
        self.git(
            Some(dir),
            &[
                "push",
                "-q",
                &remote,
                "HEAD:refs/heads/main",
                &format!("refs/tags/{tag}"),
            ],
        );
        self.git(Some(dir), &["rev-parse", "HEAD"])
    }

    /// The files at `rev` in the remote, with their contents.
    fn tree(&self, rev: &str) -> Vec<(String, String)> {
        let remote = self.remote();
        let listing = self.git(Some(&remote), &["ls-tree", "-r", "--name-only", rev]);
        listing
            .lines()
            .map(|path| {
                (
                    path.to_owned(),
                    self.git(Some(&remote), &["show", &format!("{rev}:{path}")]),
                )
            })
            .collect()
    }

    fn paths(&self, rev: &str) -> Vec<String> {
        self.tree(rev).into_iter().map(|(path, _)| path).collect()
    }

    fn channel(&self, config: GitChannelConfig) -> GitTagChannel {
        GitTagChannel::with_runner(
            &config,
            Box::new(Isolated {
                global: self.runner.global.clone(),
            }),
        )
        .unwrap()
    }

    /// A package directory holding `files`, with the `.crucible` a
    /// generator writes.
    fn package(&self, name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = self.root.path().join(name);
        for (path, contents) in files {
            let path = dir.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        let mut owned: Vec<&str> = files.iter().map(|(path, _)| *path).collect();
        owned.sort_unstable();
        fs::write(
            dir.join(".crucible"),
            serde_json::to_string(&owned).unwrap(),
        )
        .unwrap();
        dir
    }
}

fn provenance(version: &str, digest: &str) -> String {
    serde_json::json!({
        "schemaVersion": 2, "language": "rust", "package": "pigeon-sdk", "packageVersion": version,
        "serviceVersion": "2026-10-01", "contentDigest": digest,
        "source": { "commit": "20ccb55" },
    })
    .to_string()
}

fn request<'a>(version: &'a Version, artifact: &'a Path) -> PublishRequest<'a> {
    PublishRequest {
        sdk: "rust",
        version,
        artifact,
        source_commit: Some("20ccb55"),
        package: Some("pigeon-sdk"),
    }
}

#[test]
fn an_empty_repository_is_an_empty_channel() {
    let fixture = Fixture::new();
    let channel = fixture.channel(GitChannelConfig::new(fixture.url()));
    assert_eq!(channel.read_last().unwrap(), ChannelRead::Empty);
}

#[test]
fn an_unreachable_repository_is_unreadable_not_empty() {
    let fixture = Fixture::new();
    let missing = url::Url::from_directory_path(fixture.root.path().join("missing.git"))
        .unwrap()
        .to_string();
    let error = fixture
        .channel(GitChannelConfig::new(missing))
        .read_last()
        .unwrap_err();
    assert!(matches!(error, ChannelError::Unreadable { .. }), "{error}");
}

#[test]
fn releases_and_reads_back_with_ownership() {
    let fixture = Fixture::new();
    let channel = fixture.channel(GitChannelConfig::new(fixture.url()));
    let first = fixture.package(
        "first",
        &[
            ("Cargo.toml", "[package]\nversion = \"0.1.0\"\n"),
            ("src/lib.rs", "pub mod old;\n"),
            ("src/old.rs", "// removed in 0.1.1\n"),
            (
                "crucible.json",
                &provenance("0.1.0", "crucible-content-1:sha256:aa"),
            ),
            ("crucible-model.json", "{\"format\":5}"),
        ],
    );
    let v010 = Version::new(0, 1, 0);
    assert_eq!(
        channel.check_published(&v010, &first).unwrap(),
        Published::Absent
    );
    let outcome = channel.publish(&request(&v010, &first)).unwrap();
    assert!(
        outcome.detail.contains("pushed v0.1.0"),
        "{}",
        outcome.detail
    );
    assert_eq!(
        channel.check_published(&v010, &first).unwrap(),
        Published::SameBytes
    );

    let read = channel.read_last().unwrap();
    let last = read.last().unwrap();
    assert_eq!(last.version, v010);
    assert_eq!(last.content_digest(), Some("crucible-content-1:sha256:aa"));
    assert_eq!(last.model.as_deref(), Some("{\"format\":5}"));
    let commit = fixture.git(Some(&fixture.remote()), &["rev-parse", "v0.1.0^{commit}"]);
    assert_eq!(last.artifact, format!("git:{commit}"));
    let message = fixture.git(
        Some(&fixture.remote()),
        &["log", "-1", "--format=%B", "v0.1.0"],
    );
    assert!(
        message.starts_with("Release pigeon-sdk v0.1.0 from 20ccb55\n"),
        "{message}"
    );
    assert!(message.contains("20ccb55"), "{message}");

    // Someone adds a file the package does not own.
    fixture.seed(&[("NOTICE", "keep me\n")], "unrelated", false);

    let second = fixture.package(
        "second",
        &[
            ("Cargo.toml", "[package]\nversion = \"0.1.1\"\n"),
            ("src/lib.rs", "pub mod new;\n"),
            ("src/new.rs", "// new\n"),
            (
                "crucible.json",
                &provenance("0.1.1", "crucible-content-1:sha256:bb"),
            ),
        ],
    );
    let v011 = Version::new(0, 1, 1);
    // A changed package under an existing version is different bytes.
    assert!(matches!(
        channel.check_published(&v010, &second).unwrap(),
        Published::DifferentBytes { .. }
    ));
    channel.publish(&request(&v011, &second)).unwrap();
    assert_eq!(
        fixture.paths("v0.1.1"),
        [
            ".crucible",
            "Cargo.toml",
            "NOTICE",
            "crucible.json",
            "src/lib.rs",
            "src/new.rs"
        ]
    );
    // 0.1.0's tag is untouched.
    assert!(fixture.paths("v0.1.0").contains(&"src/old.rs".to_owned()));
    assert_eq!(channel.latest().unwrap().unwrap().version, v011);
}

#[test]
fn never_re_tags() {
    let fixture = Fixture::new();
    let channel = fixture.channel(GitChannelConfig::new(fixture.url()));
    let package = fixture.package("p", &[("a.txt", "one\n")]);
    let version = Version::new(0, 1, 0);
    channel.publish(&request(&version, &package)).unwrap();
    let changed = fixture.package("q", &[("a.txt", "two\n")]);
    let error = channel.publish(&request(&version, &changed)).unwrap_err();
    assert!(matches!(error, ChannelError::Publish { .. }), "{error}");
    // Atomic: the branch did not move either.
    let main = fixture.git(Some(&fixture.remote()), &["rev-parse", "main"]);
    let tag = fixture.git(Some(&fixture.remote()), &["rev-parse", "v0.1.0^{commit}"]);
    assert_eq!(main, tag);
}

/// Pigeon's mailpigeon-rs, as tools/publish_rust_sdk.py left it: no
/// `.crucible`, an `sdk-release.json`, tests and a lockfile in the tree.
#[test]
fn imports_and_adopts_pigeons_rust_distribution() {
    let fixture = Fixture::new();
    let sdk_release = r#"{
  "contractHash": "a03f504b756b952bf1fa4f84e2b43bcb41b85b79aa2201e8f1c7f7ad97fe09bc",
  "sourceRepository": "Operation-Pigeon/pigeon",
  "sourceCommit": "20ccb55bd9a902de2bc30912dbe97e8d94474a2f",
  "generator": "crucible@63f6a3015e0856ef73004e5f0b3acc425335b71c",
  "packageName": "pigeon-sdk"
}"#;
    fixture.seed(
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"pigeon-sdk\"\nversion = \"0.1.3\"\n",
            ),
            ("src/lib.rs", "// 0.1.3\n"),
            ("sdk-release.json", sdk_release),
        ],
        "v0.1.3",
        false,
    );
    fixture.seed(
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"pigeon-sdk\"\nversion = \"0.2.0\"\n",
            ),
            ("Cargo.lock", "# lock\n"),
            ("README.md", "old readme\n"),
            (".gitignore", "/target/\n"),
            (".rustfmt.toml", "edition = \"2024\"\n"),
            ("src/lib.rs", "// 0.2.0\n"),
            (
                "src/legacy_only.rs",
                "// gone after adoption? no: src is the package's\n",
            ),
            ("tests/wire.rs", "#[test] fn wire() {}\n"),
            ("docs/guide.md", "guide\n"),
            ("NOTICE", "notice\n"),
            (".github/workflows/ci.yml", "on: push\n"),
        ],
        "v0.2.0",
        true,
    );
    let config = GitChannelConfig {
        adopt: [
            "tests",
            "docs",
            "Cargo.lock",
            "sdk-release.json",
            ".gitignore",
        ]
        .map(String::from)
        .to_vec(),
        ..GitChannelConfig::new(fixture.url())
    };
    let channel = fixture.channel(config);
    let read = channel.read_last().unwrap();
    let last = read.last().unwrap();
    // Annotated tags are peeled to their commit.
    let commit = fixture.git(Some(&fixture.remote()), &["rev-parse", "v0.2.0^{commit}"]);
    assert_eq!(last.artifact, format!("git:{commit}"));
    assert_eq!(last.version, Version::new(0, 2, 0));
    assert_eq!(last.provenance, None);
    assert_eq!(
        last.legacy.as_ref().map(LegacyRecord::source_commit),
        Some(Some("20ccb55bd9a902de2bc30912dbe97e8d94474a2f"))
    );
    // No schema-2 digest: changed once, the next patch over the floor 0.2.0.
    let floor = Version::new(0, 2, 0);
    let mut inputs = Inputs::new("rust", &read);
    inputs.floor = Some(&floor);
    let decision = choose(&inputs).unwrap();
    let version = decision.version.unwrap();
    assert_eq!(version, Version::new(0, 2, 1));

    let package = fixture.package(
        "package",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"pigeon-sdk\"\nversion = \"0.2.1\"\npublish = false\n",
            ),
            ("README.md", "new readme\n"),
            (".rustfmt.toml", "edition = \"2024\"\n"),
            ("src/lib.rs", "// 0.2.1\n"),
            (
                "crucible.json",
                &provenance("0.2.1", "crucible-content-1:sha256:cc"),
            ),
        ],
    );
    let release_dir = tempfile::tempdir().unwrap();
    let item = PublishItem {
        sdk: "rust".into(),
        version: version.clone(),
        baseline: decision.baseline.clone(),
        artifact: package.clone(),
        artifact_sha256: None,
        source_commit: None,
        package: None,
    };
    let mut journal = Journal::default();
    let step = publish(
        &channel,
        &item,
        release_dir.path(),
        &mut journal,
        Mode::Execute,
    )
    .unwrap();
    let Step::Published(outcome) = step else {
        panic!("{step:?}")
    };
    assert!(outcome.detail.contains("adopted"), "{}", outcome.detail);
    let tree = fixture.tree("v0.2.1");
    let paths: Vec<&str> = tree.iter().map(|(path, _)| path.as_str()).collect();
    // Adopted paths are gone; the package's paths are written; what the
    // old publisher did not own (NOTICE, workflows) stays. `src` is not in
    // `adopt`, so a stray file there survives too: adoption removes only
    // what it is told.
    assert_eq!(
        paths,
        [
            ".crucible",
            ".github/workflows/ci.yml",
            ".rustfmt.toml",
            "Cargo.toml",
            "NOTICE",
            "README.md",
            "crucible.json",
            "src/legacy_only.rs",
            "src/lib.rs",
        ]
    );
    assert!(tree.contains(&("README.md".into(), "new readme".into())));
    // From now on the tree has a .crucible, and the next release reads it.
    let read = channel.read_last().unwrap();
    assert_eq!(
        read.last().unwrap().content_digest(),
        Some("crucible-content-1:sha256:cc")
    );
    assert_eq!(read.last().unwrap().legacy, None);
}

#[test]
fn a_module_in_a_subdirectory_with_a_tag_prefix() {
    let fixture = Fixture::new();
    fixture.seed(
        &[("README.md", "repo\n"), ("go/.gitignore", "*.json\n")],
        "v9.9.9",
        false,
    );
    let config = GitChannelConfig {
        path: "go".into(),
        tag: "go/v{version}".into(),
        ..GitChannelConfig::new(fixture.url())
    };
    let channel = fixture.channel(config);
    // v9.9.9 is another line's tag.
    assert_eq!(channel.read_last().unwrap(), ChannelRead::Empty);
    let package = fixture.package(
        "module",
        &[
            ("go.mod", "module example.com/sdk\n"),
            ("client.go", "package sdk\n"),
            (
                "crucible.json",
                &provenance("0.1.0", "crucible-content-1:sha256:dd"),
            ),
        ],
    );
    // Published from its tree archive, as `build` leaves it.
    let archive = fixture.root.path().join("module.tar");
    write_tree_archive(&package, &archive).unwrap();
    let version = Version::new(0, 1, 0);
    channel.publish(&request(&version, &archive)).unwrap();
    assert_eq!(
        fixture.paths("go/v0.1.0"),
        [
            "README.md",
            "go/.crucible",
            "go/.gitignore",
            "go/client.go",
            "go/crucible.json",
            "go/go.mod",
        ]
    );
    assert_eq!(
        channel.check_published(&version, &archive).unwrap(),
        Published::SameBytes
    );
    let read = channel.read_last().unwrap();
    assert_eq!(read.last().unwrap().version, version);
    assert_eq!(
        read.last().unwrap().content_digest(),
        Some("crucible-content-1:sha256:dd")
    );
}

#[test]
fn publish_resumes_and_stops_on_a_moved_channel() {
    let fixture = Fixture::new();
    let channel = fixture.channel(GitChannelConfig::new(fixture.url()));
    let first = fixture.package("first", &[("a.txt", "1\n")]);
    channel
        .publish(&request(&Version::new(0, 1, 0), &first))
        .unwrap();
    let baseline = channel.latest().unwrap().unwrap().baseline();
    let package = fixture.package("next", &[("a.txt", "2\n")]);
    let item = PublishItem {
        sdk: "rust".into(),
        version: Version::new(0, 1, 1),
        baseline: Some(baseline.clone()),
        artifact: package.clone(),
        artifact_sha256: None,
        source_commit: None,
        package: None,
    };
    let release_dir = tempfile::tempdir().unwrap();
    let mut journal = Journal::load(release_dir.path()).unwrap();
    let step = publish(
        &channel,
        &item,
        release_dir.path(),
        &mut journal,
        Mode::DryRun,
    )
    .unwrap();
    let Step::WouldPublish(lines) = step else {
        panic!("{step:?}")
    };
    assert!(
        lines.last().unwrap().starts_with("git push --atomic"),
        "{lines:?}"
    );
    assert_eq!(
        channel.latest().unwrap().unwrap().version,
        Version::new(0, 1, 0)
    );

    assert!(matches!(
        publish(
            &channel,
            &item,
            release_dir.path(),
            &mut journal,
            Mode::Execute
        )
        .unwrap(),
        Step::Published(_)
    ));
    let mut reloaded = Journal::load(release_dir.path()).unwrap();
    assert!(matches!(
        publish(
            &channel,
            &item,
            release_dir.path(),
            &mut reloaded,
            Mode::Execute
        )
        .unwrap(),
        Step::Journaled(_)
    ));
    // Lost journal: the tag's tree matches the package, so it is done.
    assert!(matches!(
        publish(
            &channel,
            &item,
            release_dir.path(),
            &mut Journal::default(),
            Mode::Execute
        )
        .unwrap(),
        Step::AlreadyPublished { .. }
    ));
    // A plan made against 0.1.0 for 0.1.2 after 0.1.1 went out.
    let stale = PublishItem {
        version: Version::new(0, 1, 2),
        ..item.clone()
    };
    let error = publish(
        &channel,
        &stale,
        release_dir.path(),
        &mut Journal::default(),
        Mode::Execute,
    )
    .unwrap_err();
    assert!(matches!(error, PublishError::Moved(_)), "{error}");
    // A release someone else tagged at the same version with other bytes.
    fixture.seed(&[("a.txt", "rogue\n")], "v0.1.3", false);
    let rogue = PublishItem {
        version: Version::new(0, 1, 3),
        ..item
    };
    let error = publish(
        &channel,
        &rogue,
        release_dir.path(),
        &mut Journal::default(),
        Mode::Execute,
    )
    .unwrap_err();
    assert!(matches!(error, PublishError::Conflict { .. }), "{error}");
}

#[test]
fn a_damaged_ownership_manifest_stops_the_release() {
    let fixture = Fixture::new();
    fixture.seed(&[(".crucible", "[\"../escape\"]")], "v0.1.0", false);
    let channel = fixture.channel(GitChannelConfig::new(fixture.url()));
    let package = fixture.package("p", &[("a.txt", "1\n")]);
    let error = channel
        .publish(&request(&Version::new(0, 1, 1), &package))
        .unwrap_err();
    assert!(error.to_string().contains("not a relative path"), "{error}");
}

#[test]
fn a_package_without_an_ownership_manifest_is_refused() {
    let fixture = Fixture::new();
    let channel = fixture.channel(GitChannelConfig::new(fixture.url()));
    let package = fixture.package("p", &[("a.txt", "1\n")]);
    fs::remove_file(package.join(".crucible")).unwrap();
    let error = channel
        .publish(&request(&Version::new(0, 1, 0), &package))
        .unwrap_err();
    assert!(matches!(error, ChannelError::Artifact { .. }), "{error}");
    assert!(
        fixture
            .git(None, &["ls-remote", "--tags", &fixture.url()])
            .is_empty()
    );
}

/// The distribution repository's attributes never touch the release:
/// `ident` would rewrite `$Id$` and `text` would turn CRLF into LF, so the
/// tree would hold bytes nobody tested.
#[test]
fn publishes_the_tested_bytes_whatever_the_repositorys_attributes_say() {
    let fixture = Fixture::new();
    fixture.seed(
        &[(".gitattributes", "* text eol=lf\nREADME.md ident\n")],
        "seed",
        false,
    );
    let channel = fixture.channel(GitChannelConfig::new(fixture.url()));
    let package = fixture.package(
        "p",
        &[
            ("README.md", "$Id: reviewed $\r\nCRLF stays\r\n"),
            ("src/lib.rs", "pub fn f() {}\r\n"),
        ],
    );
    let version = Version::new(0, 1, 0);
    channel.publish(&request(&version, &package)).unwrap();
    for path in ["README.md", "src/lib.rs"] {
        let tested = fixture.git(
            None,
            &[
                "hash-object",
                "--no-filters",
                package.join(path).to_str().unwrap(),
            ],
        );
        let published = fixture.git(
            Some(&fixture.remote()),
            &["rev-parse", &format!("v0.1.0:{path}")],
        );
        assert_eq!(tested, published, "{path}");
    }
    assert_eq!(
        channel.check_published(&version, &package).unwrap(),
        Published::SameBytes
    );
    // The repository's own file stays.
    assert!(
        fixture
            .paths("v0.1.0")
            .contains(&".gitattributes".to_owned())
    );
}

/// A file's mode is part of what was published: the same bytes under
/// another mode are not the same release.
#[test]
fn modes_are_compared() {
    let fixture = Fixture::new();
    let work = tempfile::tempdir_in(fixture.root.path()).unwrap();
    let dir = work.path();
    fixture.git(Some(dir), &["init", "-q", "-b", "main"]);
    fs::write(dir.join("a.txt"), "1\n").unwrap();
    fs::write(dir.join(".crucible"), "[\"a.txt\"]").unwrap();
    fixture.git(Some(dir), &["add", "-A"]);
    fixture.git(Some(dir), &["update-index", "--chmod=+x", "a.txt"]);
    fixture.git(Some(dir), &["commit", "-q", "-m", "executable"]);
    fixture.git(Some(dir), &["tag", "v0.1.0"]);
    fixture.git(
        Some(dir),
        &[
            "push",
            "-q",
            &fixture.url(),
            "HEAD:refs/heads/main",
            "refs/tags/v0.1.0",
        ],
    );
    let channel = fixture.channel(GitChannelConfig::new(fixture.url()));
    let package = fixture.package("p", &[("a.txt", "1\n")]);
    let Published::DifferentBytes { detail } = channel
        .check_published(&Version::new(0, 1, 0), &package)
        .unwrap()
    else {
        panic!("a mode change is a difference")
    };
    assert!(detail.contains("mode 100755"), "{detail}");
    // Publishing writes the package's mode, whatever the parent had.
    channel
        .publish(&request(&Version::new(0, 1, 1), &package))
        .unwrap();
    let listing = fixture.git(Some(&fixture.remote()), &["ls-tree", "v0.1.1", "a.txt"]);
    assert!(listing.starts_with("100644 "), "{listing}");
}

/// Runs Git as a hook's child would: `inherited` is in its environment
/// before the command's own changes apply.
struct Inheriting {
    global: PathBuf,
    inherited: Vec<(String, String)>,
}

impl CommandRunner for Inheriting {
    fn run(&self, command: &CommandLine) -> io::Result<CommandOutput> {
        let mut process = Command::new(&command.program);
        process
            .args(&command.args)
            .env("GIT_CONFIG_GLOBAL", &self.global)
            .env("GIT_CONFIG_NOSYSTEM", "1");
        for (name, value) in &self.inherited {
            process.env(name, value);
        }
        for name in &command.env_remove {
            process.env_remove(name);
        }
        process.envs(command.env.iter().map(|(name, value)| (name, value)));
        if let Some(directory) = &command.cwd {
            process.current_dir(directory);
        }
        let output = process.stdin(std::process::Stdio::null()).output()?;
        Ok(CommandOutput {
            success: output.status.success(),
            code: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

/// From a Git hook, GIT_DIR and GIT_INDEX_FILE name the user's repository;
/// a channel's commands must still act only on their own scratch ones.
#[test]
fn a_hooks_git_environment_never_reaches_the_users_repository() {
    let fixture = Fixture::new();
    let user = fixture.root.path().join("user");
    fixture.git(None, &["init", "-q", "-b", "main", user.to_str().unwrap()]);
    fs::write(user.join("mine.txt"), "mine\n").unwrap();
    fixture.git(Some(&user), &["add", "-A"]);
    fixture.git(Some(&user), &["commit", "-q", "-m", "mine"]);
    let git_dir = user.join(".git");
    let index = fs::read(git_dir.join("index")).unwrap();
    let refs_before = fixture.git(Some(&user), &["for-each-ref"]);
    let channel = GitTagChannel::with_runner(
        &GitChannelConfig::new(fixture.url()),
        Box::new(Inheriting {
            global: fixture.runner.global.clone(),
            inherited: vec![
                ("GIT_DIR".into(), git_dir.to_string_lossy().into_owned()),
                (
                    "GIT_INDEX_FILE".into(),
                    git_dir.join("index").to_string_lossy().into_owned(),
                ),
                ("GIT_WORK_TREE".into(), user.to_string_lossy().into_owned()),
            ],
        }),
    )
    .unwrap();
    let package = fixture.package("p", &[("a.txt", "1\n")]);
    let version = Version::new(0, 1, 0);
    channel.publish(&request(&version, &package)).unwrap();
    assert_eq!(
        channel.check_published(&version, &package).unwrap(),
        Published::SameBytes
    );
    channel.read_last().unwrap();
    assert_eq!(fs::read(git_dir.join("index")).unwrap(), index);
    assert_eq!(fixture.git(Some(&user), &["for-each-ref"]), refs_before);
    assert_eq!(fixture.paths("v0.1.0"), [".crucible", "a.txt"]);
}

/// The user's Git configuration cannot make publishing wait or run
/// their hooks: tag.gpgSign would want a message, core.hooksPath and
/// init.templateDir would run hooks on the push.
#[test]
fn publishing_ignores_tag_signing_and_user_hooks() {
    let fixture = Fixture::new();
    let hooks = fixture.root.path().join("hooks");
    let template = fixture.root.path().join("template");
    let marker = fixture.root.path().join("hook-ran");
    fs::create_dir_all(&hooks).unwrap();
    fs::create_dir_all(template.join("hooks")).unwrap();
    let hook = format!(
        "#!/bin/sh\necho ran > '{}'\nexit 1\n",
        marker.to_string_lossy().replace('\\', "/")
    );
    for path in [
        hooks.join("pre-push"),
        hooks.join("reference-transaction"),
        hooks.join("post-index-change"),
        template.join("hooks/pre-push"),
    ] {
        fs::write(&path, &hook).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let quoted = |path: &Path| path.to_string_lossy().replace('\\', "/");
    // The remote stands in for a server, whose own hooks are its business:
    // it must not pick up this user's global hooks.
    let server_hooks = fixture.root.path().join("server-hooks");
    fs::create_dir_all(&server_hooks).unwrap();
    fixture.git(
        Some(&fixture.remote()),
        &["config", "core.hooksPath", &quoted(&server_hooks)],
    );
    fs::write(
        &fixture.runner.global,
        format!(
            "[tag]\n\tgpgSign = true\n[core]\n\thooksPath = {}\n[init]\n\ttemplateDir = {}\n",
            quoted(&hooks),
            quoted(&template)
        ),
    )
    .unwrap();
    let channel = fixture.channel(GitChannelConfig::new(fixture.url()));
    let package = fixture.package("p", &[("a.txt", "1\n")]);
    channel
        .publish(&request(&Version::new(0, 1, 0), &package))
        .unwrap();
    assert!(!marker.exists(), "a user hook ran");
    // A lightweight tag, as ever: nothing signed or annotated.
    let kind = fixture.git(Some(&fixture.remote()), &["cat-file", "-t", "v0.1.0"]);
    assert_eq!(kind, "commit");
}
