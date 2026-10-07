use crate::checker::Checker;
use crate::{Config, Report};
use crucible_smithy::ir::Service;
use std::collections::BTreeSet;

/// Compare old clients against a new service, and optionally the model's SDK symbols.
/// Unsupported wire families produce a finding instead of a compatibility pass.
pub fn diff(old: &Service, new: &Service, config: &Config) -> Report {
    let mut checker = Checker {
        old,
        new,
        severity: config.compatibility.severity,
        report: Report::default(),
        seen: BTreeSet::new(),
    };
    if config.compatibility.wire {
        checker.wire();
    }
    if config.compatibility.sdk_source {
        checker.sdk();
    }
    checker.report.finish(config)
}
