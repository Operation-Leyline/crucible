//! SDK lines (docs/api-versions-clients.md, 5c; decision 76): a client,
//! document or CLI generated for one supported release, from the current
//! model as that release sees it.
//!
//! A line is [`ApiVersion::service`], the release's projection, with the
//! whole chain beside it: every older release as lowered, then the current
//! release as one more entry, whose `service` and `release` are the current
//! model. So a line is recognizable without a field of its own (its
//! `version` names one of its own `versions`), and what reads a line can
//! still find the current release: the reference's `versions` block names
//! it, and an SDK description finds each other line from it.

use crate::Diagnostics;
use crate::ir::{ApiVersion, Service};

use super::current_name;

/// `service`, the current model, as its release `release` sees it: the
/// model every output but a server is generated from for that release. The
/// current release is `service` itself. A release outside the chain is a
/// diagnostic naming the supported ones; a sunset release is still in the
/// chain until its migration is removed, so it can still be generated.
pub fn line(service: &Service, release: &str) -> Result<Service, Diagnostics> {
    if let Some(index) = line_index(service) {
        let mut diagnostics = Diagnostics::default();
        diagnostics.push(
            &service.id,
            format!(
                "this model is already release \"{}\" of the service as the current model sees it; select a release from the current model",
                service.versions[index].name
            ),
        );
        return Err(diagnostics);
    }
    if release == current_name(service) {
        return Ok(service.clone());
    }
    let Some(version) = service
        .versions
        .iter()
        .find(|version| version.name == release)
    else {
        let mut diagnostics = Diagnostics::default();
        diagnostics.push(
            &service.id,
            format!(
                "release {release:?} is not one {} serves: its releases are {}",
                service.id,
                releases(service)
                    .iter()
                    .map(|name| format!("{name:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
        return Err(diagnostics);
    };
    let mut current = service.clone();
    let mut chain = std::mem::take(&mut current.versions);
    let name = current_name(service).to_owned();
    let current = Box::new(current);
    // The current release's entry holds the current model twice, as
    // `service` and `release`, since both are owned boxes: sharing one would
    // change `ApiVersion`'s types, which the IR serializes. A line is built
    // once per generated output, at build time, so the copy is a model's
    // size once, never per request.
    chain.push(ApiVersion {
        name,
        service: current.clone(),
        release: current,
        steps: Vec::new(),
        sunset: None,
        sunset_docs: None,
        docs: None,
        retired: Vec::new(),
        restored: Vec::new(),
        external: false,
    });
    let mut line = (*version.service).clone();
    line.versions = chain;
    Ok(line)
}

/// The supported releases of `service`, oldest first, the current one
/// last: the names [`line`] takes.
pub fn releases(service: &Service) -> Vec<&str> {
    match line_index(service) {
        Some(_) => service
            .versions
            .iter()
            .map(|version| version.name.as_str())
            .collect(),
        None => service
            .versions
            .iter()
            .map(|version| version.name.as_str())
            .chain(std::iter::once(current_name(service)))
            .collect(),
    }
}

/// Whether `service` is a line of an older release, as [`line`] builds
/// one: its `version` names one of its own `versions`. A server is never
/// generated from one; it serves every release from the current model.
pub fn is_line(service: &Service) -> bool {
    line_index(service).is_some()
}

/// The index in `service.versions` of the release a line was made for.
pub(super) fn line_index(service: &Service) -> Option<usize> {
    let name = current_name(service);
    service
        .versions
        .iter()
        .position(|version| version.name == name)
}

/// The current model a line was made from, with its own chain, or `None`
/// when `service` is not a line. The versions keep their audience marks,
/// so a line projected to an audience gives the current model with its
/// older releases left to [`super::visible`], as an audience projection
/// leaves them; the current model itself is as lowered.
pub fn line_current(service: &Service) -> Option<Service> {
    line_index(service)?;
    let (last, older) = service.versions.split_last()?;
    let mut current = (*last.release).clone();
    current.versions = older.to_vec();
    Some(current)
}
