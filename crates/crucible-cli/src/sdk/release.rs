//! `crucible sdk plan` and `crucible sdk build` (docs/sdk-packaging.md,
//! sections 3 and 5): read each SDK's channel, choose its version, and
//! build, test and record what the plan chose.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Args;
use crucible_codegen::File;
use crucible_codegen::provenance::{Source, Stamp, files_digest, generator_changes};
use crucible_codegen::sdk::{self, Channel, Language, Sdk, SdkFile};
use crucible_sdk_release::archive::write_tree_archive;
use crucible_sdk_release::record::{
    ArtifactRecord, GeneratorRecord, LastSummary, PLAN_FILE, PlanRecord, RECORD_SCHEMA,
    RELEASE_FILE, ReleaseRecord, SdkPlan, SdkRelease, write_json,
};
use crucible_sdk_release::version::{Inputs, choose};
use crucible_sdk_release::{
    Bump, ChangeKind, ChannelConfig, GitChannelConfig, NpmAccess, NpmChannelConfig, Version,
};
use crucible_smithy::ir::Service;

use super::lockfile::Reconciled;

/// The TypeScript compiler a release build pins, whatever range the
/// package's `devDependencies` allow: `tsc` output varies by version.
pub(crate) const TYPESCRIPT: &str = "5.9.3";

/// What `plan` and `build` share.
#[derive(Args, Clone, Debug, Default)]
pub(crate) struct PlanArgs {
    /// Only this SDK; repeat for more. Defaults to every declared SDK.
    #[arg(long = "sdk", value_name = "NAME")]
    pub(crate) sdks: Vec<String>,
    /// Release this SDK for the first time, at its floor or 0.1.0: its
    /// channel lists no release. Needed because an empty channel looks the
    /// same as a private one read without credentials.
    #[arg(long = "first-release", value_name = "NAME")]
    pub(crate) first_release: Vec<String>,
    /// Raise this SDK's version at least this much: patch, minor or major.
    #[arg(long = "bump", value_name = "NAME=KIND")]
    pub(crate) bump: Vec<String>,
}

struct Options {
    sdks: Vec<String>,
    first_release: BTreeSet<String>,
    bump: BTreeMap<String, ChangeKind>,
}

impl PlanArgs {
    fn options(&self, file: &SdkFile) -> Result<Options, String> {
        let mut errors = Vec::new();
        let mut bump = BTreeMap::new();
        for text in &self.bump {
            match text.parse::<Bump>() {
                Ok(parsed) => {
                    bump.insert(parsed.sdk, parsed.kind);
                }
                Err(error) => errors.push(error),
            }
        }
        let first_release: BTreeSet<String> = self.first_release.iter().cloned().collect();
        for (flag, name) in first_release
            .iter()
            .map(|name| ("--first-release", name))
            .chain(bump.keys().map(|name| ("--bump", name)))
        {
            match file.sdks.get(name) {
                None => errors.push(format!(
                    "{flag} {name}: {} declares no SDK {name:?}",
                    file.path.display()
                )),
                Some(sdk) if sdk.channel.is_none() => errors.push(format!(
                    "{flag} {name}: sdk.{name} has no channel, so it is not versioned"
                )),
                Some(_) if !self.sdks.is_empty() && !self.sdks.contains(name) => {
                    errors.push(format!(
                        "{flag} {name}: sdk.{name} is not selected by --sdk"
                    ));
                }
                Some(_) => {}
            }
        }
        if errors.is_empty() {
            Ok(Options {
                sdks: self.sdks.clone(),
                first_release,
                bump,
            })
        } else {
            Err(errors.join("\n"))
        }
    }
}

/// The model, lowered once with every capability; each SDK projects its
/// audience from it.
fn lower(file: &SdkFile) -> Result<Service, String> {
    let source = crate::Source {
        model: file.join(&file.model),
        service: file
            .service
            .as_ref()
            .map(|id| id.parse().expect("the resolver checked the service ID")),
        protocol: None,
    };
    let all = crucible_smithy::Capabilities::new(crucible_smithy::Feature::ALL.iter().copied());
    crate::lower_with_capabilities(&source, &all)
}

