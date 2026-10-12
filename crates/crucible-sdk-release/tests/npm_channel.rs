//! The npm channel against an in-memory registry: a fake HTTPS transport
//! that answers like GitHub Packages (a private package reads as 404
//! without credentials; tarballs redirect to a CDN) and a fake `npm` that
//! publishes into it. No network.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;
use std::rc::Rc;

use crucible_sdk_release::archive::{read_tarball_files, sri_sha512};
use crucible_sdk_release::command::{CommandLine, CommandOutput, CommandRunner};
use crucible_sdk_release::http::{Credential, HttpRequest, HttpResponse, HttpTransport};
use crucible_sdk_release::legacy::LegacyRecord;
use crucible_sdk_release::publish::{Mode, PublishError, PublishItem, Step, publish};
use crucible_sdk_release::{
    Channel, ChannelError, ChannelRead, Journal, NpmAccess, NpmChannel, NpmChannelConfig,
    PublishRequest, Published, UploadOutcome, Version,
};
use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::{Value, json};
use url::Url;

const REGISTRY: &str = "https://npm.pkg.github.com";
const PACKAGE: &str = "@operation-pigeon/sdk";

#[derive(Default)]
struct State {
    /// version → tarball bytes.
    versions: BTreeMap<String, Vec<u8>>,
    tags: BTreeMap<String, String>,
    /// A private package: anonymous reads answer 404.
    private: bool,
    /// Answers every package-document read with this status instead.
    status: Option<u16>,
    /// Serves this tarball instead of the real one.
    tamper: Option<Vec<u8>>,
    /// (URL, whether it carried a credential).
    requests: Vec<(String, bool)>,
    commands: Vec<CommandLine>,
    /// Makes `npm publish` fail.
    refuse_publish: bool,
    /// An `.npmrc` sends the scope elsewhere (`@operation-pigeon:registry=
    /// https://registry.npmjs.org/`), as npm honours over `--registry`
    /// unless the command pins the scope itself.
    scope_elsewhere: bool,
    /// An inherited `dry-run=true`, unless the command says otherwise.
    inherited_dry_run: bool,
    /// npm reports success and does nothing, whatever it is told.
    swallow: bool,
    /// What `npm --version` prints.
    npm_version: Option<String>,
}

#[derive(Clone, Default)]
struct Registry(Rc<RefCell<State>>);

impl Registry {
    fn with(versions: &[(&str, Vec<u8>)], latest: &str) -> Self {
        let registry = Self::default();
        {
            let mut state = registry.0.borrow_mut();
            for (version, tgz) in versions {
                state.versions.insert((*version).into(), tgz.clone());
            }
            if !latest.is_empty() {
                state.tags.insert("latest".into(), latest.into());
            }
        }
        registry
    }

    fn packument(&self) -> Value {
        let state = self.0.borrow();
        let versions: serde_json::Map<String, Value> = state
            .versions
            .iter()
            .map(|(version, tgz)| {
                (
                    version.clone(),
                    json!({
                        "name": PACKAGE,
                        "version": version,
                        "dist": {
                            "tarball": format!("{REGISTRY}/download/@operation-pigeon/sdk/{version}/sdk-{version}.tgz"),
                            "integrity": sri_sha512(tgz),
                            "shasum": "0000",
                        },
                    }),
                )
            })
            .collect();
        json!({ "name": PACKAGE, "dist-tags": state.tags, "versions": versions })
    }

    fn requests(&self) -> Vec<(String, bool)> {
        self.0.borrow().requests.clone()
    }

    fn commands(&self) -> Vec<String> {
        self.0
            .borrow()
            .commands
            .iter()
            .map(ToString::to_string)
            .collect()
    }
}

fn response(status: u16, body: Vec<u8>) -> HttpResponse {
    HttpResponse {
        status,
        location: None,
        body,
    }
}

