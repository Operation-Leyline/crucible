//! Audience semantics operate only on lowered IR, independently of an emitter.
use std::collections::{BTreeMap, BTreeSet};

use crate::ir::{Bindings, Member, Operation, Service, Shape};
use crate::{Diagnostics, ShapeId};

/// Public packages omit internal API unless their producer explicitly opts in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Audience {
    #[default]
    External,
    Internal,
}

/// Project a contract without mutating the full model used by servers and policy.
/// Refusals are collected before returning; callers must not publish partial output.
///
/// An external projection leaves the older releases in `versions` as they
/// are and marks them ([`crate::ir::ApiVersion::external`]): only the
/// reference reads them, through [`crate::versions::visible`], which
/// projects them then, best effort, so an older release this audience
/// could not project never fails an output that does not read it.
pub fn project(service: &Service, audience: Audience) -> Result<Service, Diagnostics> {
    if audience == Audience::Internal {
        return Ok(service.clone());
    }
    external(service, false)
}

/// What a client calls (decision 81): `service` without its sender-signed
/// operations, which a third party calls and signs, and without what only
/// they reached. Client generators and the service CLI read this view of a
/// projected service, and MCP tools leave the same operations out;
/// documentation and the server keep them.
pub fn client_view(service: &Service) -> Service {
    let removed: BTreeSet<ShapeId> = service
        .operations
        .iter()
        .filter(|op| op.sender.is_some())
        .map(|op| op.id.clone())
        .collect();
    if removed.is_empty() {
        return service.clone();
    }
    let reached = |operations: &mut dyn Iterator<Item = &Operation>| {
        let mut reached = BTreeSet::new();
        let mut pending: Vec<ShapeId> = operations
            .flat_map(|op| op.input.iter().chain(&op.output).chain(&op.errors))
            .chain(&service.errors)
            .cloned()
            .collect();
        while let Some(id) = pending.pop() {
            if let Some(shape) = service.shape(&id)
                && reached.insert(id)
            {
                pending.extend(shape.members().into_iter().map(|m| m.target.clone()));
            }
        }
        reached
    };
    let before = reached(&mut service.operations.iter());
    let after = reached(
        &mut service
            .operations
            .iter()
            .filter(|op| !removed.contains(&op.id)),
    );
    let mut result = service.clone();
    result.operations.retain(|op| !removed.contains(&op.id));
    // A shape no operation reached before stays: what reaches it is not
    // this view's to judge. One only the removed operations reached goes.
    result
        .shapes
        .retain(|id, _| !before.contains(id) || after.contains(id));
    for resource in result.resources.values_mut() {
        for slot in [
            &mut resource.lifecycle.create,
            &mut resource.lifecycle.put,
            &mut resource.lifecycle.read,
            &mut resource.lifecycle.update,
            &mut resource.lifecycle.delete,
            &mut resource.lifecycle.list,
        ] {
            if slot.as_ref().is_some_and(|id| removed.contains(id)) {
                *slot = None;
            }
        }
        resource.operations.retain(|id| !removed.contains(id));
        resource
            .collection_operations
            .retain(|id| !removed.contains(id));
        resource.bindings.retain(|id, _| !removed.contains(id));
    }
    let present: BTreeSet<ShapeId> = result.shapes.keys().cloned().collect();
    result
        .resource_references
        .retain(|id, _| present.contains(id));
    result.recursive_groups.retain_mut(|group| {
        group.shapes.retain(|id| present.contains(id));
        group.members.retain(|id| present.contains(&id.root()));
        !group.shapes.is_empty()
    });
    // A capability a removed operation, or a shape only it reached, used.
    result.requirements.retain(|usage| {
        let root = usage.shape.root();
        !removed.contains(&root) && (!service.shapes.contains_key(&root) || present.contains(&root))
    });
    // A doc link to a removed operation is its text.
    result.retain_doc_links();
    result
}

/// An older release as the external audience sees it, never refused: what
/// [`project`] would refuse (an internal member that is required, an
/// internal target, an example naming what it removes) is left out instead,
/// with what then reaches nothing. The reference reads older releases this
/// way, where a frozen release's internal API must not show and no refusal
/// may stop the current release's reference.
pub(crate) fn lenient_external(service: &Service) -> Service {
    external(service, true).unwrap_or_else(|_| unreachable!("a lenient projection refuses nothing"))
}

