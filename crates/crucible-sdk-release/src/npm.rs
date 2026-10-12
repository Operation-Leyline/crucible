//! An npm registry channel, GitHub Packages included
//! (`{ npm = "<registry url>", access = "restricted", dist_tag = "latest" }`).
//!
//! The registry's package document lists the versions and each tarball's
//! `dist.integrity`; the provenance is read from inside the tarball, never
//! from the document, since GitHub Packages drops `package.json` fields it
//! does not know. Publishing runs `npm publish <tgz> --ignore-scripts` on
//! the tested tarball, and `npm` finds its credential in the same `.npmrc`
//! this channel reads.

use std::fs;
use std::path::Path;
use std::time::Duration;

use semver::Version;
use serde_json::Value;
use url::Url;

use crate::archive::{read_tarball_files, sri_matches};
use crate::channel::{Channel, ChannelError, PublishOutcome, PublishRequest, Published};
use crate::command::{CommandLine, CommandRunner, SystemRunner};
use crate::http::{self, Credential, HttpTransport, UreqTransport};
use crate::legacy::LegacyRecord;
use crate::npmrc;
use crate::provenance::{MODEL_FILE, PROVENANCE_FILE, Provenance};
use crate::release::{LastRelease, ReleaseRef, highest_stable};

/// The most a package document may weigh.
pub const PACKUMENT_LIMIT: u64 = 64 << 20;
/// The most a tarball may weigh.
pub const TARBALL_LIMIT: u64 = 512 << 20;

/// `npm publish --access`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NpmAccess {
    Public,
    Restricted,
}

impl NpmAccess {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "public" => Some(Self::Public),
            "restricted" => Some(Self::Restricted),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Restricted => "restricted",
        }
    }
}

/// One package on one npm registry.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NpmChannelConfig {
    /// The registry URL: the channel's `npm`, which the generated
    /// `package.json` also names as `publishConfig.registry`.
    pub registry: String,
    /// The npm package name, `@scope/name` or `name`.
    pub package: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access: Option<NpmAccess>,
    /// The dist-tag a release moves (`latest`), never backwards.
    pub dist_tag: String,
}

impl NpmChannelConfig {
    pub fn new(registry: impl Into<String>, package: impl Into<String>) -> Self {
        Self {
            registry: registry.into(),
            package: package.into(),
            access: None,
            dist_tag: "latest".into(),
        }
    }
}

/// See the [module documentation](self).
pub struct NpmChannel {
    config: NpmChannelConfig,
    registry: Url,
    credential: Option<Credential>,
    http: Box<dyn HttpTransport>,
    runner: Box<dyn CommandRunner>,
    npm: String,
    /// How often, and how far apart, an upload is looked for on the
    /// registry before it counts as done.
    confirm_attempts: u32,
    confirm_delay: Duration,
    /// Why a configured credential was skipped, for an unreadable channel's
    /// message.
    credential_note: Option<String>,
}

/// What the registry's package document says.
#[derive(Debug, Clone, Default)]
struct Packument {
    versions: Vec<(Version, Dist)>,
    dist_tags: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
struct Dist {
    tarball: String,
    integrity: Option<String>,
}

impl Packument {
    fn dist(&self, version: &Version) -> Option<&Dist> {
        self.versions
            .iter()
            .find(|(candidate, _)| candidate == version)
            .map(|(_, dist)| dist)
    }

