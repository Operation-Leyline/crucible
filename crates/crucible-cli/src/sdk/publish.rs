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
        if planned.artifact.as_ref() != Some(&built.artifact) || planned.version != built.version {
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
    let (plan, record) = records(release)?;
    let label = |name: &String| label(name, plan.sdks.get(name));
    let problems: Vec<String> = record
        .sdks
        .iter()
        .filter_map(|(name, built)| {
            built
                .artifact
                .verify(release)
                .err()
                .map(|error| format!("{}: {error}", label(name)))
        })
        .collect();
    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }
    for (name, built) in &record.sdks {
        println!(
            "{} {}: {} ({} bytes, sha256 {})",
            label(name),
            built.version.as_deref().unwrap_or("unversioned"),
            built.artifact.path,
            built.artifact.size,
            built.artifact.sha256
        );
    }
    println!("verified {PLAN_FILE} and {} artifact(s)", record.sdks.len());
    Ok(())
}

/// `language` of the `[docs]` release in `plan.json`: the API reference.
const DOCS_LANGUAGE: &str = "docs";

/// The order a release is published in: each SDK by name, then the
/// reference, whose install steps name the SDK versions before it.
fn publish_order<'a>(plan: &PlanRecord, record: &'a ReleaseRecord) -> Vec<&'a String> {
    let is_docs = |name: &&String| {
        plan.sdks
            .get(*name)
            .is_some_and(|plan| plan.language == DOCS_LANGUAGE)
    };
    let (docs, sdks): (Vec<_>, Vec<_>) = record.sdks.keys().partition(is_docs);
    sdks.into_iter().chain(docs).collect()
}

/// How messages name a released unit: `sdk.<name>`, or `docs` for the
/// reference.
fn label(name: &str, plan: Option<&SdkPlan>) -> String {
    if plan.is_some_and(|plan| plan.language == DOCS_LANGUAGE) {
        name.to_owned()
    } else {
        format!("sdk.{name}")
    }
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
    // The reference (`[docs]`) goes last: its install steps name the SDK
    // versions this release publishes, so it must not be on its channel
    // before they are.
    let is_docs = |name: &String| plan.sdks[name].language == DOCS_LANGUAGE;
    for name in publish_order(&plan, &record) {
        let built = &record.sdks[name];
        if !names.is_empty() && !names.contains(name) {
            continue;
        }
        let planned = &plan.sdks[name];
        if is_docs(name) && !errors.is_empty() {
            let error = format!(
                "{name}: not published: it describes the SDK releases above, and {} failed; publish again once they are on their channels",
                errors.len()
            );
            eprintln!("error: {error}");
            errors.push(error);
            continue;
        }
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
    // What messages call it; the journal and records keep its name.
    let unit = label(name, Some(planned));
    let (Some(config), true) = (&planned.channel, planned.releases()) else {
        println!("{unit}: no channel; built and tested only");
        return Ok(false);
    };
    let text = built
        .version
        .as_deref()
        .ok_or_else(|| format!("{unit}: a released SDK has no version"))?;
    let version = Version::parse(text).map_err(|error| format!("{unit}: {text:?}: {error}"))?;
    let channel =
        crucible_sdk_release::open(config, project).map_err(|error| format!("{unit}: {error}"))?;
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
            .map_err(|error| format!("{unit}: {error}"))?,
        artifact_sha256: Some(built.artifact.sha256.clone()),
        source_commit: planned.source_commit.clone(),
        package: planned.package.clone(),
    };
    let step =
        publish_one(&*channel, &item, release, journal, mode).map_err(|error| match error {
            // The other errors name their SDK already.
            PublishError::Channel(error) => format!("{unit}: {error}"),
            other => other.to_string(),
        })?;
    Ok(match step {
        Step::Journaled(upload) => {
            println!(
                "{unit} {}: already done ({})",
                upload.version, upload.detail
            );
            false
        }
        Step::AlreadyPublished { notes } => {
            println!(
                "{unit} {}: already on {} with the same bytes",
                item.version,
                channel.describe()
            );
            for note in notes {
                println!("  {note}");
            }
            false
        }
        Step::Published(outcome) => {
            println!("{unit} {}: {}", item.version, outcome.detail);
            true
        }
        Step::WouldPublish(lines) => {
            println!("{unit} {} to {}:", item.version, channel.describe());
            for line in lines {
                println!("  {line}");
            }
            false
        }
    })
}

#[cfg(test)]
mod tests {
    use crucible_sdk_release::record::{PlanRecord, ReleaseRecord};
    use serde_json::json;

    use super::publish_order;

    /// The reference names the SDK versions a release publishes, so it is
    /// uploaded after every SDK, wherever its name sorts.
    #[test]
    fn the_reference_is_published_after_every_sdk() {
        let plan = |language: &str| json!({"language": language, "config": {}, "configSha256": ""});
        let built = json!({"channel": true, "reasons": [], "configSha256": "",
            "artifact": {"path": "x", "sha256": "", "size": 0}});
        let plan: PlanRecord = serde_json::from_value(json!({
            "schemaVersion": 1, "sdkFile": "crucible-sdk.toml",
            "generator": {"version": "0", "changes": 0},
            "sdks": {"docs": plan("docs"), "a": plan("ts"), "rust": plan("rust")},
        }))
        .unwrap();
        let record: ReleaseRecord = serde_json::from_value(json!({
            "schemaVersion": 1,
            "sdks": {"docs": built, "a": built, "rust": built},
        }))
        .unwrap();
        assert_eq!(publish_order(&plan, &record), ["a", "rust", "docs"]);
    }
}
