//! `crucible sdk verify` and `crucible sdk publish`: they read only the
//! release directory, so the publish job needs Crucible and each channel's
//! uploader, not the service checkout or a build toolchain.

use std::path::Path;

use crucible_sdk_release::publish::{Mode, PublishItem, Step, publish as publish_one};
use crucible_sdk_release::record::{PLAN_FILE, PlanRecord, RELEASE_FILE, ReleaseRecord, read_json};
use crucible_sdk_release::{Journal, Version};

fn records(release: &Path) -> Result<(PlanRecord, ReleaseRecord), String> {
    let plan: PlanRecord = read_json(&release.join(PLAN_FILE))?;
    if !release.join(RELEASE_FILE).is_file() {
        return Err(format!(
            "{} has no {RELEASE_FILE}: its build did not finish or a test failed",
            release.display()
        ));
    }
    let record: ReleaseRecord = read_json(&release.join(RELEASE_FILE))?;
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

/// `crucible sdk verify`: re-hashes every artifact against `release.json`.
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
    println!("verified {} artifact(s)", record.sdks.len());
    Ok(())
}

/// `crucible sdk publish`: for each released SDK, checks the channel,
/// uploads (with `execute`) and journals the upload.
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
    for (name, built) in &record.sdks {
        let planned = &plan.sdks[name];
        if !names.is_empty() && !names.contains(name) {
            continue;
        }
        let (Some(config), true) = (&planned.channel, planned.releases()) else {
            println!("sdk.{name}: no channel; built and tested only");
            continue;
        };
        let version = Version::parse(&built.version)
            .map_err(|error| format!("sdk.{name}: {:?}: {error}", built.version))?;
        let channel = crucible_sdk_release::open(config, &project)
            .map_err(|error| format!("sdk.{name}: {error}"))?;
        let item = PublishItem {
            sdk: name.clone(),
            version,
            baseline: planned
                .decision
                .as_ref()
                .and_then(|decision| decision.baseline.clone()),
            artifact: built
                .artifact
                .locate(&release)
                .map_err(|error| error.to_string())?,
            artifact_sha256: Some(built.artifact.sha256.clone()),
            source_commit: planned.source_commit.clone(),
            package: planned.package.clone(),
        };
        let step = publish_one(&*channel, &item, &release, &mut journal, mode)
            .map_err(|error| error.to_string())?;
        match step {
            Step::Journaled(upload) => println!(
                "sdk.{name} {}: already done ({})",
                upload.version, upload.detail
            ),
            Step::AlreadyPublished { notes } => {
                println!(
                    "sdk.{name} {}: already on {} with the same bytes",
                    item.version,
                    channel.describe()
                );
                for note in notes {
                    println!("  {note}");
                }
            }
            Step::Published(outcome) => {
                released += 1;
                println!("sdk.{name} {}: {}", item.version, outcome.detail);
            }
            Step::WouldPublish(lines) => {
                println!("sdk.{name} {} to {}:", item.version, channel.describe());
                for line in lines {
                    println!("  {line}");
                }
            }
        }
    }
    if execute {
        println!("published {released} SDK(s)");
    } else {
        println!("dry run: nothing was published; --execute publishes");
    }
    Ok(())
}