    fn tag(&self, tag: &str) -> Option<Version> {
        self.dist_tags
            .iter()
            .find(|(name, _)| name == tag)
            .and_then(|(_, version)| Version::parse(version).ok())
    }
}

impl NpmChannel {
    /// A channel with the given credential and transports.
    pub fn new(
        config: &NpmChannelConfig,
        credential: Option<Credential>,
        http: Box<dyn HttpTransport>,
        runner: Box<dyn CommandRunner>,
    ) -> Result<Self, ChannelError> {
        let invalid = |reason: String| ChannelError::Invalid {
            channel: format!("npm {}", config.package),
            reason,
        };
        let registry = registry_url(&config.registry).map_err(invalid)?;
        if !valid_package(&config.package) {
            return Err(invalid(format!(
                "{:?} is not an npm package name",
                config.package
            )));
        }
        if config.dist_tag.is_empty()
            || Version::parse(config.dist_tag.trim_start_matches('v')).is_ok()
            || config.dist_tag.contains(char::is_whitespace)
        {
            return Err(invalid(format!(
                "{:?} cannot be a dist-tag: npm refuses empty tags and tags that read as versions",
                config.dist_tag
            )));
        }
        Ok(Self {
            config: config.clone(),
            registry,
            credential,
            http,
            runner,
            npm: if cfg!(windows) { "npm.cmd" } else { "npm" }.into(),
            confirm_attempts: 12,
            confirm_delay: Duration::from_secs(5),
            credential_note: None,
        })
    }

    /// Looks for an upload `attempts` times, `delay` apart, before failing.
    pub fn with_confirmation(mut self, attempts: u32, delay: Duration) -> Self {
        self.confirm_attempts = attempts.max(1);
        self.confirm_delay = delay;
        self
    }

    /// The real channel: HTTPS, the credential npm would use from
    /// `project`'s `.npmrc` or the user config, and the `npm` on `PATH`.
    pub fn from_environment(
        config: &NpmChannelConfig,
        project: &Path,
    ) -> Result<Self, ChannelError> {
        let registry = registry_url(&config.registry).map_err(|reason| ChannelError::Invalid {
            channel: format!("npm {}", config.package),
            reason,
        })?;
        let env = |name: &str| std::env::var(name).ok();
        let lookup = npmrc::credential(&registry, Some(project), &env).map_err(|reason| {
            ChannelError::Unreadable {
                channel: format!("npm {} on {}", config.package, http::display(&registry)),
                reason,
            }
        })?;
        let mut channel = Self::new(
            config,
            lookup.credential,
            Box::new(UreqTransport::new()),
            Box::new(SystemRunner),
        )?;
        channel.credential_note = lookup.note;
        Ok(channel)
    }

    /// Says why a configured credential was skipped, in the messages of a
    /// channel that could not be read without it.
    pub fn with_credential_note(mut self, note: impl Into<String>) -> Self {
        self.credential_note = Some(note.into());
        self
    }

    /// Runs this program instead of `npm`.
    pub fn with_npm(mut self, program: impl Into<String>) -> Self {
        self.npm = program.into();
        self
    }

    /// Whether reads carry a credential.
    pub fn authenticated(&self) -> bool {
        self.credential.is_some()
    }

    fn unreadable(&self, reason: impl Into<String>) -> ChannelError {
        ChannelError::Unreadable {
            channel: self.describe(),
            reason: reason.into(),
        }
    }

    /// `reason`, with why a configured credential was skipped.
    fn without_credential(&self, reason: String) -> ChannelError {
        match &self.credential_note {
            Some(note) => self.unreadable(format!("{reason} ({note})")),
            None => self.unreadable(reason),
        }
    }

    fn redact(&self, text: &str) -> String {
        match &self.credential {
            Some(credential) => credential.redact(text),
            None => text.to_owned(),
        }
    }

    /// The package document, or `None` when the registry has no such
    /// package. An anonymous 404 counts as "none" only for a public
    /// package: npm answers 404 for a private package read without
    /// credentials too.
    fn packument(&self) -> Result<Option<Packument>, ChannelError> {
        let url = self
            .registry
            .join(&self.config.package.replace('/', "%2f"))
            .map_err(|error| self.unreadable(error.to_string()))?;
        let response = http::get(
            &*self.http,
            &url,
            "application/json",
            self.credential.as_ref(),
            PACKUMENT_LIMIT,
        )
        .map_err(|reason| self.unreadable(reason))?;
        match response.status {
            200 => self.parse_packument(&response.body).map(Some),
            404 if self.credential.is_some() => Ok(None),
            404 if self.config.access == Some(NpmAccess::Public) => Ok(None),
            404 => Err(self.without_credential(format!(
                "the registry answered 404 to a read without credentials, which a private package and a missing one both answer. Give npm a token for it in .npmrc (`//{}/:_authToken=${{NODE_AUTH_TOKEN}}`, as actions/setup-node writes), or declare access = \"public\"",
                self.registry.host_str().unwrap_or_default()
            ))),
            401 | 403 => Err(self.without_credential(format!(
                "the registry refused {} ({})",
                if self.credential.is_some() {
                    "the credential from .npmrc"
                } else {
                    "a read without credentials"
                },
                response.status
            ))),
            status => Err(self.unreadable(format!("the registry answered {status}"))),
        }
    }