/// The external projection; `lenient` keeps going past every refusal and
/// drops what the refused construct would have left dangling, so it is
/// never `Err`.
fn external(service: &Service, lenient: bool) -> Result<Service, Diagnostics> {
    let mut result = service.clone();
    result
        .errors
        .retain(|id| !service.internal && !service.shape(id).is_some_and(Shape::internal));
    // CORS describes the deployed server contract, which audience selection
    // never projects. Keep its full header and method lists on client views too.
    result
        .operations
        .retain(|op| !service.internal && !op.internal);
    for op in &mut result.operations {
        op.errors
            .retain(|id| !service.shape(id).is_some_and(Shape::internal));
    }
    let operations: BTreeSet<_> = result.operations.iter().map(|op| op.id.clone()).collect();
    result
        .resources
        .retain(|_, resource| !service.internal && !resource.internal);
    // Only resources bound through surviving roots are part of this audience.
    let mut resources = BTreeSet::new();
    let mut pending = service.resource_roots.clone();
    while let Some(id) = pending.pop() {
        if let Some(resource) = result.resources.get(&id)
            && resources.insert(id)
        {
            pending.extend(resource.resources.iter().cloned());
        }
    }
    result.resources.retain(|id, _| resources.contains(id));
    result.resource_roots.retain(|id| resources.contains(id));
    let mut diagnostics = Diagnostics::default();
    for resource in result.resources.values_mut() {
        resource.resources.retain(|id| resources.contains(id));
        for (name, slot) in [
            ("create", &mut resource.lifecycle.create),
            ("put", &mut resource.lifecycle.put),
            ("read", &mut resource.lifecycle.read),
            ("update", &mut resource.lifecycle.update),
            ("delete", &mut resource.lifecycle.delete),
            ("list", &mut resource.lifecycle.list),
        ] {
            if let Some(id) = slot.as_ref()
                && !operations.contains(id)
            {
                diagnostics.push(&resource.id, format!("audience.resource_lifecycle: external resource {name} lifecycle requires removed internal operation {id}"));
                *slot = None;
            }
        }
        resource.operations.retain(|id| operations.contains(id));
        resource
            .collection_operations
            .retain(|id| operations.contains(id));
        resource.bindings.retain(|id, _| operations.contains(id));
    }
    let mut reached = BTreeSet::new();
    let mut pending: Vec<_> = result
        .operations
        .iter()
        .flat_map(|op| op.input.iter().chain(&op.output).chain(&op.errors))
        .chain(&result.errors)
        .cloned()
        .collect();
    // A resource names its entries' types without reaching them: the
    // audience's shapes are what its operations reach, and the compiler
    // refuses an entry of an internal type (decision 67).
    for op in &result.operations {
        for target in op.input.iter().chain(&op.output) {
            check_target(service, &op.id, target, &mut diagnostics);
        }
        check_references(service, op, &mut diagnostics);
    }
    while let Some(id) = pending.pop() {
        if !reached.insert(id.clone()) {
            continue;
        }
        let Some(shape) = service.shape(&id) else {
            continue;
        };
        if shape.internal() {
            continue;
        }
        for member in shape.members() {
            let member_id = id.with_member(&member.name);
            if member.internal {
                if member.required {
                    diagnostics.push(&member_id, "audience.required_member: external contract requires an internal member that cannot be omitted");
                }
                if matches!(shape, Shape::List(_) | Shape::Map(_)) {
                    diagnostics.push(&member_id, "audience.collection_member: external collection cannot omit its internal member");
                }
            } else {
                check_target(service, &member_id, &member.target, &mut diagnostics);
                if removed_enum_default(service, member) {
                    diagnostics.push(&member_id, "audience.default_reference: external default names a removed internal enum value");
                }
                pending.push(member.target.clone());
            }
        }
    }
    if !lenient && !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    result
        .shapes
        .retain(|id, shape| reached.contains(id) && !shape.internal());
    for shape in result.shapes.values_mut() {
        match shape {
            Shape::Structure(s) | Shape::Union(s) => {
                s.members.retain(|m| !m.internal);
                if let Some(error) = &mut s.error {
                    let names: BTreeSet<_> = s.members.iter().map(|m| m.name.as_str()).collect();
                    error.headers.retain(|b| names.contains(b.member.as_str()));
                    error
                        .timestamp_formats
                        .retain(|n, _| names.contains(n.as_str()));
                    error
                        .header_encodings
                        .retain(|n, _| names.contains(n.as_str()));
                    error.payload = error
                        .payload
                        .take()
                        .filter(|p| names.contains(p.member.as_str()));
                }
            }
            Shape::Enum(s) => s.values.retain(|v| !v.internal),
            Shape::IntEnum(s) => s.values.retain(|v| !v.internal),
            _ => {}
        }
    }
    project_permissions(&mut result);
    // A binding whose path ran through a removed internal member goes with it.
    let roots: BTreeMap<ShapeId, (Option<ShapeId>, Option<ShapeId>)> = result
        .operations
        .iter()
        .map(|op| (op.id.clone(), (op.input.clone(), op.output.clone())))
        .collect();
    for resource in result.resources.values_mut() {
        for (id, bindings) in &mut resource.bindings {
            let Some((input, output)) = roots.get(id) else {
                continue;
            };
            retain_resource_bindings(&result.shapes, input.as_ref(), &mut bindings.input);
            retain_resource_bindings(&result.shapes, output.as_ref(), &mut bindings.output);
        }
    }
    for op in &mut result.operations {
        retain_bindings(&result.shapes, op.input.as_ref(), &mut op.request);
        retain_bindings(&result.shapes, op.output.as_ref(), &mut op.response);
        op.idempotency_token = op
            .idempotency_token
            .take()
            .filter(|n| member_exists(&result.shapes, op.input.as_ref(), n));
        op.resource = op
            .resource
            .take()
            .filter(|id| resources.contains(id) || service.resources.is_empty());
        // Examples are model values too: project nested structure/union members,
        // not only their top-level bindings, before any emitter can see them.
        for example in &mut op.examples {
            let mut valid = true;
            if let Some(value) = &mut example.input {
                valid &= project_example(&result.shapes, op.input.as_ref(), value);
            }
            if let Some(value) = &mut example.output {
                valid &= project_example(&result.shapes, op.output.as_ref(), value);
            }
            example.error = example
                .error
                .take()
                .filter(|e| op.errors.contains(&e.shape));
            if let Some(error) = &mut example.error {
                valid &= project_example(&result.shapes, Some(&error.shape), &mut error.content);
            }
            if !valid {
                diagnostics.push(&op.id, format!("audience.example_reference: example {:?} uses a removed internal union arm or enum value", example.title));
            }
        }
    }
    result
        .resource_references
        .retain(|id, _| reached.contains(id));
    for references in result.resource_references.values_mut() {
        references.retain(|r| !r.resolved || resources.contains(&r.resource));
    }
    result.recursive_groups.retain_mut(|group| {
        group.shapes.retain(|id| reached.contains(id));
        group.members.retain(|id| {
            id.member()
                .is_some_and(|name| member_exists(&result.shapes, Some(&id.root()), name))
        });
        !group.shapes.is_empty()
    });
    result.requirements.retain(|usage| {
        let root = usage.shape.root();
        if root == service.id {
            return true;
        }
        if operations.contains(&root) || resources.contains(&root) {
            return true;
        }
        reached.contains(&root)
            && usage
                .shape
                .member()
                .is_none_or(|name| member_exists(&result.shapes, Some(&root), name))
    });
    // A doc link to what the projection removed is plain text (decision
    // 80): the external contract names no internal id.
    result.retain_doc_links();
    strip_raw_traits(&mut result);
    // The older releases are projected only where they are read (decision
    // 78): mark them, for `versions::visible`.
    for version in &mut result.versions {
        version.external = true;
    }
    if lenient {
        // What `drop_dangling` removes, the bindings, tokens, pagination
        // paths, examples and resource bindings filtered above may still
        // name: project what remains again, until nothing more goes, so
        // every member-name list is filtered against the final shapes.
        if drop_dangling(&mut result) {
            return external(&result, true);
        }
        return Ok(result);
    }
    if diagnostics.is_empty() {
        Ok(result)
    } else {
        Err(diagnostics)
    }
}