impl HttpTransport for Registry {
    fn get(&self, request: &HttpRequest<'_>) -> Result<HttpResponse, String> {
        let url = request.url;
        self.0
            .borrow_mut()
            .requests
            .push((url.to_string(), request.authorization.is_some()));
        let authorized = request.authorization == Some("Bearer ghp_token");
        let state = self.0.borrow();
        match (url.host_str(), url.path()) {
            (Some("npm.pkg.github.com"), "/@operation-pigeon%2fsdk") => {
                if let Some(status) = state.status {
                    return Ok(response(status, Vec::new()));
                }
                if (state.private && !authorized) || state.versions.is_empty() {
                    return Ok(response(404, b"{}".to_vec()));
                }
                drop(state);
                Ok(response(200, self.packument().to_string().into_bytes()))
            }
            (Some("npm.pkg.github.com"), path) if path.starts_with("/download/") => {
                if state.private && !authorized {
                    return Ok(response(404, Vec::new()));
                }
                let version = path.split('/').nth(4).unwrap_or_default();
                Ok(HttpResponse {
                    status: 302,
                    location: Some(format!(
                        "https://pkg-npm.githubusercontent.com/{version}.tgz?X-Signature=abc"
                    )),
                    body: Vec::new(),
                })
            }
            (Some("pkg-npm.githubusercontent.com"), path) => {
                let version = path.trim_start_matches('/').trim_end_matches(".tgz");
                let body = state
                    .tamper
                    .clone()
                    .or_else(|| state.versions.get(version).cloned());
                Ok(body.map_or_else(|| response(404, Vec::new()), |b| response(200, b)))
            }
            _ => Ok(response(404, Vec::new())),
        }
    }
}

impl CommandRunner for Registry {
    fn run(&self, command: &CommandLine) -> io::Result<CommandOutput> {
        let mut state = self.0.borrow_mut();
        if command.args.first().map(String::as_str) == Some("--version") {
            return Ok(CommandOutput {
                success: true,
                stdout: state
                    .npm_version
                    .clone()
                    .unwrap_or_else(|| "10.9.2".into())
                    .into_bytes(),
                ..CommandOutput::default()
            });
        }
        state.commands.push(command.clone());
        let flag = |name: &str| {
            command
                .args
                .iter()
                .position(|arg| arg == name)
                .map(|at| command.args[at + 1].clone())
        };
        let has = |arg: &str| command.args.iter().any(|candidate| candidate == arg);
        let lands = !state.swallow
            && (!state.scope_elsewhere
                || has("--@operation-pigeon:registry=https://npm.pkg.github.com/"))
            && (!state.inherited_dry_run || has("--dry-run=false"));
        if !lands
            && command
                .args
                .first()
                .is_some_and(|arg| arg != "publish" || !state.refuse_publish)
        {
            return Ok(CommandOutput {
                success: true,
                ..CommandOutput::default()
            });
        }
        match command.args.first().map(String::as_str) {
            Some("publish") if state.refuse_publish => Ok(CommandOutput {
                success: false,
                code: Some(1),
                stdout: Vec::new(),
                stderr: b"npm ERR! 403 Forbidden ghp_token".to_vec(),
            }),
            Some("publish") => {
                let tgz = fs::read(&command.args[1])?;
                let files = read_tarball_files(&tgz, &["package.json"], 1 << 20).unwrap();
                let package: Value = serde_json::from_slice(&files["package.json"]).unwrap();
                let version = package["version"].as_str().unwrap().to_owned();
                assert!(
                    !state.versions.contains_key(&version),
                    "npm refuses a re-publish"
                );
                state.versions.insert(version.clone(), tgz);
                state.tags.insert(flag("--tag").unwrap(), version);
                Ok(CommandOutput {
                    success: true,
                    ..CommandOutput::default()
                })
            }
            Some("dist-tag") => {
                let spec = &command.args[2];
                let version = spec.rsplit_once('@').unwrap().1.to_owned();
                state.tags.insert(command.args[3].clone(), version);
                Ok(CommandOutput {
                    success: true,
                    ..CommandOutput::default()
                })
            }
            _ => panic!("unexpected command {command}"),
        }
    }
}

fn tarball(package_json: Value, extra: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    let package = serde_json::to_vec_pretty(&package_json).unwrap();
    let mut files: Vec<(&str, &[u8])> = vec![("package/package.json", &package)];
    let owned: Vec<(String, &[u8])> = extra
        .iter()
        .map(|(path, contents)| (format!("package/{path}"), *contents))
        .collect();
    files.extend(
        owned
            .iter()
            .map(|(path, contents)| (path.as_str(), *contents)),
    );
    for (path, contents) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, path, contents).unwrap();
    }
    builder.into_inner().unwrap().finish().unwrap()
}

fn release(version: &str, digest: &str) -> Vec<u8> {
    // A short name stands for a well-formed digest.
    let digest = if digest.contains(':') {
        digest.to_owned()
    } else {
        let hex: String = digest.bytes().map(|byte| format!("{byte:02x}")).collect();
        format!("crucible-content-1:sha256:{hex}")
    };
    let provenance = json!({
        "schemaVersion": 2, "generator": "crucible", "language": "ts", "package": PACKAGE,
        "packageVersion": version, "service": "vip.mailpigeon#Pigeon", "serviceVersion": "2026-10-01",
        "contentDigest": digest,
    })
    .to_string();
    tarball(
        json!({
            "name": PACKAGE, "version": version,
            "publishConfig": { "registry": "https://npm.pkg.github.com/", "access": "restricted" },
        }),
        &[
            ("crucible.json", provenance.as_bytes()),
            ("crucible-model.json", b"{\"smithy\":\"2.0\"}"),
            ("dist/index.js", b"export {};\n"),
        ],
    )
}

