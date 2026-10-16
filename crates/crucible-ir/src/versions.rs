//! The release chain as the reference reads it (docs/api-versions-clients.md,
//! "API documentation across releases"; decisions 77 and 78): when each
//! element first appeared, which migrations changed it, and which releases
//! cannot see it. Settled here, once, so every renderer dates an element
//! the same way.
//!
//! The chain is `Service::versions`, oldest first, then the service itself.
//! An element is an operation, a shape, a member (`Shape$member`) or an enum
//! value (`Enum$NAME`), followed back through `rename` steps: a shape keeps
//! its id in every release (decision 71), and a member or enum value renamed
//! in a migration is the same element under its older name before it.

use std::collections::{BTreeMap, BTreeSet};

use crate::ShapeId;
use crate::ir::{ApiVersion, Hook, Service, Shape, VersionStep};

/// The header a client names its release in: the one carrier until a
/// protocol declares another (decision 57).
pub const VERSION_HEADER: &str = "Crucible-Version";

/// When an element arrived and changed, by release name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    /// The first release that has the element, `None` when the oldest
    /// supported release has it: a dropped release takes its history with
    /// it, and "before every supported release" stays true.
    pub added: Option<String>,
    /// Each release whose migration has a step naming the element among its
    /// [`subjects`], oldest first, without repeats.
    pub changed: Vec<String>,
    /// The supported releases that cannot see the element although it
    /// appeared before them or since: those before a `reshape` of the
    /// structure it is a member of, or a `remove operation` whose own input
    /// or output it is a member of, when the older definition lacks it
    /// (decision 74). Oldest first; empty for nearly everything.
    pub missing: Vec<String>,
}

/// The [`History`] of every element of one release of a chain.
#[derive(Debug, Clone, Default)]
pub struct Histories {
    elements: BTreeMap<ShapeId, History>,
}

impl Histories {
    /// The element's history; an id the release does not have is dated
    /// before every supported release, as an element with no history is.
    pub fn get(&self, id: &ShapeId) -> &History {
        static NONE: History = History {
            added: None,
            changed: Vec::new(),
            missing: Vec::new(),
        };
        self.elements.get(id).unwrap_or(&NONE)
    }

    /// Every element dated, by id.
    pub fn iter(&self) -> impl Iterator<Item = (&ShapeId, &History)> {
        self.elements.iter()
    }
}

/// The history of every element of `service`, the current release, through
/// its [`Service::versions`]. A service without versions dates nothing: its
/// one release is the oldest supported.
pub fn history(service: &Service) -> Histories {
    let versions = visible(service);
    let mut chain: Vec<(&str, &Service)> = versions
        .iter()
        .map(|version| (version.name.as_str(), &*version.service))
        .collect();
    chain.push((current_name(service), service));
    let steps: Vec<&[VersionStep]> = versions
        .iter()
        .map(|version| version.steps.as_slice())
        .collect();
    dated(&chain, &steps)
}

/// The history of every element of the older release `versions[release]`
/// of `service`, within the chain that ends there: what a definition
/// restored from that release, such as an older side the reference shows,
/// is dated by. Empty past the end of the chain.
pub fn history_at(service: &Service, release: usize) -> Histories {
    let visible = visible(service);
    let Some(versions) = visible.get(..=release) else {
        return Histories::default();
    };
    let chain: Vec<(&str, &Service)> = versions
        .iter()
        .map(|version| (version.name.as_str(), &*version.service))
        .collect();
    let steps: Vec<&[VersionStep]> = versions[..release]
        .iter()
        .map(|version| version.steps.as_slice())
        .collect();
    dated(&chain, &steps)
}

/// The name of the current release: the `api_version` a client sends, else
/// the service's `version`.
pub fn current_name(service: &Service) -> &str {
    service.api_version.as_deref().unwrap_or(&service.version)
}

/// The ids a step names on its newer side, which `History::changed` reads:
/// the member for `rename`, `require` and `retype`, the structure for
/// `reshape`, the enum for an added value, and the operation for an added
/// error, `move` and `behavior`. A `remove operation` names the older
/// release's operation, which its newer side does not have.
pub fn subjects(step: &VersionStep) -> Vec<ShapeId> {
    match step {
        VersionStep::Rename { new, to, .. } => vec![new.with_member(to)],
        VersionStep::Require { new, member, .. } => vec![new.with_member(member)],
        VersionStep::AddEnumValue { new, .. } => vec![new.clone()],
        VersionStep::AddError { operation, .. }
        | VersionStep::Move { operation, .. }
        | VersionStep::Behavior { operation, .. } => vec![operation.clone()],
        VersionStep::Hooked { hook, .. } => match hook {
            Hook::Reshape { new, .. } => vec![new.clone()],
            Hook::Retype { new, member, .. } | Hook::Require { new, member, .. } => {
                vec![new.with_member(member)]
            }
            Hook::RemoveOperation { operation } => vec![operation.clone()],
        },
    }
}