/// After a lenient projection, what names a shape it removed goes too: a
/// member whose target is gone, then a collection whose element is, and an
/// operation whose input or output is, until nothing dangles. Whether it
/// removed anything.
fn drop_dangling(service: &mut Service) -> bool {
    let mut removed = false;
    loop {
        let present: BTreeSet<ShapeId> = service.shapes.keys().cloned().collect();
        let mut changed = false;
        service.shapes.retain(|_, shape| match shape {
            Shape::List(list) => present.contains(&list.member.target),
            Shape::Map(map) => {
                present.contains(&map.key.target) && present.contains(&map.value.target)
            }
            _ => true,
        });
        changed |= service.shapes.len() != present.len();
        for shape in service.shapes.values_mut() {
            if let Shape::Structure(s) | Shape::Union(s) = shape {
                let before = s.members.len();
                s.members.retain(|m| present.contains(&m.target));
                changed |= s.members.len() != before;
            }
        }
        let before = service.operations.len();
        service.operations.retain(|op| {
            op.input
                .iter()
                .chain(&op.output)
                .all(|id| present.contains(id))
        });
        changed |= service.operations.len() != before;
        for op in &mut service.operations {
            let before = op.errors.len();
            op.errors.retain(|id| present.contains(id));
            changed |= op.errors.len() != before;
        }
        let before = service.errors.len();
        service.errors.retain(|id| present.contains(id));
        changed |= service.errors.len() != before;
        if !changed {
            return removed;
        }
        removed = true;
    }
}