fn credential() -> Credential {
    Credential::bearer(&Url::parse(REGISTRY).unwrap(), "ghp_token").unwrap()
}

fn config(access: Option<NpmAccess>) -> NpmChannelConfig {
    NpmChannelConfig {
        access,
        ..NpmChannelConfig::new(REGISTRY, PACKAGE)
    }
}

fn channel(
    registry: &Registry,
    credential: Option<Credential>,
    access: Option<NpmAccess>,
) -> NpmChannel {
    NpmChannel::new(
        &config(access),
        credential,
        Box::new(registry.clone()),
        Box::new(registry.clone()),
    )
    .unwrap()
    .with_npm("npm")
    .with_confirmation(2, std::time::Duration::ZERO)
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn reads_the_last_release_from_inside_its_tarball() {
    let registry = Registry::with(
        &[
            ("0.1.0", release("0.1.0", "crucible-content-1:sha256:01")),
            ("0.10.0", release("0.10.0", "crucible-content-1:sha256:10")),
            ("0.9.0", release("0.9.0", "crucible-content-1:sha256:09")),
            (
                "1.0.0-rc.1",
                release("1.0.0-rc.1", "crucible-content-1:sha256:rc"),
            ),
        ],
        // A dist-tag pointing back does not decide the baseline.
        "0.9.0",
    );
    registry.0.borrow_mut().private = true;
    let npm = channel(&registry, Some(credential()), Some(NpmAccess::Restricted));
    let read = npm.read_last().unwrap();
    let last = read.last().unwrap();
    assert_eq!(last.version, Version::new(0, 10, 0));
    assert_eq!(last.content_digest(), Some("crucible-content-1:sha256:10"));
    assert_eq!(last.service_version(), Some("2026-10-01"));
    assert_eq!(last.model.as_deref(), Some("{\"smithy\":\"2.0\"}"));
    assert!(last.artifact.starts_with("sha512-"));
    assert_eq!(last.legacy, None);
    // The token went to the registry, never to the CDN the tarball
    // redirects to.
    let requests = registry.requests();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].1 && requests[1].1, "{requests:?}");
    assert!(
        requests[2]
            .0
            .starts_with("https://pkg-npm.githubusercontent.com/")
    );
    assert!(
        !requests[2].1,
        "the credential followed a redirect off its origin"
    );
}

#[test]
fn imports_pigeons_legacy_typescript_release() {
    // What package_sdks.py published: a contractHash in package.json and the
    // schema-1 crucible.json the generator wrote.
    let legacy = tarball(
        json!({
            "name": PACKAGE, "version": "0.1.7", "contractHash": "a03f504b",
            "publishConfig": { "registry": "https://npm.pkg.github.com", "access": "restricted" },
        }),
        &[(
            "crucible.json",
            br#"{"generatorVersion":"0.5.0","service":"vip.mailpigeon#Pigeon","serviceVersion":"2026-01-01","schemaSha256":"ab"}"#,
        )],
    );
    let registry = Registry::with(&[("0.1.7", legacy)], "0.1.7");
    let npm = channel(&registry, Some(credential()), None);
    let read = npm.read_last().unwrap();
    let last = read.last().unwrap();
    assert_eq!(last.provenance.as_ref().unwrap().schema_version, 1);
    assert_eq!(last.content_digest(), None);
    assert_eq!(
        last.legacy,
        Some(LegacyRecord::ContractHash {
            contract_hash: "a03f504b".into()
        })
    );
    // So the first crucible sdk release counts as changed and takes a patch.
    let decision = crucible_sdk_release::version::choose(
        &crucible_sdk_release::version::Inputs::new("ts", &read),
    )
    .unwrap();
    assert_eq!(decision.version, Some(Version::new(0, 1, 8)));
}

