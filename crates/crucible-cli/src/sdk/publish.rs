//! `crucible sdk verify` and `crucible sdk publish`: they read only the
//! release directory, so the publish job needs Crucible and each channel's
//! uploader, not the service checkout or a build toolchain.

use std::path::Path;

use crucible_sdk_release::archive::sha256_bytes;
use crucible_sdk_release::publish::{
    Mode, PublishError, PublishItem, Step, publish as publish_one,
};
use crucible_sdk_release::record::{
    PLAN_FILE, PlanRecord, RELEASE_FILE, ReleaseRecord, SdkPlan, SdkRelease, parse_json, read_json,
};
use crucible_sdk_release::{Journal, Version};

use super::release::shown;

/// The release directory's records, checked against each other:
/// `release.json` holds `plan.json`'s digest, since `publish` takes each
/// channel and baseline from the plan.
fn records(release: &Path) -> Result<(PlanRecord, ReleaseRecord), String> {
    if !release.join(RELEASE_FILE).is_file() {
        return Err(format!(
            "{} has no {RELEASE_FILE}: its build did not finish or a test failed",
            shown(release)
        ));
    }
    let record: ReleaseRecord = read_json(&release.join(RELEASE_FILE))?;
    let plan_path = release.join(PLAN_FILE);
    // Hashed and parsed from the same bytes, so what is checked is what
    // is used.
    let plan_bytes = std::fs::read(&plan_path)
        .map_err(|error| format!("cannot read {}: {error}", shown(&plan_path)))?;
    let digest = sha256_bytes(&plan_bytes);
    match &record.plan_sha256 {
        Some(expected) if *expected == digest => {}
        Some(_) => {
            return Err(format!(
                "{PLAN_FILE} changed after {RELEASE_FILE} recorded it; it names each channel and baseline, so the release cannot be trusted"
            ));
        }
        None => {
            return Err(format!(
                "{RELEASE_FILE} records no digest of {PLAN_FILE}; rebuild the release with this Crucible"
            ));
        }
    }
    let plan: PlanRecord = parse_json(&plan_bytes, &plan_path)?;
    for (name, built) in &record.sdks {
        let planned = plan.sdks.get(name).ok_or_else(|| {
            format!("{RELEASE_FILE} lists sdk.{name}, which {PLAN_FILE} does not")
        })?;
        if planned.artifact.as_ref() != Some(&built.artifact)
            || planned.version.as_deref() != Some(built.version.as_str())
        {
            return Err(format!(
                "{PLAN_FILE} and {RELEASE_FILE} disagree about sdk.{name}"
            ));
        }
    }
    Ok((plan, record))
}

/// `crucible sdk verify`: checks `plan.json` and re-hashes every artifact
/// against `release.json`.
pub(crate) fn verify(release: &Path) -> Result<(), String> {
    let (_, record) = records(release)?;
    let problems: Vec<String> = record
        .sdks
        .iter()
        .filter_map(|(name, built)| {
            built
                .artifact
                .verify(release)
                .err()
                .map(|error| format!("sdk.{name}: {error}"))
        })
        .collect();
    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }
    for (name, built) in &record.sdks {
        println!(
            "sdk.{name} {}: {} ({} bytes, sha256 {})",
            built.version, built.artifact.path, built.artifact.size, built.artifact.sha256
        );
    }
    println!("verified {PLAN_FILE} and {} artifact(s)", record.sdks.len());
    Ok(())
}

/// `crucible sdk publish`: for each released SDK, checks the channel,
/// uploads (with `execute`) and journals the upload. Every SDK is
/// attempted; the errors are reported together.
pub(crate) fn publish(release: &Path, names: &[String], execute: bool) -> Result<(), String> {
    let release = std::path::absolute(release).map_err(|error| error.to_string())?;
    let (plan, record) = records(&release)?;
    if let Some(unknown) = names.iter().find(|name| !record.sdks.contains_key(*name)) {
        return Err(format!(
            "{RELEASE_FILE} releases no sdk.{unknown}; it has {}",
            record.sdks.keys().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    let project = std::env::current_dir().map_err(|error| error.to_string())?;
    let mode = if execute { Mode::Execute } else { Mode::DryRun };
    let mut journal = Journal::load(&release).map_err(|error| error.to_string())?;
    let mut released = 0;
    let mut errors = Vec::new();
    for (name, built) in &record.sdks {
        if !names.is_empty() && !names.contains(name) {
            continue;
        }
        let planned = &plan.sdks[name];
        match publish_sdk(name, planned, built, &release, &project, &mut journal, mode) {
            Ok(true) => released += 1,
            Ok(false) => {}
            Err(error) => {
                eprintln!("error: {error}");
                errors.push(error);
            }
        }
    }
    if execute {
        println!("published {released} SDK(s)");
    } else {
        println!("dry run: nothing was published; --execute publishes");
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} SDK(s) failed to publish:\n{}",
            errors.len(),
            errors.join("\n")
        ))
    }
}

/// Publishes one SDK; `Ok(true)` when this run uploaded it.
fn publish_sdk(
    name: &str,
    planned: &SdkPlan,
    built: &SdkRelease,
    release: &Path,
    project: &Path,
    journal: &mut Journal,
    mode: Mode,
) -> Result<bool, String> {
    let (Some(config), true) = (&planned.channel, planned.releases()) else {
        println!("sdk.{name}: no channel; built and tested only");
        return Ok(false);
    };
    let version = Version::parse(&built.version)
        .map_err(|error| format!("sdk.{name}: {:?}: {error}", built.version))?;
    let channel = crucible_sdk_release::open(config, project)
        .map_err(|error| format!("sdk.{name}: {error}"))?;
    let item = PublishItem {
        sdk: name.to_owned(),
        version,
        baseline: planned
            .decision
            .as_ref()
            .and_then(|decision| decision.baseline.clone()),
        artifact: built
            .artifact
            .locate(release)
            .map_err(|error| format!("sdk.{name}: {error}"))?,
        artifact_sha256: Some(built.artifact.sha256.clone()),
        source_commit: planned.source_commit.clone(),
        package: planned.package.clone(),
    };
    let step =
        publish_one(&*channel, &item, release, journal, mode).map_err(|error| match error {
            // The other errors name their SDK already.
            PublishError::Channel(error) => format!("sdk.{name}: {error}"),
            other => other.to_string(),
        })?;
    Ok(match step {
        Step::Journaled(upload) => {
            println!(
                "sdk.{name} {}: already done ({})",
                upload.version, upload.detail
            );
            false
        }
        Step::AlreadyPublished { notes } => {
            println!(
                "sdk.{name} {}: already on {} with the same bytes",
                item.version,
                channel.describe()
            );
            for note in notes {
                println!("  {note}");
            }
            false
        }
        Step::Published(outcome) => {
            println!("sdk.{name} {}: {}", item.version, outcome.detail);
            true
        }
        Step::WouldPublish(lines) => {
            println!("sdk.{name} {} to {}:", item.version, channel.describe());
            for line in lines {
                println!("  {line}");
            }
            false
        }
    })
}