/// The channel, as `crucible-sdk-release` opens it.
fn channel_config(sdk: &Sdk) -> Result<Option<ChannelConfig>, String> {
    Ok(match &sdk.channel {
        None => None,
        Some(Channel::Npm {
            url,
            access,
            dist_tag,
        }) => Some(ChannelConfig::Npm(NpmChannelConfig {
            registry: url.clone(),
            package: sdk
                .options
                .package
                .clone()
                .ok_or_else(|| format!("sdk.{}: an npm channel needs `package`", sdk.name))?,
            access: access
                .as_deref()
                .map(|access| {
                    NpmAccess::parse(access)
                        .ok_or_else(|| format!("sdk.{}: access {access:?}", sdk.name))
                })
                .transpose()?,
            dist_tag: dist_tag.clone(),
        })),
        Some(Channel::Git {
            url,
            branch,
            path,
            tag,
            adopt,
        }) => Some(ChannelConfig::Git(GitChannelConfig {
            url: url.clone(),
            branch: branch.clone(),
            path: path.clone(),
            tag: tag.clone(),
            adopt: adopt.clone(),
        })),
    })
}

/// The SDK's package files at `version`, stamped.
fn emit(
    file: &SdkFile,
    sdk: &Sdk,
    full: &Service,
    version: Option<&str>,
    source: Option<Source>,
) -> Result<Vec<File>, String> {
    let client = sdk.client(file, version)?;
    let service = crucible_smithy::project(full, sdk.audience)
        .map_err(|diagnostics| format!("sdk.{}:\n{diagnostics}", sdk.name))?;
    let stamp = Stamp {
        config_sha256: Some(sdk.config_sha256(file)),
        source,
    };
    client
        .emit_with_provenance(&service, sdk.audience, &stamp)
        .map_err(|diagnostics| format!("cannot generate sdk.{}:\n{diagnostics}", sdk.name))
}

fn plan_one(
    file: &SdkFile,
    sdk: &Sdk,
    options: &Options,
    full: &Service,
    cru: bool,
) -> Result<SdkPlan, String> {
    let name = &sdk.name;
    let floor = sdk
        .version
        .as_deref()
        .map(|floor| {
            Version::parse(floor)
                .map_err(|error| format!("sdk.{name}.version: {floor:?} is not x.y.z: {error}"))
        })
        .transpose()?;
    let config = channel_config(sdk)?;
    let mut plan = SdkPlan {
        language: sdk.language.name().to_owned(),
        package: sdk.options.package.clone(),
        config: sdk.effective(file),
        config_sha256: sdk.config_sha256(file),
        channel: config.clone(),
        channel_description: None,
        last: None,
        compared_digest: None,
        decision: None,
        version: None,
        source_commit: None,
        content_digest: None,
        tools: BTreeMap::new(),
        artifact: None,
    };
    let Some(config) = config else {
        // Built and tested, never versioned against anything.
        plan.version = sdk.version.clone().or_else(|| sdk.development_version());
        return Ok(plan);
    };
    let channel = crucible_sdk_release::open(&config, file.root())
        .map_err(|error| format!("sdk.{name}: {error}"))?;
    plan.channel_description = Some(channel.describe());
    let read = channel
        .read_last()
        .map_err(|error| format!("sdk.{name}: {error}"))?;
    // "Unchanged": the current inputs generated at the last release's
    // version digest the same as that release.
    let current = match read.last() {
        Some(last) => Some(
            files_digest(&emit(
                file,
                sdk,
                full,
                Some(&last.version.to_string()),
                None,
            )?)
            .map_err(|error| format!("sdk.{name}: {error}"))?,
        ),
        None => None,
    };
    let service_version = full.version.clone();
    let decision = choose(&Inputs {
        sdk: name,
        read: &read,
        current_digest: current.as_deref(),
        floor: floor.as_ref(),
        change: ChangeKind::Patch,
        bump: options.bump.get(name).copied(),
        first_release: options.first_release.contains(name),
        cru_service_version: cru.then_some(service_version.as_str()),
    })
    .map_err(|error| error.to_string())?;
    plan.last = LastSummary::of(&read);
    plan.compared_digest = current;
    plan.version = decision.version.as_ref().map(ToString::to_string);
    plan.decision = Some(decision);
    Ok(plan)
}