#[test]
fn a_404_is_empty_only_when_it_can_be_trusted() {
    let registry = Registry::default();
    // With credentials, 404 is "no such package".
    assert_eq!(
        channel(&registry, Some(credential()), None)
            .read_last()
            .unwrap(),
        ChannelRead::Empty
    );
    // Without, a restricted (or unspecified) package could be private.
    for access in [None, Some(NpmAccess::Restricted)] {
        let error = channel(&registry, None, access).read_last().unwrap_err();
        assert!(matches!(error, ChannelError::Unreadable { .. }), "{error}");
        assert!(error.to_string().contains("_authToken"), "{error}");
    }
    // A public package's anonymous 404 is trustworthy.
    assert_eq!(
        channel(&registry, None, Some(NpmAccess::Public))
            .read_last()
            .unwrap(),
        ChannelRead::Empty
    );
}

#[test]
fn refusals_and_server_errors_are_unreadable_not_empty() {
    for status in [401, 403, 500, 502] {
        let registry = Registry::with(&[("0.1.0", release("0.1.0", "d"))], "0.1.0");
        registry.0.borrow_mut().status = Some(status);
        let error = channel(&registry, Some(credential()), None)
            .read_last()
            .unwrap_err();
        assert!(matches!(error, ChannelError::Unreadable { .. }), "{status}");
        assert!(!error.to_string().contains("ghp_token"));
    }
}

#[test]
fn a_tarball_that_is_not_what_the_registry_lists_is_refused() {
    let registry = Registry::with(&[("0.1.0", release("0.1.0", "d"))], "0.1.0");
    registry.0.borrow_mut().tamper = Some(release("0.1.0", "other"));
    let error = channel(&registry, Some(credential()), None)
        .read_last()
        .unwrap_err();
    assert!(error.to_string().contains("dist.integrity"), "{error}");
    // A tarball listed as 0.1.0 that says it is 0.0.9.
    let registry = Registry::with(&[("0.1.0", release("0.0.9", "d"))], "0.1.0");
    let error = channel(&registry, Some(credential()), None)
        .read_last()
        .unwrap_err();
    assert!(error.to_string().contains("listed as 0.1.0"), "{error}");
}

#[test]
fn compares_a_published_version_by_its_bytes() {
    let tested = release("0.2.0", "d2");
    let registry = Registry::with(&[("0.2.0", tested.clone())], "0.2.0");
    let npm = channel(&registry, Some(credential()), None);
    let dir = tempfile::tempdir().unwrap();
    let same = write(dir.path(), "same.tgz", &tested);
    let other = write(dir.path(), "other.tgz", &release("0.2.0", "d3"));
    assert_eq!(
        npm.check_published(&Version::new(0, 2, 0), &same).unwrap(),
        Published::SameBytes
    );
    assert!(matches!(
        npm.check_published(&Version::new(0, 2, 0), &other).unwrap(),
        Published::DifferentBytes { .. }
    ));
    assert_eq!(
        npm.check_published(&Version::new(0, 2, 1), &same).unwrap(),
        Published::Absent
    );
}

#[test]
fn publishes_the_tested_tarball_with_the_channels_settings() {
    let registry = Registry::with(&[("0.2.0", release("0.2.0", "d2"))], "0.2.0");
    let npm = channel(&registry, Some(credential()), Some(NpmAccess::Restricted));
    let dir = tempfile::tempdir().unwrap();
    let artifact = write(dir.path(), "sdk-0.2.1.tgz", &release("0.2.1", "d3"));
    let version = Version::new(0, 2, 1);
    let request = PublishRequest {
        sdk: "ts",
        version: &version,
        artifact: &artifact,
        source_commit: None,
        package: None,
    };
    let preview = npm.describe_publish(&request).unwrap();
    let outcome = npm.publish(&request).unwrap();
    let commands = registry.commands();
    assert_eq!(commands.len(), 1);
    assert_eq!(preview, commands);
    assert!(
        commands[0].ends_with(
            "--ignore-scripts --tag latest --registry https://npm.pkg.github.com/ --@operation-pigeon:registry=https://npm.pkg.github.com/ --dry-run=false --access restricted"
        ),
        "{commands:?}"
    );
    assert!(outcome.detail.contains("dist-tag latest"));
    assert_eq!(
        npm.check_published(&version, &artifact).unwrap(),
        Published::SameBytes
    );
}