/// Whether `service` has the element `id`: an operation, a shape, a member
/// of a structure, union, list or map, or an enum value by name.
pub fn has(service: &Service, id: &ShapeId) -> bool {
    match id.member() {
        None => service.shapes.contains_key(id) || service.operations.iter().any(|op| &op.id == id),
        Some(name) => match service.shapes.get(&id.root()) {
            Some(Shape::Enum(e)) => e.values.iter().any(|v| v.name == name),
            Some(Shape::IntEnum(e)) => e.values.iter().any(|v| v.name == name),
            Some(shape) => shape.members().iter().any(|m| m.name == name),
            None => false,
        },
    }
}

/// Every element of `service`, as [`has`] knows them.
fn elements(service: &Service) -> Vec<ShapeId> {
    let mut out: Vec<ShapeId> = service.operations.iter().map(|op| op.id.clone()).collect();
    for (id, shape) in &service.shapes {
        out.push(id.clone());
        match shape {
            Shape::Enum(e) => out.extend(e.values.iter().map(|v| id.with_member(&v.name))),
            Shape::IntEnum(e) => out.extend(e.values.iter().map(|v| id.with_member(&v.name))),
            shape => out.extend(shape.members().iter().map(|m| id.with_member(&m.name))),
        }
    }
    out
}

/// The element `id` of the newer release as the release before `steps`
/// names it: its older name when a `rename` among them renamed it.
fn older_id(steps: &[VersionStep], id: &ShapeId) -> ShapeId {
    let Some(member) = id.member() else {
        return id.clone();
    };
    steps
        .iter()
        .find_map(|step| match step {
            VersionStep::Rename {
                old, new, from, to, ..
            } if *new == id.root() && to == member && !from.is_empty() => {
                Some(old.with_member(from))
            }
            _ => None,
        })
        .unwrap_or_else(|| id.clone())
}

/// The structures whose older definition a migration restores whole, so
/// that what was added to them since does not reach the releases before it
/// (decision 74): a reshaped structure, by its newer id, and a removed
/// operation's own input and output, as the older release has them.
fn restored_whole(steps: &[VersionStep], older: &Service) -> BTreeSet<ShapeId> {
    let mut out = BTreeSet::new();
    for step in steps {
        let VersionStep::Hooked { hook, .. } = step else {
            continue;
        };
        match hook {
            Hook::Reshape { new, .. } => {
                out.insert(new.clone());
            }
            Hook::RemoveOperation { operation } => {
                if let Some(op) = older.operations.iter().find(|op| &op.id == operation) {
                    out.extend(op.input.iter().chain(&op.output).cloned());
                }
            }
            _ => {}
        }
    }
    out
}

/// Dates every element of the chain's last release. `chain[i]` is a
/// release, oldest first, and `steps[i]` the migration from `chain[i]` to
/// `chain[i + 1]`.
fn dated(chain: &[(&str, &Service)], steps: &[&[VersionStep]]) -> Histories {
    let Some(&(_, last)) = chain.last() else {
        return Histories::default();
    };
    let newest = chain.len() - 1;
    // Each migration's subjects, and the structures it restores whole.
    let subjects: Vec<BTreeSet<ShapeId>> = steps
        .iter()
        .map(|steps| steps.iter().flat_map(subjects).collect())
        .collect();
    let whole: Vec<BTreeSet<ShapeId>> = steps
        .iter()
        .enumerate()
        .map(|(index, steps)| restored_whole(steps, chain[index].1))
        .collect();
    let mut dated = BTreeMap::new();
    for element in elements(last) {
        let mut id = element.clone();
        let mut first = newest;
        let mut changed = Vec::new();
        // Back through the chain, one migration at a time, while the
        // release before it still has the element.
        for index in (0..newest).rev() {
            if subjects[index].contains(&id) {
                changed.push(chain[index + 1].0.to_owned());
            }
            let older = older_id(steps[index], &id);
            if !has(chain[index].1, &older) {
                break;
            }
            first = index;
            id = older;
        }
        changed.reverse();
        changed.dedup();
        // A member of a structure some migration restores whole, which the
        // release before that migration lacks: every release up to that one
        // sees the older definition, without it.
        let mut missing_through = None;
        if element.member().is_some() {
            let parent = element.root();
            for (index, whole) in whole.iter().enumerate() {
                if first > index && whole.contains(&parent) {
                    missing_through = Some(index);
                }
            }
        }
        let missing = missing_through.map_or_else(Vec::new, |through| {
            chain[..=through]
                .iter()
                .map(|(name, _)| (*name).to_owned())
                .collect()
        });
        dated.insert(
            element,
            History {
                added: (first > 0).then(|| chain[first].0.to_owned()),
                changed,
                missing,
            },
        );
    }
    Histories { elements: dated }
}