/// Whether an external view keeps a raw trait application. A raw map holds
/// the author's own traits ([`Service::project_metadata_only`]): one stays
/// unless its definition is `@internal`. A standard trait in IR built some
/// other way is dropped; its typed form remains.
///
/// [`Service::project_metadata_only`]: crate::ir::Service::project_metadata_only
fn external_raw_trait(id: &ShapeId, internal_traits: &BTreeSet<ShapeId>) -> bool {
    !crate::ir::is_standard_trait(id) && !internal_traits.contains(id)
}

/// Every IR node keeps a raw map of its traits. The projection
/// rewrites the typed fields, not those copies, so a removed vector, enum
/// value or resource could survive in them: an external view keeps only
/// the raw traits [`external_raw_trait`] allows, and is read through the
/// typed fields for the rest.
fn strip_raw_traits(service: &mut Service) {
    let internal_traits = std::mem::take(&mut service.internal_traits);
    let strip = |traits: &mut crate::ir::Traits| {
        traits.retain(|id, _| external_raw_trait(id, &internal_traits));
    };
    strip(&mut service.traits);
    for op in &mut service.operations {
        strip(&mut op.traits);
    }
    for resource in service.resources.values_mut() {
        strip(&mut resource.traits);
    }
    for shape in service.shapes.values_mut() {
        match shape {
            Shape::Unit { .. } => {}
            Shape::Simple(s) => strip(&mut s.traits),
            Shape::Enum(s) => {
                strip(&mut s.traits);
                s.values.iter_mut().for_each(|v| strip(&mut v.traits));
            }
            Shape::IntEnum(s) => {
                strip(&mut s.traits);
                s.values.iter_mut().for_each(|v| strip(&mut v.traits));
            }
            Shape::List(s) => {
                strip(&mut s.traits);
                strip(&mut s.member.traits);
            }
            Shape::Map(s) => {
                strip(&mut s.traits);
                strip(&mut s.key.traits);
                strip(&mut s.value.traits);
            }
            Shape::Structure(s) | Shape::Union(s) => {
                strip(&mut s.traits);
                s.members.iter_mut().for_each(|m| strip(&mut m.traits));
            }
        }
    }
}

fn project_example(
    shapes: &BTreeMap<ShapeId, Shape>,
    id: Option<&ShapeId>,
    value: &mut serde_json::Value,
) -> bool {
    match (id.and_then(|id| shapes.get(id)), value) {
        (
            Some(shape @ (Shape::Structure(s) | Shape::Union(s))),
            serde_json::Value::Object(values),
        ) => {
            values.retain(|name, _| s.member(name).is_some());
            let mut valid = !matches!(shape, Shape::Union(_)) || !values.is_empty();
            for (name, value) in values {
                valid &= project_example(shapes, s.member(name).map(|m| &m.target), value);
            }
            valid
        }
        (Some(Shape::List(s)), serde_json::Value::Array(values)) => values
            .iter_mut()
            .all(|value| project_example(shapes, Some(&s.member.target), value)),
        (Some(Shape::Map(s)), serde_json::Value::Object(values)) => values
            .values_mut()
            .all(|value| project_example(shapes, Some(&s.value.target), value)),
        (Some(Shape::Enum(s)), serde_json::Value::String(value)) => {
            s.values.iter().any(|v| v.value == *value)
        }
        (Some(Shape::IntEnum(s)), serde_json::Value::Number(value)) => s
            .values
            .iter()
            .any(|v| Some(i64::from(v.value)) == value.as_i64()),
        _ => true,
    }
}

