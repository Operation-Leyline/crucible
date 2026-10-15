use crate::checker::Checker;
use crate::{Config, Report};
use crucible_ir::ShapeId;
use crucible_ir::ir::Service;
use std::collections::{BTreeMap, BTreeSet};

/// Members renamed between two releases: `(old structure, old member)` to
/// `(new structure, new member)`.
pub type Renames = BTreeMap<(ShapeId, String), (ShapeId, String)>;

/// Compare old clients against a new service, and optionally the model's SDK symbols.
/// Unsupported wire families produce a finding instead of a compatibility pass.
pub fn diff(old: &Service, new: &Service, config: &Config) -> Report {
    diff_renamed(old, new, config, &Renames::new())
}

/// [`diff`], with members a migration renamed compared with their new
/// names: a renamed member is checked like any other (presence, default,
/// type, constraints), so the rename accounts for its name alone.
pub fn diff_renamed(old: &Service, new: &Service, config: &Config, renames: &Renames) -> Report {
    let mut checker = Checker {
        old,
        new,
        renames,
        severity: config.compatibility.severity,
        report: Report::default(),
        seen: BTreeSet::new(),
    };
    let compatibility = &config.compatibility;
    if compatibility.wire.value {
        checker.severity = compatibility.wire.severity_in(compatibility.severity);
        checker.wire();
        checker.permissions_wire();
    }
    if compatibility.sdk_source.value {
        checker.severity = compatibility.sdk_source.severity_in(compatibility.severity);
        checker.sdk();
        checker.permissions_sdk();
    }
    checker.report.finish(config)
}
