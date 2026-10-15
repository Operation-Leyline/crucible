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
use crucible_ir::ir::Service;
use crucible_sdk_release::archive::{sha256_file, write_tree_archive};
use crucible_sdk_release::record::{
    ArtifactRecord, GeneratorRecord, LastSummary, PLAN_FILE, PlanRecord, RECORD_SCHEMA,
    RELEASE_FILE, ReleaseRecord, SdkPlan, SdkRelease, write_json,
};
use crucible_sdk_release::version::{Inputs, choose};
use crucible_sdk_release::{
    Bump, ChangeKind, ChannelConfig, GitChannelConfig, NpmAccess, NpmChannelConfig, Version,
};

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
    };
    let all = crucible_ir::Capabilities::new(crucible_ir::Feature::ALL.iter().copied());
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
    let service = crucible_ir::project(full, sdk.audience)
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
            // A `+dirty` build has no verified revision.
            revision: crucible_revision::verified_revision().map(str::to_owned),
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
            shown(&out)
        ));
    }
    let options = args.options(file)?;
    let full = lower(file)?;
    let mut planned = plan_all(file, &options, &full)?;
    // What would fail late fails before anything is built.
    let mut refused = Vec::new();
    for (sdk, plan) in &planned {
        if !builds(plan) {
            continue;
        }
        if sdk.options.runtime_path.is_some() {
            refused.push(format!(
                "sdk.{}: a release refuses a local runtime_path, which no consumer can resolve; use runtime = {{ git, rev, version }}",
                sdk.name
            ));
        }
        for lockfile in &sdk.lockfiles {
            if !file.join(lockfile).is_file() {
                refused.push(format!(
                    "sdk.{}.lockfiles: {lockfile} does not exist (paths are relative to {})",
                    sdk.name,
                    shown(file.root())
                ));
            }
        }
    }
    if !refused.is_empty() {
        return Err(refused.join("\n"));
    }
    // Two `out`s that alias would overwrite each other.
    file.check_outs()?;
    fs::create_dir_all(&out).map_err(|error| format!("cannot create {}: {error}", shown(&out)))?;
    let mut built: Vec<(&Sdk, Vec<File>)> = Vec::new();
    for (sdk, plan) in &mut planned {
        if !builds(plan) {
            continue;
        }
        let files = package(file, sdk, plan, &full, &out, allow_dirty)?;
        built.push((sdk, files));
    }
    let record = record(file, &planned);
    let plan_path = out.join(PLAN_FILE);
    write_json(&plan_path, &record)
        .map_err(|error| format!("cannot write {PLAN_FILE}: {error}"))?;
    let mut release = ReleaseRecord {
        schema_version: RECORD_SCHEMA,
        plan_sha256: Some(
            sha256_file(&plan_path).map_err(|error| format!("cannot hash {PLAN_FILE}: {error}"))?,
        ),
        sdks: BTreeMap::new(),
    };
    for (sdk, files) in &built {
        let plan = &record.sdks[&sdk.name];
        // Point `out` at the release, so a path dependency resolves to the
        // sources being released.
        let target = file.join(&sdk.out);
        // Again before each write: writing one `out` creates directories
        // another may alias.
        file.check_outs()?;
        crucible_codegen::write(&target, files).map_err(|error| {
            format!(
                "sdk.{}: cannot write {}: {error}{}",
                sdk.name,
                sdk.out,
                pre_crucible_hint(&error.to_string(), &sdk.out)
            )
        })?;
        only_release_files(sdk, &target)?;
        test(file, sdk, plan, files, &out)?;
        let artifact = plan.artifact.clone().expect("built above");
        release.sdks.insert(
            sdk.name.clone(),
            SdkRelease {
                version: plan.version.clone(),
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
    if release.sdks.is_empty() {
        println!(
            "nothing to release: every SDK is unchanged, so nothing was built or tested; {} lists none",
            shown(&out.join(RELEASE_FILE))
        );
    } else {
        println!(
            "built and tested {} SDK(s) in {}",
            release.sdks.len(),
            shown(&out)
        );
    }
    Ok(())
}

/// The remedy for an `out` that a pre-Crucible export left behind.
pub(crate) fn pre_crucible_hint(error: &str, out: &str) -> String {
    if error.contains("did not write") {
        format!(
            "\nhint: if {out} holds an export from before Crucible tracked its files, delete {out} once and run `crucible sdk generate`; it is generated output"
        )
    } else {
        String::new()
    }
}

/// Native build and dependency directories, and lockfiles, which a
/// consumer's build leaves in `out` and no release ships.
const NATIVE: &[&str] = &[
    "target",
    "node_modules",
    "dist",
    ".build",
    ".git",
    "Cargo.lock",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
];

/// Refuses a file in `out` the release does not own: the consumer tests
/// would build it, but the release would not ship it.
fn only_release_files(sdk: &Sdk, out: &Path) -> Result<(), String> {
    let manifest = fs::read_to_string(out.join(".crucible")).map_err(|error| {
        format!(
            "sdk.{}: cannot read {}/.crucible: {error}",
            sdk.name, sdk.out
        )
    })?;
    let owned: BTreeSet<String> = serde_json::from_str(&manifest)
        .map_err(|error| format!("sdk.{}: {}/.crucible: {error}", sdk.name, sdk.out))?;
    let mut stray = Vec::new();
    let mut pending = vec![(out.to_owned(), String::new())];
    while let Some((dir, prefix)) = pending.pop() {
        let entries =
            fs::read_dir(&dir).map_err(|error| format!("cannot read {}: {error}", shown(&dir)))?;
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = format!("{prefix}{name}");
            if prefix.is_empty() && (name == ".crucible" || NATIVE.contains(&name.as_str())) {
                continue;
            }
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            if kind.is_dir() {
                pending.push((entry.path(), format!("{relative}/")));
            } else if !owned.contains(&relative) {
                stray.push(relative);
            }
        }
    }
    if stray.is_empty() {
        return Ok(());
    }
    stray.sort();
    Err(format!(
        "sdk.{}: {} holds files the release does not ship, which its tests would build: {}; remove them",
        sdk.name,
        sdk.out,
        stray.join(", ")
    ))
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
    if plan.package.is_none() {
        // The name the generator chose, for commits and messages.
        plan.package = cargo_package_name(&files);
    }
    let dir = out.join(&sdk.name);
    let package = dir.join("package");
    crucible_codegen::write(&package, &files)
        .map_err(|error| format!("cannot write {}: {error}", shown(&package)))?;
    let artifact = match (&plan.channel, sdk.language) {
        (Some(ChannelConfig::Npm(_)), _) | (None, Language::Ts) => {
            npm_pack(&package, &dir, &mut plan.tools)?
        }
        // A git channel publishes the package tree itself.
        _ => {
            let archive = dir.join("package.tar");
            write_tree_archive(&package, &archive)
                .map_err(|error| format!("cannot archive {}: {error}", shown(&package)))?;
            archive
        }
    };
    let record = ArtifactRecord::of(out, &artifact)
        .map_err(|error| format!("cannot hash {}: {error}", shown(&artifact)))?;
    println!(
        "sdk.{}: built {} at {}",
        sdk.name,
        record.path,
        plan.version.as_deref().unwrap_or("no version")
    );
    plan.artifact = Some(record);
    Ok(files)
}

/// `npm install --ignore-scripts` with the pinned compiler, `npm run
/// build`, `npm pack --ignore-scripts`. The compiler is the package's only
/// dependency and is installed exactly, so no lockfile is written. The
/// installed `node_modules` is removed afterwards: the release directory
/// is uploaded between jobs.
fn npm_pack(
    package: &Path,
    dir: &Path,
    tools: &mut BTreeMap<String, String>,
) -> Result<PathBuf, String> {
    let npm = program("npm");
    let run = |program: &Path, args: &[&str]| -> Result<String, String> {
        let output = Command::new(program)
            .args(args)
            .current_dir(package)
            .output()
            .map_err(|error| format!("cannot run {}: {error}", shown(program)))?;
        if !output.status.success() {
            return Err(format!(
                "`{} {}` failed in {}:\n{}{}",
                shown(program),
                args.join(" "),
                shown(package),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    tools.insert("node".into(), run(&program("node"), &["--version"])?);
    tools.insert("npm".into(), run(&npm, &["--version"])?);
    run(
        &npm,
        &[
            "install",
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
            "--no-save",
            &format!("typescript@{TYPESCRIPT}"),
        ],
    )?;
    // The compiler that ran, not the one asked for.
    let installed = fs::read_to_string(package.join("node_modules/typescript/package.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|manifest| manifest["version"].as_str().map(str::to_owned))
        .ok_or("npm installed no TypeScript compiler")?;
    if installed != TYPESCRIPT {
        return Err(format!(
            "npm installed TypeScript {installed}, not the pinned {TYPESCRIPT}"
        ));
    }
    tools.insert("typescript".into(), installed);
    run(&npm, &["run", "build"])?;
    let packed = run(
        &npm,
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
    let modules = package.join("node_modules");
    if modules.exists() {
        fs::remove_dir_all(&modules)
            .map_err(|error| format!("cannot remove {}: {error}", shown(&modules)))?;
    }
    Ok(dir.join(name))
}

/// Runs the SDK's consumer tests against the release, with its lockfiles
/// reconciled for the run and restored after it.
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
    let reconciled = if lockfiles.is_empty() {
        None
    } else {
        let package = cargo_package_name(files)
            .or_else(|| sdk.options.package.clone())
            .ok_or_else(|| format!("sdk.{}: no package name to reconcile", sdk.name))?;
        let reconciled = Reconciled::apply(&lockfiles, &package, &version)
            .map_err(|error| format!("sdk.{}: {error}", sdk.name))?;
        for lockfile in &sdk.lockfiles {
            println!("sdk.{}: {lockfile} locks {version} for the tests", sdk.name);
        }
        Some(reconciled)
    };
    let result = run_tests(file, sdk, plan, out);
    // Restored whether the tests passed or not; a restore that fails is an
    // error, never a release.
    let restored = reconciled.map_or(Ok(()), Reconciled::restore);
    result?;
    restored.map_err(|error| format!("sdk.{}: {error}", sdk.name))
}

fn run_tests(file: &SdkFile, sdk: &Sdk, plan: &SdkPlan, out: &Path) -> Result<(), String> {
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
        let Some((name, args)) = argv.split_first() else {
            continue;
        };
        println!("sdk.{}: test: {}", sdk.name, argv.join(" "));
        let status = Command::new(program(name))
            .args(args)
            .current_dir(file.root())
            .status()
            .map_err(|error| format!("sdk.{}: cannot run {name}: {error}", sdk.name))?;
        if !status.success() {
            return Err(format!(
                "sdk.{}: test `{}` failed ({status}); {} is left for inspection, with no {RELEASE_FILE}",
                sdk.name,
                argv.join(" "),
                shown(out)
            ));
        }
    }
    Ok(())
}

/// The program a test or packaging command names. On Windows a bare name
/// is looked up on `PATH` with each `PATHEXT` extension, as a shell does,
/// so `npm` finds `npm.cmd` and one SDK file serves Linux CI and Windows.
pub(crate) fn program(name: &str) -> PathBuf {
    if !cfg!(windows) || name.contains(['/', '\\']) || Path::new(name).extension().is_some() {
        return PathBuf::from(name);
    }
    let extensions = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    let directories = std::env::var_os("PATH").unwrap_or_default();
    for directory in std::env::split_paths(&directories) {
        for extension in extensions
            .split(';')
            .filter(|extension| !extension.is_empty())
        {
            let candidate = directory.join(format!("{name}{}", extension.to_ascii_lowercase()));
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    PathBuf::from(name)
}

/// A path for a message, with one separator.
pub(crate) fn shown(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
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