#[test]
fn refuses_a_tarball_bound_elsewhere() {
    let registry = Registry::with(&[("0.2.0", release("0.2.0", "d2"))], "0.2.0");
    let npm = channel(&registry, Some(credential()), Some(NpmAccess::Restricted));
    let dir = tempfile::tempdir().unwrap();
    let version = Version::new(0, 2, 1);
    let cases = [
        (
            json!({"name": "other", "version": "0.2.1"}),
            "not @operation-pigeon/sdk",
        ),
        (json!({"name": PACKAGE, "version": "0.2.2"}), "not 0.2.1"),
        (
            json!({"name": PACKAGE, "version": "0.2.1", "private": true}),
            "private",
        ),
        (
            json!({"name": PACKAGE, "version": "0.2.1", "publishConfig": {"registry": "https://registry.npmjs.org/"}}),
            "publishConfig.registry",
        ),
        (
            json!({"name": PACKAGE, "version": "0.2.1", "publishConfig": {"access": "public"}}),
            "publishConfig.access",
        ),
    ];
    for (package, expected) in cases {
        let artifact = write(dir.path(), "a.tgz", &tarball(package, &[]));
        let error = npm
            .publish(&PublishRequest {
                sdk: "ts",
                version: &version,
                artifact: &artifact,
                source_commit: None,
                package: None,
            })
            .unwrap_err();
        assert!(matches!(error, ChannelError::Artifact { .. }), "{error}");
        assert!(error.to_string().contains(expected), "{error}");
    }
    assert!(registry.commands().is_empty());
}

#[test]
fn a_failed_publish_never_shows_the_token() {
    let registry = Registry::with(&[("0.2.0", release("0.2.0", "d2"))], "0.2.0");
    registry.0.borrow_mut().refuse_publish = true;
    let npm = channel(&registry, Some(credential()), None);
    let dir = tempfile::tempdir().unwrap();
    let artifact = write(dir.path(), "a.tgz", &release("0.2.1", "d3"));
    let error = npm
        .publish(&PublishRequest {
            sdk: "ts",
            version: &Version::new(0, 2, 1),
            artifact: &artifact,
            source_commit: None,
            package: None,
        })
        .unwrap_err();
    let shown = error.to_string();
    assert!(shown.contains("403 Forbidden"), "{shown}");
    assert!(!shown.contains("ghp_token"), "{shown}");
}

#[test]
fn the_dist_tag_never_moves_backwards() {
    // latest already points past the version being published (a manual
    // tag, or a later line): it stays, and the release gets its own tag.
    let registry = Registry::with(
        &[
            ("0.2.0", release("0.2.0", "d")),
            ("0.5.0", release("0.5.0", "d")),
        ],
        "0.5.0",
    );
    let npm = channel(&registry, Some(credential()), None);
    let dir = tempfile::tempdir().unwrap();
    let artifact = write(dir.path(), "a.tgz", &release("0.2.1", "d3"));
    let version = Version::new(0, 2, 1);
    let request = PublishRequest {
        sdk: "ts",
        version: &version,
        artifact: &artifact,
        source_commit: None,
        package: None,
    };
    let preview = npm.describe_publish(&request).unwrap();
    assert!(preview[1].contains("stays"), "{preview:?}");
    npm.publish(&request).unwrap();
    assert!(registry.commands()[0].contains("--tag latest-0.2"));
    assert_eq!(registry.0.borrow().tags["latest"], "0.5.0");
    // settle points the tag forward, never back.
    assert!(npm.settle(&version).unwrap().is_empty());
    registry
        .0
        .borrow_mut()
        .tags
        .insert("latest".into(), "0.2.0".into());
    assert_eq!(npm.settle(&version).unwrap().len(), 1);
    assert_eq!(registry.0.borrow().tags["latest"], "0.2.1");
    assert!(
        registry.commands()[1]
            .starts_with("npm dist-tag add @operation-pigeon/sdk@0.2.1 latest --registry")
    );
}

#[test]
fn configurations_are_checked() {
    let registry = Registry::default();
    let make = |config: NpmChannelConfig| {
        NpmChannel::new(
            &config,
            None,
            Box::new(registry.clone()),
            Box::new(registry.clone()),
        )
    };
    assert!(make(NpmChannelConfig::new("http://npm.pkg.github.com", PACKAGE)).is_err());
    assert!(
        make(NpmChannelConfig::new(
            "https://t:x@npm.pkg.github.com",
            PACKAGE
        ))
        .is_err()
    );
    assert!(make(NpmChannelConfig::new(REGISTRY, "Not/Valid")).is_err());
    let mut tag = NpmChannelConfig::new(REGISTRY, PACKAGE);
    tag.dist_tag = "1.2.3".into();
    assert!(make(tag).is_err());
}

