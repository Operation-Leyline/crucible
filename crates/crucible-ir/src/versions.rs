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

mod identity;
pub use identity::identical_operation_closure;

mod line;
mod projection;
pub use line::{is_line, line, line_current, releases};
pub use projection::{
    Projection, current_permission, projections, rename_path, undo, undo_restoring,
};

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
    /// or output it is a member of, when the older definition lacks it,
    /// and those whose payload-bound message has no body for it (decision
    /// 74). Oldest first; empty for nearly everything.
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
///
/// A [`line`] of an older release is dated where each element comes from
/// (docs/api-versions-clients.md, "Each element's history"; decision 79):
/// what undo restored from frozen release `r` is added and missing as the
/// chain that ends at `r` dates it ([`history_at`]), and changed in that
/// chain, in the release whose migration restored it, and in every release
/// since with a step naming what it became, through the renames; everything
/// else as the current model dates it, followed through the renames since
/// the line's release.
pub fn history(service: &Service) -> Histories {
    history_cached(service, &HistoryCache::default())
}

/// [`history`], reading and filling `cache`, which must only ever hold the
/// histories of one current model at one audience: every line of a model a
/// run documents shares them.
pub fn history_cached(service: &Service, cache: &HistoryCache) -> Histories {
    if let Some(release) = line::line_index(service) {
        return line_history(service, release, cache);
    }
    let versions = visible(service);
    let mut chain: Vec<(&str, &Service)> = versions
        .iter()
        .map(|version| (version.name.as_str(), &*version.release))
        .collect();
    chain.push((current_name(service), service));
    let steps: Vec<&[VersionStep]> = versions
        .iter()
        .map(|version| version.steps.as_slice())
        .collect();
    let projections: Vec<&Service> = versions.iter().map(|version| &*version.service).collect();
    dated(&chain, &steps, &projections, service)
}

/// What one run computes once and reads many times: the histories of one
/// current model's older releases ([`history_at`]) and the current model's
/// own as its lines read it. A reference of each release reads the same
/// ones, so a run documenting every release computes each once.
#[derive(Debug, Default)]
pub struct HistoryCache {
    current: std::cell::OnceCell<Histories>,
    at: std::cell::RefCell<BTreeMap<usize, std::rc::Rc<Histories>>>,
}

impl HistoryCache {
    /// [`history_at`]`(service, release)`, computed on first use. An older
    /// release's history reads only the chain, which every line of the
    /// cache's model and its current reference share.
    pub fn at(&self, service: &Service, release: usize) -> std::rc::Rc<Histories> {
        if let Some(found) = self.at.borrow().get(&release) {
            return found.clone();
        }
        let histories = std::rc::Rc::new(history_at(service, release));
        self.at.borrow_mut().insert(release, histories.clone());
        histories
    }
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
        .map(|version| (version.name.as_str(), &*version.release))
        .collect();
    let steps: Vec<&[VersionStep]> = versions[..release]
        .iter()
        .map(|version| version.steps.as_slice())
        .collect();
    let projections: Vec<&Service> = versions[..release]
        .iter()
        .map(|version| &*version.service)
        .collect();
    dated(&chain, &steps, &projections, &versions[release].service)
}

/// The history of each element of `line`, the line of its release
/// `versions[release]`. The oldest release at or after it whose undo
/// restored the element, which is where its definition comes from, dates
/// its addition and what cannot see it within the chain that ends there.
/// Its changes are that chain's, the migration that restored it, and each
/// later migration with a step naming what it became, so a reader of the
/// line sees each break that reached it, also after a later step removed
/// it from the current model. An element nothing restored is
/// the current model's, under its current id.
fn line_history(line: &Service, release: usize, cache: &HistoryCache) -> Histories {
    let view;
    let line = match visible(line) {
        std::borrow::Cow::Borrowed(_) => line,
        std::borrow::Cow::Owned(versions) => {
            view = Service {
                versions,
                ..line.clone()
            };
            &view
        }
    };
    let Some(current) = line_current(line) else {
        return Histories::default();
    };
    // The current release is the chain's last entry.
    let newest = line.versions.len() - 1;
    let names: Vec<&str> = line.versions.iter().map(|v| v.name.as_str()).collect();
    let dates = cache.current.get_or_init(|| history(&current));
    let mut out = BTreeMap::new();
    for element in elements(line) {
        // The element as each release from the line's on names it, until
        // the undo that restored it, if any.
        let mut id = element.clone();
        let mut restored_at = None;
        for (index, version) in line.versions.iter().enumerate().take(newest).skip(release) {
            if restored(version, &id) {
                restored_at = Some(index);
                break;
            }
            id = newer_id(&version.steps, &id);
        }
        let Some(index) = restored_at else {
            out.insert(element, dates.get(&id).clone());
            continue;
        };
        let mut history = cache.at(&current, index).get(&id).clone();
        // The migration from the release it was restored from changed it,
        // and so did every later one with a step naming what it became,
        // followed through the renames, also once a later step removed it
        // from the current model. A `remove operation` names the operation
        // as the release before it has it.
        let mut changed: BTreeSet<&str> = history.changed.iter().map(String::as_str).collect();
        changed.insert(names[index + 1]);
        let mut id = newer_id(&line.versions[index].steps, &id);
        for (later, version) in line
            .versions
            .iter()
            .enumerate()
            .take(newest)
            .skip(index + 1)
        {
            let next = newer_id(&version.steps, &id);
            if version.steps.iter().any(|step| {
                subjects(step)
                    .iter()
                    .any(|subject| *subject == next || *subject == id)
            }) {
                changed.insert(names[later + 1]);
            }
            id = next;
        }
        history.changed = names
            .iter()
            .filter(|name| changed.contains(*name))
            .map(|name| (*name).to_owned())
            .collect();
        out.insert(element, history);
    }
    Histories { elements: out }
}