    fn parse_packument(&self, body: &[u8]) -> Result<Packument, ChannelError> {
        let value: Value = serde_json::from_slice(body).map_err(|error| {
            self.unreadable(format!("the package document is not JSON: {error}"))
        })?;
        if let Some(name) = value.get("name").and_then(Value::as_str)
            && name != self.config.package
        {
            return Err(self.unreadable(format!("the registry answered for {name}")));
        }
        let mut packument = Packument::default();
        if let Some(versions) = value.get("versions").and_then(Value::as_object) {
            for (text, manifest) in versions {
                let Ok(version) = Version::parse(text) else {
                    continue;
                };
                let dist = manifest.get("dist");
                let field = |name: &str| {
                    dist.and_then(|dist| dist.get(name))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                };
                let tarball = field("tarball")
                    .ok_or_else(|| self.unreadable(format!("{version} has no dist.tarball")))?;
                packument.versions.push((
                    version,
                    Dist {
                        tarball,
                        integrity: field("integrity"),
                    },
                ));
            }
        }
        if let Some(tags) = value.get("dist-tags").and_then(Value::as_object) {
            packument.dist_tags = tags
                .iter()
                .filter_map(|(tag, version)| Some((tag.clone(), version.as_str()?.to_owned())))
                .collect();
        }
        Ok(packument)
    }

    fn release_ref(&self, version: &Version, dist: &Dist) -> Result<ReleaseRef, ChannelError> {
        let integrity = dist.integrity.clone().ok_or_else(|| {
            self.unreadable(format!(
                "{version} has no dist.integrity, so its bytes cannot be checked"
            ))
        })?;
        Ok(ReleaseRef {
            version: version.clone(),
            artifact: integrity,
            locator: dist.tarball.clone(),
        })
    }

    /// Checks the tested tarball is this package at this version, bound
    /// for this registry, before anything is uploaded.
    fn check_artifact(&self, request: &PublishRequest<'_>) -> Result<(), ChannelError> {
        let refuse = |reason: String| ChannelError::Artifact {
            channel: self.describe(),
            path: request.artifact.to_owned(),
            reason,
        };
        let bytes = fs::read(request.artifact).map_err(|error| refuse(error.to_string()))?;
        let files =
            read_tarball_files(&bytes, &["package.json"], PACKUMENT_LIMIT).map_err(refuse)?;
        let package: Value = files
            .get("package.json")
            .ok_or_else(|| refuse("the tarball has no package.json".into()))
            .and_then(|text| {
                serde_json::from_slice(text)
                    .map_err(|error| refuse(format!("its package.json: {error}")))
            })?;
        let field = |name: &str| package.get(name).and_then(Value::as_str);
        if field("name") != Some(self.config.package.as_str()) {
            return Err(refuse(format!(
                "it is {:?}, not {}",
                field("name").unwrap_or_default(),
                self.config.package
            )));
        }
        if field("version") != Some(request.version.to_string().as_str()) {
            return Err(refuse(format!(
                "it is version {:?}, not {}",
                field("version").unwrap_or_default(),
                request.version
            )));
        }
        if package.get("private") == Some(&Value::Bool(true)) {
            return Err(refuse("its package.json says \"private\": true".into()));
        }
        let publish_config = package.get("publishConfig");
        if let Some(registry) = publish_config
            .and_then(|config| config.get("registry"))
            .and_then(Value::as_str)
            && registry_url(registry).ok().as_ref() != Some(&self.registry)
        {
            return Err(refuse(format!(
                "its publishConfig.registry is {registry}, not this channel's"
            )));
        }
        if let (Some(access), Some(expected)) = (
            publish_config
                .and_then(|config| config.get("access"))
                .and_then(Value::as_str),
            self.config.access,
        ) && access != expected.as_str()
        {
            return Err(refuse(format!(
                "its publishConfig.access is {access}, not {}",
                expected.as_str()
            )));
        }
        Ok(())
    }