fn removed_enum_default(service: &Service, member: &Member) -> bool {
    // Input/client-optional defaults are ignored by clients, so they do not
    // reference a generated enum variant in the projected contract.
    match (service.shape(&member.target), member.default.as_ref()) {
        (Some(Shape::Enum(shape)), Some(crate::ir::Value::String(value))) => shape
            .values
            .iter()
            .any(|variant| variant.internal && variant.value == *value),
        (Some(Shape::IntEnum(shape)), Some(crate::ir::Value::Number(value))) => shape
            .values
            .iter()
            .any(|variant| variant.internal && Some(i64::from(variant.value)) == value.as_i64()),
        _ => false,
    }
}

fn check_target(
    service: &Service,
    owner: &ShapeId,
    target: &ShapeId,
    diagnostics: &mut Diagnostics,
) {
    if service.shape(target).is_some_and(Shape::internal) {
        diagnostics.push(
            owner,
            format!("audience.internal_target: external contract targets internal shape {target}"),
        );
    }
}

/// Keeps the resource bindings whose paths still resolve from `root`, and
/// the collections whose members do, each relative to its elements.
fn retain_resource_bindings(
    shapes: &BTreeMap<ShapeId, Shape>,
    root: Option<&ShapeId>,
    bindings: &mut crate::ir::ResourceMemberBindings,
) {
    let resolves = |path: &[String]| {
        let mut target = root.cloned();
        for name in path {
            let next = target
                .as_ref()
                .and_then(|id| shapes.get(id))
                .and_then(|shape| {
                    shape
                        .members()
                        .into_iter()
                        .find(|m| &m.name == name)
                        .map(|m| m.target.clone())
                });
            let Some(next) = next else {
                return false;
            };
            target = Some(next);
        }
        true
    };
    bindings.identifiers.retain(|_, path| resolves(path));
    bindings.properties.retain(|_, path| resolves(path));
    bindings.collections.retain(|name, inner| {
        let element = root
            .and_then(|id| shapes.get(id))
            .and_then(|shape| {
                shape
                    .members()
                    .into_iter()
                    .find(|m| &m.name == name)
                    .map(|m| m.target.clone())
            })
            .and_then(|target| match shapes.get(&target) {
                Some(Shape::List(list)) => Some(list.member.target.clone()),
                Some(Shape::Map(map)) => Some(map.value.target.clone()),
                _ => None,
            });
        let Some(element) = element else {
            return false;
        };
        retain_resource_bindings(shapes, Some(&element), inner);
        !(inner.identifiers.is_empty() && inner.properties.is_empty())
    });
}

fn member_exists(shapes: &BTreeMap<ShapeId, Shape>, id: Option<&ShapeId>, name: &str) -> bool {
    id.and_then(|id| shapes.get(id))
        .is_some_and(|s| s.members().iter().any(|m| m.name == name))
}

fn retain_bindings(
    shapes: &BTreeMap<ShapeId, Shape>,
    id: Option<&ShapeId>,
    bindings: &mut Bindings,
) {
    let keep = |name: &str| member_exists(shapes, id, name);
    bindings.labels.retain(|name| keep(name));
    bindings.body.retain(|name| keep(name));
    bindings.query.retain(|b| keep(&b.member));
    bindings.headers.retain(|b| keep(&b.member));
    bindings.timestamp_formats.retain(|n, _| keep(n));
    bindings.header_encodings.retain(|n, _| keep(n));
    bindings.payload = bindings.payload.take().filter(|p| keep(&p.member));
}

fn check_path(
    service: &Service,
    op: &Operation,
    root: Option<&ShapeId>,
    path: &str,
    kind: &str,
    diagnostics: &mut Diagnostics,
) {
    let mut current = root.cloned();
    for name in path.split('.') {
        let Some(mut id) = current else {
            break;
        };
        // A path may cross a list: resolve the element type.
        if let Some(Shape::List(list)) = service.shape(&id) {
            id = list.member.target.clone();
        }
        let Some(member) = service
            .shape(&id)
            .and_then(|s| s.members().into_iter().find(|m| m.name == name))
        else {
            break;
        };
        if member.internal {
            diagnostics.push(
                &op.id,
                format!(
                    "audience.{kind}_reference: {kind} references removed member {}",
                    id.with_member(name)
                ),
            );
        }
        current = Some(member.target.clone());
    }
}

