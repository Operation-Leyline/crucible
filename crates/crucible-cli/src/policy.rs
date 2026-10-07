use std::path::Path;

use crucible_policy::{Config, Report};
use crucible_smithy::ir::Service;
use crucible_smithy::{Capabilities, Feature, Model, ShapeId};

pub(crate) fn check(service: &Service, path: &Path) -> Result<(), String> {
    emit(crucible_policy::check(service, &Config::load(path)?), false)
}

pub(crate) fn diff(
    old: &Path,
    new: &Path,
    service: Option<&ShapeId>,
    config: Option<&Path>,
    json: bool,
) -> Result<(), String> {
    let config = config.map(Config::load).transpose()?.unwrap_or_default();
    let old = load(old, service)?;
    let new = load(new, service)?;
    emit(crucible_policy::diff(&old, &new, &config), json)
}

fn load(path: &Path, service: Option<&ShapeId>) -> Result<Service, String> {
    let json = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let model = Model::from_json(&json).map_err(|error| format!("{}: {error}", path.display()))?;
    // Lower all recognized semantics. The comparator diagnoses its own coverage;
    // generator-specific capability limits must not hide a contract change.
    model
        .lower_with_capabilities(service, &Capabilities::new(Feature::ALL.iter().copied()))
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn emit(report: Report, json: bool) -> Result<(), String> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?
        );
    } else {
        print!("{report}");
        if report.findings.is_empty() {
            println!("No policy violations found.");
        }
    }
    if report.has_errors() {
        Err("API policy check failed".into())
    } else {
        Ok(())
    }
}