/// The resumable publish, end to end on the npm channel.
#[test]
fn publish_resumes_and_stops_on_a_moved_channel() {
    let registry = Registry::with(&[("0.2.0", release("0.2.0", "d2"))], "0.2.0");
    registry.0.borrow_mut().private = true;
    let npm = channel(&registry, Some(credential()), Some(NpmAccess::Restricted));
    let baseline = npm.latest().unwrap().unwrap().baseline();
    let release_dir = tempfile::tempdir().unwrap();
    let tested = release("0.2.1", "d3");
    let artifact = write(release_dir.path(), "sdk-0.2.1.tgz", &tested);
    let item = PublishItem {
        sdk: "ts".into(),
        version: Version::new(0, 2, 1),
        baseline: Some(baseline.clone()),
        artifact: artifact.clone(),
        artifact_sha256: Some(crucible_sdk_release::archive::sha256_file(&artifact).unwrap()),
        source_commit: None,
        package: None,
    };
    let mut journal = Journal::load(release_dir.path()).unwrap();

    // Without --execute: says what it would do, uploads and journals nothing.
    let step = publish(&npm, &item, release_dir.path(), &mut journal, Mode::DryRun).unwrap();
    assert!(matches!(&step, Step::WouldPublish(lines) if lines[0].starts_with("npm publish")));
    assert!(registry.commands().is_empty());
    assert!(!Journal::path(release_dir.path()).exists());

    let step = publish(&npm, &item, release_dir.path(), &mut journal, Mode::Execute).unwrap();
    assert!(matches!(step, Step::Published(_)));
    assert_eq!(registry.commands().len(), 1);

    // A rerun with the journal skips it without asking the registry.
    let before = registry.requests().len();
    let mut reloaded = Journal::load(release_dir.path()).unwrap();
    let step = publish(
        &npm,
        &item,
        release_dir.path(),
        &mut reloaded,
        Mode::Execute,
    )
    .unwrap();
    assert!(matches!(step, Step::Journaled(_)));
    assert_eq!(registry.requests().len(), before);

    // A rerun that lost publication.json finds the same bytes and is done.
    fs::remove_file(Journal::path(release_dir.path())).unwrap();
    let mut fresh = Journal::load(release_dir.path()).unwrap();
    let step = publish(&npm, &item, release_dir.path(), &mut fresh, Mode::Execute).unwrap();
    assert!(matches!(step, Step::AlreadyPublished { .. }));
    assert_eq!(fresh.uploads[0].outcome, UploadOutcome::AlreadyPublished);
    assert_eq!(registry.commands().len(), 1, "never re-published");

    // A plan made against 0.2.0 for 0.2.2, after 0.2.1 went out: moved.
    let next = write(release_dir.path(), "sdk-0.2.2.tgz", &release("0.2.2", "d4"));
    let stale = PublishItem {
        version: Version::new(0, 2, 2),
        artifact: next,
        artifact_sha256: None,
        ..item.clone()
    };
    let error = publish(&npm, &stale, release_dir.path(), &mut fresh, Mode::Execute).unwrap_err();
    assert!(matches!(error, PublishError::Moved(_)), "{error}");
    assert!(error.to_string().contains("make a new plan"), "{error}");

    // Other bytes under the plan's version: a conflict, never a replace.
    let conflicting = write(release_dir.path(), "other.tgz", &release("0.2.1", "zz"));
    let other = PublishItem {
        artifact: conflicting,
        artifact_sha256: None,
        ..item.clone()
    };
    let error = publish(
        &npm,
        &other,
        release_dir.path(),
        &mut Journal::default(),
        Mode::Execute,
    )
    .unwrap_err();
    assert!(matches!(error, PublishError::Conflict { .. }), "{error}");

    // The artifact changed since release.json recorded it.
    fs::write(&artifact, release("0.2.1", "tampered")).unwrap();
    let error = publish(
        &npm,
        &item,
        release_dir.path(),
        &mut Journal::default(),
        Mode::Execute,
    )
    .unwrap_err();
    assert!(
        matches!(error, PublishError::ArtifactChanged { .. }),
        "{error}"
    );
}

#[test]
fn a_first_release_publishes_to_an_empty_channel() {
    let registry = Registry::default();
    let npm = channel(&registry, Some(credential()), None);
    let dir = tempfile::tempdir().unwrap();
    let artifact = write(dir.path(), "a.tgz", &release("0.1.0", "d"));
    let item = PublishItem {
        sdk: "ts".into(),
        version: Version::new(0, 1, 0),
        baseline: None,
        artifact,
        artifact_sha256: None,
        source_commit: None,
        package: None,
    };
    let step = publish(
        &npm,
        &item,
        dir.path(),
        &mut Journal::default(),
        Mode::Execute,
    )
    .unwrap();
    assert!(matches!(step, Step::Published(_)));
    // The same plan again (a rerun) is recognised, not re-published.
    let step = publish(
        &npm,
        &item,
        dir.path(),
        &mut Journal::default(),
        Mode::Execute,
    )
    .unwrap();
    assert!(matches!(step, Step::AlreadyPublished { .. }));
}