    /// The dist-tag to publish under: the configured one, unless it already
    /// points at a higher version, which a release must never move
    /// backwards; then `<tag>-<major>.<minor>`.
    fn publish_tag(&self, packument: Option<&Packument>, version: &Version) -> String {
        let tag = &self.config.dist_tag;
        match packument.and_then(|packument| packument.tag(tag)) {
            Some(current) if current > *version => {
                format!("{tag}-{}.{}", version.major, version.minor)
            }
            _ => tag.clone(),
        }
    }

    /// `npm <args>`, bound to this channel's registry whatever the
    /// environment says: a scoped package's `@scope:registry` in any
    /// `.npmrc` beats `--registry` and `publishConfig`, so the scope is
    /// pinned too; and an inherited `dry-run` would make an upload a no-op.
    fn npm_command<I, S>(&self, args: I) -> CommandLine
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut command = CommandLine::new(&self.npm)
            .args(args)
            .arg("--registry")
            .arg(self.registry.as_str());
        if let Some((scope, _)) = self.config.package.split_once('/') {
            command = command.arg(format!("--{scope}:registry={}", self.registry));
        }
        command.arg("--dry-run=false")
    }

    fn publish_command(&self, artifact: &Path, tag: &str) -> CommandLine {
        let mut command = self.npm_command([
            "publish".to_owned(),
            artifact.to_string_lossy().into_owned(),
            "--ignore-scripts".into(),
            "--tag".into(),
            tag.to_owned(),
        ]);
        if let Some(access) = self.config.access {
            command = command.args(["--access", access.as_str()]);
        }
        command
    }

    /// Waits until the registry shows what an `npm` command claimed to do.
    /// A success npm reported is not trusted alone: a dry-run setting or a
    /// redirected scope makes npm succeed without touching this registry.
    fn confirm(&self, what: &str, done: impl Fn(&Packument) -> bool) -> Result<(), ChannelError> {
        for attempt in 0..self.confirm_attempts {
            if attempt > 0 {
                std::thread::sleep(self.confirm_delay);
            }
            if self.packument()?.as_ref().is_some_and(&done) {
                return Ok(());
            }
        }
        Err(ChannelError::Publish {
            channel: self.describe(),
            reason: format!(
                "npm reported success, but the registry does not show {what}; nothing is recorded as published"
            ),
        })
    }

    fn run(&self, command: &CommandLine) -> Result<(), ChannelError> {
        let failed = |reason: String| ChannelError::Publish {
            channel: self.describe(),
            reason: self.redact(&reason),
        };
        let output = self
            .runner
            .run(command)
            .map_err(|error| failed(error.to_string()))?;
        if !output.success {
            return Err(failed(format!(
                "`{command}` failed: {}",
                output.stderr_text()
            )));
        }
        Ok(())
    }
}

impl Channel for NpmChannel {
    fn describe(&self) -> String {
        format!(
            "npm {} on {}",
            self.config.package,
            http::display(&self.registry)
        )
    }

    fn latest(&self) -> Result<Option<ReleaseRef>, ChannelError> {
        let Some(packument) = self.packument()? else {
            return Ok(None);
        };
        let Some(version) =
            highest_stable(packument.versions.iter().map(|(version, _)| version)).cloned()
        else {
            return Ok(None);
        };
        let dist = packument.dist(&version).expect("listed above");
        self.release_ref(&version, dist).map(Some)
    }