/// Plans every selected SDK. Every SDK's error is reported in one run.
fn plan_all<'a>(
    file: &'a SdkFile,
    options: &Options,
    full: &Service,
) -> Result<Vec<(&'a Sdk, SdkPlan)>, String> {
    let cru = crate::is_cru_package(&file.join(&file.model));
    let mut planned = Vec::new();
    let mut errors = Vec::new();
    for sdk in file.select(&options.sdks)? {
        match plan_one(file, sdk, options, full, cru) {
            Ok(plan) => planned.push((sdk, plan)),
            Err(error) => errors.push(error),
        }
    }
    if errors.is_empty() {
        Ok(planned)
    } else {
        Err(errors.join("\n"))
    }
}

fn record(file: &SdkFile, planned: &[(&Sdk, SdkPlan)]) -> PlanRecord {
    PlanRecord {
        schema_version: RECORD_SCHEMA,
        sdk_file: file.path.file_name().map_or_else(
            || sdk::FILE_NAME.into(),
            |name| name.to_string_lossy().into_owned(),
        ),
        generator: GeneratorRecord {
            version: env!("CARGO_PKG_VERSION").into(),
            revision: crucible_codegen_rust::source_revision().map(str::to_owned),
            changes: generator_changes(),
        },
        sdks: planned
            .iter()
            .map(|(sdk, plan)| (sdk.name.clone(), plan.clone()))
            .collect(),
    }
}

/// Whether `build` builds this SDK: it has no channel, or the plan
/// releases it.
fn builds(plan: &SdkPlan) -> bool {
    plan.channel.is_none() || plan.version.is_some()
}

/// `crucible sdk plan`: prints each SDK's last version, change, next
/// version and why. Writes nothing.
pub(crate) fn plan(file: &SdkFile, args: &PlanArgs, json: bool) -> Result<(), String> {
    let options = args.options(file)?;
    let full = lower(file)?;
    let planned = plan_all(file, &options, &full)?;
    let record = record(file, &planned);
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&record).expect("JSON serializes")
        );
    } else {
        print_plan(&record);
    }
    warn(&record);
    Ok(())
}

fn warn(record: &PlanRecord) {
    for plan in record.sdks.values() {
        for warning in plan.decision.iter().flat_map(|decision| &decision.warnings) {
            eprintln!("warning: {warning}");
        }
    }
}

fn print_plan(record: &PlanRecord) {
    for (name, plan) in &record.sdks {
        let Some(channel) = &plan.channel_description else {
            println!(
                "sdk.{name}: no channel; built at {}, not released",
                plan.version.as_deref().unwrap_or("no version")
            );
            continue;
        };
        println!("sdk.{name}  {channel}");
        match &plan.last {
            Some(last) => println!(
                "  last  {} ({})",
                last.version,
                last.artifact.chars().take(19).collect::<String>()
            ),
            None => println!("  last  none"),
        }
        let Some(decision) = &plan.decision else {
            continue;
        };
        match &decision.version {
            None => println!(
                "  next  unchanged since {}: no release",
                plan.last
                    .as_ref()
                    .map_or_else(String::new, |last| last.version.to_string())
            ),
            Some(version) => {
                println!("  next  {version}");
                for reason in &decision.reasons {
                    println!("        {reason}");
                }
            }
        }
    }
}