/// npm sends a scoped package wherever an `.npmrc` scope says, and an
/// inherited `dry-run` makes it upload nothing: the commands pin both, so
/// the release lands on the channel's registry.
#[test]
fn the_scope_registry_and_dry_run_are_pinned_on_every_command() {
    let registry = Registry::with(&[("0.2.0", release("0.2.0", "d2"))], "0.2.0");
    {
        let mut state = registry.0.borrow_mut();
        state.scope_elsewhere = true;
        state.inherited_dry_run = true;
    }
    let npm = channel(&registry, Some(credential()), None);
    let dir = tempfile::tempdir().unwrap();
    let artifact = write(dir.path(), "a.tgz", &release("0.2.1", "d3"));
    let version = Version::new(0, 2, 1);
    npm.publish(&PublishRequest {
        sdk: "ts",
        version: &version,
        artifact: &artifact,
        source_commit: None,
        package: None,
    })
    .unwrap();
    assert!(registry.0.borrow().versions.contains_key("0.2.1"));
    registry
        .0
        .borrow_mut()
        .tags
        .insert("latest".into(), "0.2.0".into());
    npm.settle(&version).unwrap();
    assert_eq!(registry.0.borrow().tags["latest"], "0.2.1");
    for command in registry.commands() {
        assert!(
            command.contains("--@operation-pigeon:registry=https://npm.pkg.github.com/")
                && command.contains("--dry-run=false"),
            "{command}"
        );
    }
}

/// An npm that reports success without uploading is caught, and nothing
/// is journalled: the next run publishes for real.
#[test]
fn a_success_the_registry_does_not_show_is_not_recorded() {
    let registry = Registry::with(&[("0.2.0", release("0.2.0", "d2"))], "0.2.0");
    registry.0.borrow_mut().swallow = true;
    let npm = channel(&registry, Some(credential()), None);
    let dir = tempfile::tempdir().unwrap();
    let artifact = write(dir.path(), "a.tgz", &release("0.2.1", "d3"));
    let item = PublishItem {
        sdk: "ts".into(),
        version: Version::new(0, 2, 1),
        baseline: Some(npm.latest().unwrap().unwrap().baseline()),
        artifact,
        artifact_sha256: None,
        source_commit: None,
        package: None,
    };
    let mut journal = Journal::default();
    let error = publish(&npm, &item, dir.path(), &mut journal, Mode::Execute).unwrap_err();
    assert!(error.to_string().contains("does not show"), "{error}");
    assert!(journal.uploads.is_empty());
    assert!(!Journal::path(dir.path()).exists());
    registry.0.borrow_mut().swallow = false;
    let step = publish(&npm, &item, dir.path(), &mut journal, Mode::Execute).unwrap();
    assert!(matches!(step, Step::Published(_)));
}

/// A credential skipped for an unset variable is named when the channel
/// cannot be read without it.
#[test]
fn an_unreadable_channel_says_which_credential_was_skipped() {
    let registry = Registry::default();
    let error = channel(&registry, None, Some(NpmAccess::Restricted))
        .with_credential_note(
            "the .npmrc _authToken for //npm.pkg.github.com/ names ${NODE_AUTH_TOKEN}, which is not set",
        )
        .read_last()
        .unwrap_err();
    assert!(error.to_string().contains("${NODE_AUTH_TOKEN}"), "{error}");
}

/// A journal entry skips an upload only for the same channel and bytes: a
/// restored or stale journal must not hide a different release.
#[test]
fn a_journal_entry_for_other_bytes_or_another_channel_is_refused() {
    let registry = Registry::with(&[("0.2.0", release("0.2.0", "d2"))], "0.2.0");
    let npm = channel(&registry, Some(credential()), None);
    let dir = tempfile::tempdir().unwrap();
    let artifact = write(dir.path(), "a.tgz", &release("0.2.1", "d3"));
    let item = PublishItem {
        sdk: "ts".into(),
        version: Version::new(0, 2, 1),
        baseline: Some(npm.latest().unwrap().unwrap().baseline()),
        artifact,
        artifact_sha256: None,
        source_commit: None,
        package: None,
    };
    let upload = |channel: String, sha: &str| crucible_sdk_release::Upload {
        sdk: "ts".into(),
        version: Version::new(0, 2, 1),
        channel,
        artifact_sha256: Some(sha.into()),
        outcome: UploadOutcome::Published,
        detail: String::new(),
        completed_at: 0,
    };
    let actual = crucible_sdk_release::archive::sha256_file(&item.artifact).unwrap();
    for stale in [
        upload(npm.describe(), "0000"),
        upload(
            "npm @operation-pigeon/sdk on https://registry.npmjs.org/".into(),
            &actual,
        ),
    ] {
        let mut journal = Journal::default();
        journal.uploads.push(stale);
        let error = publish(&npm, &item, dir.path(), &mut journal, Mode::Execute).unwrap_err();
        assert!(
            matches!(error, PublishError::JournalMismatch { .. }),
            "{error}"
        );
    }
    assert!(registry.commands().is_empty());
    let mut journal = Journal::default();
    journal.uploads.push(upload(npm.describe(), &actual));
    let step = publish(&npm, &item, dir.path(), &mut journal, Mode::Execute).unwrap();
    assert!(matches!(step, Step::Journaled(_)));
}