    fn read_release(&self, release: &ReleaseRef) -> Result<LastRelease, ChannelError> {
        let url = self
            .registry
            .join(&release.locator)
            .map_err(|error| self.unreadable(format!("its tarball URL: {error}")))?;
        let response = http::get(
            &*self.http,
            &url,
            "application/octet-stream",
            self.credential.as_ref(),
            TARBALL_LIMIT,
        )
        .map_err(|reason| self.unreadable(reason))?;
        if response.status != 200 {
            return Err(self.unreadable(format!(
                "the tarball of {} answered {}",
                release.version, response.status
            )));
        }
        match sri_matches(&release.artifact, &response.body) {
            Some(true) => {}
            Some(false) => {
                return Err(self.unreadable(format!(
                    "the tarball of {} does not match its dist.integrity",
                    release.version
                )));
            }
            None => {
                return Err(self.unreadable(format!(
                    "{}'s dist.integrity has no sha512 to check its tarball against",
                    release.version
                )));
            }
        }
        let files = read_tarball_files(
            &response.body,
            &["package.json", PROVENANCE_FILE, MODEL_FILE],
            TARBALL_LIMIT,
        )
        .map_err(|reason| {
            self.unreadable(format!("the tarball of {}: {reason}", release.version))
        })?;
        let package: Value = files
            .get("package.json")
            .and_then(|text| serde_json::from_slice(text).ok())
            .ok_or_else(|| {
                self.unreadable(format!(
                    "the tarball of {} has no readable package.json",
                    release.version
                ))
            })?;
        let field = |name: &str| package.get(name).and_then(Value::as_str);
        if field("name") != Some(self.config.package.as_str())
            || field("version") != Some(release.version.to_string().as_str())
        {
            return Err(self.unreadable(format!(
                "the tarball listed as {} is {} {}",
                release.version,
                field("name").unwrap_or("(no name)"),
                field("version").unwrap_or("(no version)")
            )));
        }
        let provenance = files
            .get(PROVENANCE_FILE)
            .map(|bytes| Provenance::parse(bytes))
            .transpose()
            .map_err(|reason| self.unreadable(format!("{}: {reason}", release.version)))?;
        let model = files
            .get(MODEL_FILE)
            .map(|bytes| String::from_utf8(bytes.clone()))
            .transpose()
            .map_err(|_| {
                self.unreadable(format!("{}: {MODEL_FILE} is not UTF-8", release.version))
            })?;
        Ok(LastRelease {
            version: release.version.clone(),
            artifact: release.artifact.clone(),
            provenance,
            model,
            legacy: LegacyRecord::from_package_json(&package),
        })
    }

    fn check_published(
        &self,
        version: &Version,
        artifact: &Path,
    ) -> Result<Published, ChannelError> {
        let bytes = fs::read(artifact).map_err(|error| ChannelError::Artifact {
            channel: self.describe(),
            path: artifact.to_owned(),
            reason: error.to_string(),
        })?;
        let Some(packument) = self.packument()? else {
            return Ok(Published::Absent);
        };
        let Some(dist) = packument.dist(version) else {
            return Ok(Published::Absent);
        };
        match dist
            .integrity
            .as_deref()
            .and_then(|integrity| sri_matches(integrity, &bytes))
        {
            Some(true) => Ok(Published::SameBytes),
            Some(false) => Ok(Published::DifferentBytes {
                detail: format!(
                    "the registry's dist.integrity for {version} is not the tested tarball's"
                ),
            }),
            None => Err(self.unreadable(format!(
                "{version} is published without a sha512 dist.integrity, so it cannot be compared"
            ))),
        }
    }

    fn describe_publish(&self, request: &PublishRequest<'_>) -> Result<Vec<String>, ChannelError> {
        self.check_artifact(request)?;
        let packument = self.packument()?;
        let tag = self.publish_tag(packument.as_ref(), request.version);
        let mut lines = vec![self.publish_command(request.artifact, &tag).to_string()];
        if tag != self.config.dist_tag {
            lines.push(format!(
                "(dist-tag {} already points past {}, so it stays)",
                self.config.dist_tag, request.version
            ));
        }
        Ok(lines)
    }