/// `crucible sdk build`: plans, then packages each SDK the plan releases
/// (and each SDK without a channel), points its `out` at the release,
/// reconciles lockfiles, runs its consumer tests, and records the release.
pub(crate) fn build(
    file: &SdkFile,
    args: &PlanArgs,
    out: &Path,
    allow_dirty: bool,
) -> Result<(), String> {
    let out = std::path::absolute(out).map_err(|error| error.to_string())?;
    if fs::read_dir(&out).is_ok_and(|mut entries| entries.next().is_some()) {
        return Err(format!(
            "{} is not empty; a release builds in a fresh directory",
            out.display()
        ));
    }
    let options = args.options(file)?;
    let full = lower(file)?;
    let mut planned = plan_all(file, &options, &full)?;
    let refused: Vec<String> = planned
        .iter()
        .filter(|(sdk, plan)| builds(plan) && sdk.options.runtime_path.is_some())
        .map(|(sdk, _)| {
            format!(
                "sdk.{}: a release refuses a local runtime_path, which no consumer can resolve; use runtime = {{ git, rev, version }}",
                sdk.name
            )
        })
        .collect();
    if !refused.is_empty() {
        return Err(refused.join("\n"));
    }
    fs::create_dir_all(&out)
        .map_err(|error| format!("cannot create {}: {error}", out.display()))?;
    let mut built: Vec<(&Sdk, Vec<File>)> = Vec::new();
    for (sdk, plan) in &mut planned {
        if !builds(plan) {
            continue;
        }
        let files = package(file, sdk, plan, &full, &out, allow_dirty)?;
        built.push((sdk, files));
    }
    let record = record(file, &planned);
    write_json(&out.join(PLAN_FILE), &record)
        .map_err(|error| format!("cannot write {PLAN_FILE}: {error}"))?;
    let mut release = ReleaseRecord {
        schema_version: RECORD_SCHEMA,
        sdks: BTreeMap::new(),
    };
    for (sdk, files) in &built {
        let plan = &record.sdks[&sdk.name];
        // Point `out` at the release, so a path dependency resolves to the
        // sources being released.
        let target = file.join(&sdk.out);
        crucible_codegen::write(&target, files)
            .map_err(|error| format!("cannot write {}: {error}", target.display()))?;
        test(file, sdk, plan, files, &out)?;
        let artifact = plan.artifact.clone().expect("built above");
        release.sdks.insert(
            sdk.name.clone(),
            SdkRelease {
                version: plan.version.clone().unwrap_or_default(),
                channel: plan.channel.is_some(),
                reasons: plan
                    .decision
                    .iter()
                    .flat_map(|decision| &decision.reasons)
                    .map(ToString::to_string)
                    .collect(),
                config_sha256: plan.config_sha256.clone(),
                content_digest: plan.content_digest.clone(),
                artifact,
            },
        );
    }
    write_json(&out.join(RELEASE_FILE), &release)
        .map_err(|error| format!("cannot write {RELEASE_FILE}: {error}"))?;
    print_plan(&record);
    warn(&record);
    println!(
        "built and tested {} SDK(s) in {}",
        release.sdks.len(),
        out.display()
    );
    Ok(())
}

/// Generates the SDK at its version into `<out>/<name>/package` with its
/// source stamped, and packages it. Returns the files, which `build` also
/// writes to the SDK's `out`.
fn package(
    file: &SdkFile,
    sdk: &Sdk,
    plan: &mut SdkPlan,
    full: &Service,
    out: &Path,
    allow_dirty: bool,
) -> Result<Vec<File>, String> {
    let source = sdk::source(file, sdk, allow_dirty)?;
    plan.source_commit = Some(source.commit.clone());
    let files = emit(file, sdk, full, plan.version.as_deref(), Some(source))?;
    plan.content_digest =
        Some(files_digest(&files).map_err(|error| format!("sdk.{}: {error}", sdk.name))?);
    let dir = out.join(&sdk.name);
    let package = dir.join("package");
    crucible_codegen::write(&package, &files)
        .map_err(|error| format!("cannot write {}: {error}", package.display()))?;
    let artifact = match (&plan.channel, sdk.language) {
        (Some(ChannelConfig::Npm(_)), _) | (None, Language::Ts) => {
            npm_pack(&package, &dir, &mut plan.tools)?
        }
        // A git channel publishes the package tree itself.
        _ => {
            let archive = dir.join("package.tar");
            write_tree_archive(&package, &archive)
                .map_err(|error| format!("cannot archive {}: {error}", package.display()))?;
            archive
        }
    };
    plan.artifact = Some(
        ArtifactRecord::of(out, &artifact)
            .map_err(|error| format!("cannot hash {}: {error}", artifact.display()))?,
    );
    println!(
        "sdk.{}: built {} at {}",
        sdk.name,
        artifact.display(),
        plan.version.as_deref().unwrap_or("no version")
    );
    Ok(files)
}