fn check_references(service: &Service, op: &Operation, diagnostics: &mut Diagnostics) {
    if let Some(p) = &op.paginated {
        for path in std::iter::once(&p.input_token).chain(&p.page_size) {
            check_path(
                service,
                op,
                op.input.as_ref(),
                path,
                "paginator",
                diagnostics,
            );
        }
        for path in std::iter::once(&p.output_token).chain(&p.items) {
            check_path(
                service,
                op,
                op.output.as_ref(),
                path,
                "paginator",
                diagnostics,
            );
        }
    }
}

fn project_permissions(result: &mut Service) {
    use crate::ir::{Mode, Requirement, Requirements, Tier};
    for resource in result.permissions.values_mut() {
        let internal: BTreeSet<String> = resource
            .permissions
            .iter()
            .filter(|p| p.tier == Tier::Internal)
            .map(|p| p.symbol.clone())
            .collect();
        resource.permissions.retain(|p| {
            p.tier != Tier::Internal
                && !p
                    .alias_of
                    .as_ref()
                    .is_some_and(|primary| internal.contains(primary))
        });
    }
    let permissions = result.permissions.clone();
    for op in &mut result.operations {
        // A pending reason is the server's migration note: the client
        // contract says nothing about the operation yet (§6.3).
        if matches!(op.requirements, Requirements::Pending { .. }) {
            op.requirements = Requirements::Undeclared;
        }
        if let Requirements::List(clauses) = &mut op.requirements {
            let visible = |clause: &Requirement| {
                permissions
                    .get(&clause.resource)
                    .is_some_and(|r| r.permission(&clause.permission).is_some())
            };
            // A `filters` clause narrows what a listing answers; it refuses
            // nothing, so leaving one out hides no requirement.
            let refusing = clauses
                .iter()
                .any(|clause| !visible(clause) && clause.mode != Mode::Filter);
            clauses.retain(visible);
            // Requirements the client may not see are still requirements:
            // an empty list would read as a model that lists none, and a
            // shorter one as everything the caller must hold.
            if refusing {
                op.requirements = if clauses.is_empty() {
                    Requirements::Internal
                } else {
                    Requirements::Partial(std::mem::take(clauses))
                };
            } else if clauses.is_empty() {
                // Only internal filters: no caller is refused for lacking a
                // permission, so to the client it requires none (what it
                // answers is narrowed, as any listing's may be). Not
                // `Undeclared`, which the shared check refuses.
                op.requirements = Requirements::None;
            }
        }
    }
    result.permissions.retain(|_, r| !r.permissions.is_empty());
    // `@permissionTypes` names its enum and the resources it excludes. A
    // resource whose permissions were all internal is gone from this view,
    // so it is no longer excluded either. The enum (still a wire enum here)
    // is this view's permission types only if both halves of check 6 still
    // hold: every value names a resource of the view, and every grantable
    // resource of the view that is not excluded has a value (one whose
    // value the projection removed as internal would have none).
    if let Some(types) = &mut result.permission_types {
        types
            .excluded
            .retain(|id| result.permissions.contains_key(id));
    }
    let names: BTreeSet<&str> = result
        .permissions
        .values()
        .map(|r| r.name.as_str())
        .collect();
    let consistent = result.permission_types.as_ref().is_some_and(|types| {
        let Some(Shape::Enum(e)) = result.shapes.get(&types.shape) else {
            return false;
        };
        let values: BTreeSet<&str> = e.values.iter().map(|v| v.value.as_str()).collect();
        values.iter().all(|value| names.contains(value))
            && result.permissions.iter().all(|(id, r)| {
                let grantable = r.permissions.iter().any(|p| {
                    p.alias_of.is_none() && matches!(p.tier, Tier::Public | Tier::PermissionOnly)
                });
                !grantable || types.excluded.contains(id) || values.contains(r.name.as_str())
            })
    });
    if !consistent {
        result.permission_types = None;
    }
}