    fn publish(&self, request: &PublishRequest<'_>) -> Result<PublishOutcome, ChannelError> {
        self.check_artifact(request)?;
        let packument = self.packument()?;
        let tag = self.publish_tag(packument.as_ref(), request.version);
        self.run(&self.publish_command(request.artifact, &tag))?;
        let bytes = fs::read(request.artifact).map_err(|error| ChannelError::Artifact {
            channel: self.describe(),
            path: request.artifact.to_owned(),
            reason: error.to_string(),
        })?;
        self.confirm(
            &format!("{} with the tested bytes", request.version),
            |packument| {
                packument
                    .dist(request.version)
                    .and_then(|dist| dist.integrity.as_deref())
                    .and_then(|integrity| sri_matches(integrity, &bytes))
                    == Some(true)
            },
        )?;
        Ok(PublishOutcome {
            detail: format!(
                "published {}@{} with dist-tag {tag}",
                self.config.package, request.version
            ),
        })
    }

    fn settle(&self, version: &Version) -> Result<Vec<String>, ChannelError> {
        let packument = self
            .packument()?
            .ok_or_else(|| self.unreadable("the package disappeared"))?;
        let tag = &self.config.dist_tag;
        match packument.tag(tag) {
            Some(current) if current >= *version => Ok(Vec::new()),
            _ => {
                let command = self.npm_command([
                    "dist-tag".to_owned(),
                    "add".into(),
                    format!("{}@{version}", self.config.package),
                    tag.clone(),
                ]);
                self.run(&command)?;
                self.confirm(&format!("dist-tag {tag} at {version}"), |packument| {
                    packument
                        .tag(tag)
                        .is_some_and(|current| current >= *version)
                })?;
                Ok(vec![format!("pointed dist-tag {tag} at {version}")])
            }
        }
    }
}

/// A registry URL as npm compares them: HTTPS, no credentials, query or
/// fragment, with a trailing slash.
fn registry_url(text: &str) -> Result<Url, String> {
    let mut url = Url::parse(text).map_err(|error| format!("{text:?}: {error}"))?;
    if url.scheme() != "https" {
        return Err(format!("{text:?}: an npm channel is read over HTTPS only"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(format!(
            "{}: credentials come from .npmrc, never the channel URL",
            http::display(&url)
        ));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(format!("{}: has a query or fragment", http::display(&url)));
    }
    // One spelling per registry, as the SDK file's resolver compares hosts.
    if let Some(host) = crucible_codegen_common::metadata::canonical_host(text) {
        url.set_host(Some(&host))
            .map_err(|error| format!("{}: {error}", http::display(&url)))?;
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}

fn valid_package(name: &str) -> bool {
    let part = |part: &str| {
        !part.is_empty()
            && !part.starts_with(['.', '_'])
            && part.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '.' | '_' | '~')
            })
    };
    if name.len() > 214 {
        return false;
    }
    match name.strip_prefix('@') {
        Some(scoped) => scoped
            .split_once('/')
            .is_some_and(|(scope, bare)| part(scope) && part(bare)),
        None => part(name),
    }
}

#[cfg(test)]
mod tests {
    use super::{registry_url, valid_package};

    #[test]
    fn registry_urls_are_https_without_credentials() {
        assert_eq!(
            registry_url("https://npm.pkg.github.com").unwrap().as_str(),
            "https://npm.pkg.github.com/"
        );
        assert_eq!(
            registry_url("https://r.example/npm/repo").unwrap().as_str(),
            "https://r.example/npm/repo/"
        );
        assert_eq!(
            registry_url("https://NPM.pkg.github.com./").unwrap(),
            registry_url("https://npm.pkg.github.com").unwrap()
        );
        for bad in [
            "http://npm.pkg.github.com",
            "https://user:token@npm.pkg.github.com",
            "https://token@npm.pkg.github.com",
            "https://r.example/?q=1",
            "not a url",
        ] {
            assert!(registry_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn package_names() {
        for good in ["@operation-pigeon/sdk", "pigeon-sdk", "a.b_c~d"] {
            assert!(valid_package(good), "{good}");
        }
        for bad in [
            "", "@scope", "@/x", "Upper", ".hidden", "a b", "@s/../x", "a/b",
        ] {
            assert!(!valid_package(bad), "{bad}");
        }
    }
}