/// Whether the undo of `version` restored the element `id` from its frozen
/// release: the element itself, or the definition it is a member of.
fn restored(version: &ApiVersion, id: &ShapeId) -> bool {
    let found = |id: &ShapeId| version.restored.binary_search(id).is_ok();
    found(id) || (id.member().is_some() && found(&id.root()))
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

/// The element `id` of the older release as the release after `steps`
/// names it: its newer name when a `rename` among them renamed it.
fn newer_id(steps: &[VersionStep], id: &ShapeId) -> ShapeId {
    let Some(member) = id.member() else {
        return id.clone();
    };
    steps
        .iter()
        .find_map(|step| match step {
            VersionStep::Rename {
                old, new, from, to, ..
            } if *old == id.root() && from == member && !to.is_empty() => Some(new.with_member(to)),
            _ => None,
        })
        .unwrap_or_else(|| id.clone())
}

/// Whether `model` sends `shape` with a payload: an operation's input or
/// output bound to one, or an error whose body is one. Such a message has
/// no body for a member added since (decision 74).
fn payload_bound(model: &Service, shape: &ShapeId) -> bool {
    let operation = model.operations.iter().any(|op| {
        (op.input.as_ref() == Some(shape) && op.request.payload.is_some())
            || (op.output.as_ref() == Some(shape) && op.response.payload.is_some())
    });
    // Defensive: no `.cru` model reaches an error here. The guard refuses
    // a change to an error's wire bindings, and validation refuses a member
    // beside an error's payload, so an error never gains a body member its
    // release lacks. Kept so an edited IR is still dated right.
    operation
        || matches!(
            model.shapes.get(shape),
            Some(Shape::Structure(s)) if s.error.as_ref().is_some_and(|e| e.payload.is_some())
        )
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
/// release, oldest first, as frozen, and `steps[i]` the migration from
/// `chain[i]` to `chain[i + 1]`; `projections[i]` is `chain[i]` as the
/// current model sees it, and `top` the last release's.
fn dated(
    chain: &[(&str, &Service)],
    steps: &[&[VersionStep]],
    projections: &[&Service],
    top: &Service,
) -> Histories {
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
        // A member a release's payload-bound message has no body for
        // (decision 74): its projection lacks it although the next one has
        // it, and every release before that one sees the message without it.
        if element.member().is_some() {
            let mut newer = element.clone();
            for index in (0..newest).rev() {
                let older = older_id(steps[index], &newer);
                let next = projections.get(index + 1).copied().unwrap_or(top);
                let Some(projection) = projections.get(index) else {
                    break;
                };
                if has(next, &newer)
                    && !has(projection, &older)
                    && payload_bound(projection, &older.root())
                {
                    missing_through = missing_through.max(Some(index));
                    break;
                }
                newer = older;
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

/// Every id that makes `step` what it is, on each side: its [`subjects`],
/// the member's older name, an enum value and its downgrade, an error and
/// its downgrade, a reshape's new member, a moved or removed operation.
/// The lists of parts a step carries are not among them, since they do not
/// decide whether the audience sees the change: a reshape's replaced
/// members and an error's member mapping, which [`visible`] cuts to what
/// the audience sees.
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
            error, downgrade, ..
        } => {
            older.push(downgrade.clone());
            newer.push(error.clone());
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
/// kept as written. Each projected version's `service` is its projection's
/// view, best effort as well, which the history reads; nothing serves an
/// external view. As lowered, the versions are returned as they are.
pub fn visible(service: &Service) -> std::borrow::Cow<'_, [ApiVersion]> {
    let versions = &service.versions;
    if !versions.iter().any(|version| version.external) {
        return std::borrow::Cow::Borrowed(versions);
    }
    let models: Vec<Service> = versions
        .iter()
        .map(|version| crate::audience::lenient_external(&version.release))
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
        // The reference reads the release, and the projection where it
        // dates what a release's payload-bound message leaves out.
        out.push(ApiVersion {
            name: version.name.clone(),
            service: Box::new(crate::audience::lenient_external(&version.service)),
            release: Box::new(model.clone()),
            steps,
            sunset: version.sunset.clone(),
            sunset_docs: version.sunset_docs.clone(),
            docs: version.docs.clone(),
            retired,
            restored: version.restored.clone(),
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