/// `npm install --ignore-scripts` with the pinned compiler, `npm run
/// build`, `npm pack --ignore-scripts`. Any lockfile the install creates
/// is kept beside the artifact.
fn npm_pack(
    package: &Path,
    dir: &Path,
    tools: &mut BTreeMap<String, String>,
) -> Result<PathBuf, String> {
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let run = |program: &str, args: &[&str]| -> Result<String, String> {
        let output = Command::new(program)
            .args(args)
            .current_dir(package)
            .output()
            .map_err(|error| format!("cannot run {program}: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "`{program} {}` failed in {}:\n{}{}",
                args.join(" "),
                package.display(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    let node = if cfg!(windows) { "node.exe" } else { "node" };
    tools.insert("node".into(), run(node, &["--version"])?);
    tools.insert("npm".into(), run(npm, &["--version"])?);
    tools.insert("typescript".into(), TYPESCRIPT.into());
    run(
        npm,
        &[
            "install",
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
            "--no-save",
            &format!("typescript@{TYPESCRIPT}"),
        ],
    )?;
    run(npm, &["run", "build"])?;
    let packed = run(
        npm,
        &[
            "pack",
            "--ignore-scripts",
            "--json",
            "--pack-destination",
            &dir.to_string_lossy(),
        ],
    )?;
    let listing: serde_json::Value = serde_json::from_str(&packed)
        .map_err(|error| format!("npm pack printed no JSON: {error}"))?;
    let name = listing[0]["filename"]
        .as_str()
        .ok_or("npm pack named no file")?
        // npm 7-8 printed the scoped name with its `@` and `/`.
        .trim_start_matches('@')
        .replace('/', "-");
    let lock = package.join("package-lock.json");
    if lock.is_file() {
        fs::copy(&lock, dir.join("package-lock.json"))
            .map_err(|error| format!("cannot keep {}: {error}", lock.display()))?;
    }
    Ok(dir.join(name))
}

/// Runs the SDK's consumer tests against the release, with its lockfiles
/// reconciled for the run.
fn test(
    file: &SdkFile,
    sdk: &Sdk,
    plan: &SdkPlan,
    files: &[File],
    out: &Path,
) -> Result<(), String> {
    if sdk.test.is_empty() {
        return Ok(());
    }
    let version = plan.version.clone().unwrap_or_default();
    let lockfiles: Vec<PathBuf> = sdk.lockfiles.iter().map(|path| file.join(path)).collect();
    // Restored when dropped, whether the tests pass or not.
    let reconciled = if lockfiles.is_empty() {
        None
    } else {
        let package = cargo_package_name(files)
            .or_else(|| sdk.options.package.clone())
            .ok_or_else(|| format!("sdk.{}: no package name to reconcile", sdk.name))?;
        Some(Reconciled::apply(&lockfiles, &package, &version)?)
    };
    for path in reconciled.iter().flat_map(Reconciled::paths) {
        println!(
            "sdk.{}: {} locks {version} for the tests",
            sdk.name,
            path.display()
        );
    }
    let artifact = plan
        .artifact
        .as_ref()
        .expect("built")
        .locate(out)
        .map_err(|error| error.to_string())?;
    let package = out.join(&sdk.name).join("package");
    for command in &sdk.test {
        let argv: Vec<String> = command
            .iter()
            .map(|arg| {
                arg.replace("{artifact}", &artifact.to_string_lossy())
                    .replace("{package}", &package.to_string_lossy())
            })
            .collect();
        let Some((program, args)) = argv.split_first() else {
            continue;
        };
        println!("sdk.{}: test: {}", sdk.name, argv.join(" "));
        let status = Command::new(program)
            .args(args)
            .current_dir(file.root())
            .status()
            .map_err(|error| format!("sdk.{}: cannot run {program}: {error}", sdk.name))?;
        if !status.success() {
            return Err(format!(
                "sdk.{}: test `{}` failed ({status}); {} is left for inspection, with no {RELEASE_FILE}",
                sdk.name,
                argv.join(" "),
                out.display()
            ));
        }
    }
    Ok(())
}

/// The `[package] name` of a generated `Cargo.toml`.
fn cargo_package_name(files: &[File]) -> Option<String> {
    let manifest = files.iter().find(|file| file.path == "Cargo.toml")?;
    let table: toml::Table = manifest.contents.parse().ok()?;
    table
        .get("package")?
        .get("name")?
        .as_str()
        .map(str::to_owned)
}
