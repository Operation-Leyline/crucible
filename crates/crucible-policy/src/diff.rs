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
        full_new: new,
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
        // SDKs are generated from the client view, without the operations a
        // third party calls (decision 81): an operation that becomes
        // sender-signed leaves every SDK, with the types only it reached.
        let old_view = crucible_ir::client_view(old);
        let new_view = crucible_ir::client_view(new);
        let mut sdk = Checker {
            old: &old_view,
            new: &new_view,
            full_new: new,
            renames,
            severity: compatibility.sdk_source.severity_in(compatibility.severity),
            report: std::mem::take(&mut checker.report),
            seen: BTreeSet::new(),
        };
        sdk.sdk();
        sdk.permissions_sdk();
        checker.report = sdk.report;
    }
    checker.report.finish(config)
}
