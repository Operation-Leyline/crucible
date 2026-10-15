use std::path::Path;

use crucible_policy::{Config, Report};
use crucible_smithy::ShapeId;
use crucible_smithy::ir::Service;

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
    // The comparator diagnoses its own coverage, so no generator capability
    // limit is applied; and the baseline guard is skipped, since the two
    // packages named here are the comparison.
    crate::lower_cru(
        &crate::Source {
            model: path.to_path_buf(),
            service: service.cloned(),
        },
        false,
    )
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