/// What a step names on each side: on its older side, ids in the release
/// the migration comes from; on its newer side, ids in the release it
/// serves. An external reference keeps a step only when its audience sees
/// every one of them ([`visible`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Named {
    pub older: Vec<ShapeId>,
    pub newer: Vec<ShapeId>,
}

/// Every id `step` names, on each side: its [`subjects`], the member's older
/// name, an enum value and its downgrade, an error, its downgrade and the
/// members they map, a reshape's new member, a removed operation. A
/// reshape's replaced members are left out: [`visible`] keeps those the
/// audience sees.
pub fn named(step: &VersionStep) -> Named {
    let mut older = Vec::new();
    let mut newer = subjects(step);
    match step {
        VersionStep::AddEnumValue {
            old,
            new,
            value,
            downgrade,
            ..
        } => {
            older.push(old.with_member(downgrade));
            newer.push(new.with_member(value));
        }
        VersionStep::AddError {
            error,
            downgrade,
            members,
            ..
        } => {
            older.push(downgrade.clone());
            older.extend(
                members
                    .iter()
                    .map(|(member, _)| downgrade.with_member(member)),
            );
            newer.push(error.clone());
            newer.extend(members.iter().map(|(_, from)| error.with_member(from)));
        }
        VersionStep::Move { operation, .. } => older.push(operation.clone()),
        VersionStep::Hooked {
            hook: Hook::Reshape { old, new, into, .. },
            ..
        } => {
            older.push(old.clone());
            newer.push(new.with_member(into));
        }
        VersionStep::Hooked {
            hook: Hook::RemoveOperation { operation },
            ..
        } => {
            older.push(operation.clone());
            newer.clear();
        }
        _ => {}
    }
    older.extend(older_member(step));
    Named { older, newer }
}

/// The member a `rename`, `require` or `retype` names, as the release the
/// migration comes from has it: under its older name when the same
/// migration renamed it.
pub fn older_member(step: &VersionStep) -> Option<ShapeId> {
    let (old, name) = match step {
        VersionStep::Rename { old, from, .. } => (old, from),
        VersionStep::Require {
            old, member, from, ..
        }
        | VersionStep::Hooked {
            hook:
                Hook::Retype {
                    old, member, from, ..
                }
                | Hook::Require {
                    old, member, from, ..
                },
            ..
        } => (old, if from.is_empty() { member } else { from }),
        _ => return None,
    };
    Some(old.with_member(name))
}

/// `service.versions` as its audience sees them. An external projection
/// marks the older releases ([`crate::ir::ApiVersion::external`]) and this
/// projects them, best effort and never refused: each older model without
/// its internal API, whatever the strict projection would refuse; a step
/// only when that audience sees every id it [`named`] on each side, a
/// reshape's replaced members and an error's member mapping kept to those
/// it sees; and a retired operation only when it sees it. Documentation is
/// kept as written. As lowered, the versions are returned as they are.
pub fn visible(service: &Service) -> std::borrow::Cow<'_, [ApiVersion]> {
    let versions = &service.versions;
    if !versions.iter().any(|version| version.external) {
        return std::borrow::Cow::Borrowed(versions);
    }
    let models: Vec<Service> = versions
        .iter()
        .map(|version| crate::audience::lenient_external(&version.service))
        .collect();
    let mut out = Vec::with_capacity(versions.len());
    for (index, (version, model)) in versions.iter().zip(&models).enumerate() {
        let newer = models.get(index + 1).unwrap_or(service);
        let steps = version
            .steps
            .iter()
            .filter(|step| {
                let named = named(step);
                named.older.iter().all(|id| has(model, id))
                    && named.newer.iter().all(|id| has(newer, id))
            })
            .map(|step| visible_parts(step, model, newer))
            .collect();
        let retired = version
            .retired
            .iter()
            .filter(|id| has(model, id))
            .cloned()
            .collect();
        out.push(ApiVersion {
            name: version.name.clone(),
            service: Box::new(model.clone()),
            steps,
            sunset: version.sunset.clone(),
            sunset_docs: version.sunset_docs.clone(),
            docs: version.docs.clone(),
            retired,
            external: false,
        });
    }
    std::borrow::Cow::Owned(out)
}

/// A step with the lists it carries kept to what the audience sees: a
/// reshape's replaced members, an error's member mapping.
fn visible_parts(step: &VersionStep, older: &Service, newer: &Service) -> VersionStep {
    let mut step = step.clone();
    match &mut step {
        VersionStep::Hooked {
            hook: Hook::Reshape { old, members, .. },
            ..
        } => members.retain(|member| has(older, &old.with_member(member))),
        VersionStep::AddError {
            error,
            downgrade,
            members,
            ..
        } => members.retain(|(member, from)| {
            has(older, &downgrade.with_member(member)) && has(newer, &error.with_member(from))
        }),
        _ => {}
    }
    step
}