fn request<'a>(version: &'a Version, artifact: &'a Path) -> PublishRequest<'a> {
    PublishRequest {
        sdk: "ts",
        version,
        artifact,
        source_commit: None,
        package: None,
    }
}

/// npm publishes only with the credential at the registry's exact key:
/// a gap is reported before anything runs, in the dry run too.
#[test]
fn a_publish_npm_would_refuse_for_its_credential_stops_first() {
    let registry = Registry::with(&[("0.2.0", release("0.2.0", "d2"))], "0.2.0");
    let npm = channel(&registry, Some(credential()), None).with_publish_problem(
        "npm publish reads only `//npm.pkg.github.com/:_authToken`, but the credential is at `//npm.pkg.github.com`",
    );
    let dir = tempfile::tempdir().unwrap();
    let artifact = write(dir.path(), "a.tgz", &release("0.2.1", "d3"));
    let version = Version::new(0, 2, 1);
    for error in [
        npm.publish(&request(&version, &artifact)).unwrap_err(),
        npm.describe_publish(&request(&version, &artifact))
            .unwrap_err(),
        npm.settle(&version).unwrap_err(),
    ] {
        assert!(error.to_string().contains("reads only"), "{error}");
    }
    assert!(registry.commands().is_empty());
}

#[test]
fn publishing_needs_npm_9() {
    let registry = Registry::with(&[("0.2.0", release("0.2.0", "d2"))], "0.2.0");
    registry.0.borrow_mut().npm_version = Some("8.19.4".into());
    let npm = channel(&registry, Some(credential()), None);
    let dir = tempfile::tempdir().unwrap();
    let artifact = write(dir.path(), "a.tgz", &release("0.2.1", "d3"));
    let error = npm
        .publish(&request(&Version::new(0, 2, 1), &artifact))
        .unwrap_err();
    assert!(error.to_string().contains("npm 9 or later"), "{error}");
    assert!(!registry.0.borrow().versions.contains_key("0.2.1"));
}

/// npm runs in its project directory, the one whose .npmrc was read.
#[test]
fn npm_runs_in_its_project_directory() {
    let registry = Registry::with(&[("0.2.0", release("0.2.0", "d2"))], "0.2.0");
    let project = tempfile::tempdir().unwrap();
    let npm = channel(&registry, Some(credential()), None).with_project(project.path());
    let artifact = write(project.path(), "a.tgz", &release("0.2.1", "d3"));
    npm.publish(&request(&Version::new(0, 2, 1), &artifact))
        .unwrap();
    let state = registry.0.borrow();
    assert!(!state.commands.is_empty());
    for command in &state.commands {
        assert_eq!(command.cwd.as_deref(), Some(project.path()), "{command}");
    }
}

/// Only the publishConfig keys the generator writes are accepted: any
/// other could redirect or change the upload.
#[test]
fn other_publish_config_keys_are_refused() {
    let registry = Registry::with(&[("0.2.0", release("0.2.0", "d2"))], "0.2.0");
    let npm = channel(&registry, Some(credential()), None);
    let dir = tempfile::tempdir().unwrap();
    for key in ["@operation-pigeon:registry", "tag", "dry-run", "provenance"] {
        let mut config = serde_json::Map::new();
        config.insert(key.into(), json!("x"));
        let artifact = write(
            dir.path(),
            "a.tgz",
            &tarball(
                json!({"name": PACKAGE, "version": "0.2.1", "publishConfig": config}),
                &[],
            ),
        );
        let error = npm
            .publish(&request(&Version::new(0, 2, 1), &artifact))
            .unwrap_err();
        assert!(error.to_string().contains("publishConfig sets"), "{error}");
    }
    assert!(registry.commands().is_empty());
}
