//! Each older release as the current model sees it (docs/api-versions-clients.md,
//! 5b; decisions 68-75 in docs/idl/decisions.md):
//!
//! ```text
//! projected(current) = current
//! projected(r_i)     = undo(migration r_i -> r_i+1, projected(r_i+1), frozen r_i)
//! ```
//!
//! [`undo`] turns the newer projection into the older one and takes from the
//! frozen release only what a step says the older release had, so additive
//! changes reach every release (decision 51) except inside a structure a
//! later step reshapes or an operation it removes (decision 74). What
//! lowering derived from shapes and operations is computed again on the
//! result (decision 73). Checking a projection against its frozen release,
//! and refusing the names a projection would reuse (decisions 68-70), is the
//! compiler's: it has the steps' spans.

use std::collections::{BTreeMap, BTreeSet};

use crate::ir::permissions::Binding as PermissionBinding;
use crate::ir::{
    ApiVersion, Bindings, ErrorInfo, Hook, Mode, Operation, Paginated, Requirements,
    ResourceMemberBindings, ResourcePermissions, Segment, Service, Shape, Uri, Value, VersionStep,
};
use crate::{Diagnostics, ShapeId};

/// One older release, projected.
#[derive(Debug, Clone)]
pub struct Projection {
    /// The release's name, which is its projection's `version`.
    pub name: String,
    /// The current model as seen at the release.
    pub service: Service,
    /// The examples the projection left out because they do not hold at
    /// the release, each against its operation. Not an error: the release
    /// loses an example, never a definition; the compiler reports each one
    /// as a warning at the example.
    pub warnings: Diagnostics,
    /// What undo restored from the frozen release, as
    /// [`ApiVersion::restored`] records it.
    pub restored: Vec<ShapeId>,
}

/// Every older release of `service`, the current model, oldest first, as
/// [`undo`] builds them newest first: the first from `service`, each next
/// from the one after it. Every projection passes
/// [`crate::validate::service`]. `Err` holds every problem of every
/// release: a step whose subject the newer projection no longer has, a
/// definition neither model has, an older requirement with no current
/// permission, and every problem the IR's own checks find in a projection.
pub fn projections(service: &Service) -> Result<Vec<Projection>, Diagnostics> {
    let mut errors = Diagnostics::default();
    let mut out: Vec<Projection> = Vec::with_capacity(service.versions.len());
    for version in service.versions.iter().rev() {
        let newer = out.last().map_or(service, |projection| &projection.service);
        let mut warnings = Diagnostics::default();
        let (projected, restored) = undo_restoring(newer, version, &mut errors, &mut warnings);
        let mut problems = Diagnostics::default();
        crate::validate::service(&projected, &mut problems);
        for problem in problems.iter() {
            errors.push(
                &problem.shape,
                format!(
                    "release \"{}\" as the current model sees it: {}",
                    version.name, problem.message
                ),
            );
        }
        out.push(Projection {
            name: version.name.clone(),
            service: projected,
            warnings,
            restored,
        });
    }
    out.reverse();
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

/// The release `version` as `newer` sees it: `newer` with the steps of the
/// migration from `version` to it undone, taking from `version.release`, the
/// frozen release, only what each step says the release had. `newer` is the
/// projection of the release after `version`, or the current model.
///
/// Steps compose; they do not run in order (decision 72). Each step names
/// what it changed as the newer model has it, so the whole definitions are
/// restored first (`remove operation`, `reshape`), then each member's
/// definition (`retype`), its requiredness (`require`) and its name
/// (`rename`); `add` and `move` change nothing the others read. A step on a
/// member of a structure restored whole is part of that definition already.
///
/// A restored definition keeps the projection's targets: each member
/// target resolves in the newer model when that id is there and is copied
/// from the frozen release when it is not, recursively. Operations the
/// release had keep its authentication; one added since keeps the newer
/// model's (decision 73). A problem goes to `errors` and the step is
/// skipped, so one run reports them all; an example that does not hold at
/// the release is left out and goes to `warnings`.
pub fn undo(
    newer: &Service,
    version: &ApiVersion,
    errors: &mut Diagnostics,
    warnings: &mut Diagnostics,
) -> Service {
    undo_restoring(newer, version, errors, warnings).0
}

/// [`undo`], with what it restored from the frozen release: the ids
/// [`ApiVersion::restored`] records, sorted. The reference dates and
/// documents each as the release had it (decision 79).
pub fn undo_restoring(
    newer: &Service,
    version: &ApiVersion,
    errors: &mut Diagnostics,
    warnings: &mut Diagnostics,
) -> (Service, Vec<ShapeId>) {
    let frozen: &Service = &version.release;
    let mut undo = Undo {
        frozen,
        frozen_operations: frozen
            .operations
            .iter()
            .map(|op| (op.id.clone(), op))
            .collect(),
        release: &version.name,
        out: without_versions(newer),
        restored: BTreeSet::new(),
        restored_members: BTreeSet::new(),
        changed: BTreeSet::new(),
        changed_operations: BTreeSet::new(),
        operations: BTreeSet::new(),
        reshaped: BTreeSet::new(),
        errors,
    };
    let steps = &version.steps;
    // Whole definitions first: what the other steps name inside them is
    // the frozen definition's already.
    for step in steps {
        match step {
            VersionStep::Hooked {
                hook: Hook::RemoveOperation { operation },
                ..
            } => undo.restore_operation(operation),
            VersionStep::Hooked {
                hook: Hook::Reshape { old, new, .. },
                ..
            } => undo.restore_shape(old, new),
            _ => {}
        }
    }
    for step in steps {
        match step {
            VersionStep::AddEnumValue {
                new,
                value,
                downgrade,
                ..
            } => undo.remove_value(new, value, downgrade),
            VersionStep::AddError {
                operation, error, ..
            } => undo.remove_error(operation, error),
            VersionStep::Move {
                operation,
                old_method,
                old_uri,
                ..
            } => undo.restore_route(operation, old_method, old_uri),
            _ => {}
        }
    }
    for step in steps {
        if let VersionStep::Hooked {
            hook:
                Hook::Retype {
                    old,
                    new,
                    member,
                    from,
                    ..
                },
            ..
        } = step
        {
            undo.restore_member(old, new, member, older_name(member, from));
        }
    }
    for step in steps {
        match step {
            VersionStep::Require {
                old,
                new,
                member,
                from,
                ..
            }
            | VersionStep::Hooked {
                hook:
                    Hook::Require {
                        old,
                        new,
                        member,
                        from,
                    },
                ..
            } => undo.restore_requiredness(old, new, member, older_name(member, from)),
            _ => {}
        }
    }
    for step in steps {
        if let VersionStep::Rename {
            old, new, from, to, ..
        } = step
        {
            undo.rename(old, new, from, to);
        }
    }
    undo.finish(newer, warnings)
}

/// A member's name in the older release: `from` when the migration renamed
/// it, else the name it has now.
fn older_name<'a>(member: &'a str, from: &'a str) -> &'a str {
    if from.is_empty() { member } else { from }
}

/// A copy of `service` without its older releases: a projection carries no
/// versions of its own, and the current model's are not worth cloning.
fn without_versions(service: &Service) -> Service {
    Service {
        internal: service.internal,
        recursive_groups: service.recursive_groups.clone(),
        protocol: service.protocol,
        errors: service.errors.clone(),
        protocol_settings: service.protocol_settings.clone(),
        requirements: service.requirements.clone(),
        id: service.id.clone(),
        version: service.version.clone(),
        title: service.title.clone(),
        docs: service.docs.clone(),
        notes: service.notes.clone(),
        auth: service.auth.clone(),
        auth_schemes: service.auth_schemes.clone(),
        operations: service.operations.clone(),
        resources: service.resources.clone(),
        resource_roots: service.resource_roots.clone(),
        resource_references: service.resource_references.clone(),
        shapes: service.shapes.clone(),
        traits: service.traits.clone(),
        versions: Vec::new(),
        api_version: service.api_version.clone(),
        internal_traits: service.internal_traits.clone(),
        permissions: service.permissions.clone(),
        permission_types: service.permission_types.clone(),
        not_found: service.not_found.clone(),
        exceptions: service.exceptions.clone(),
        doc_links: service.doc_links.clone(),
        doc_link_operations: service.doc_link_operations.clone(),
    }
}

struct Undo<'a> {
    frozen: &'a Service,
    /// The release's operations, by id.
    frozen_operations: BTreeMap<ShapeId, &'a Operation>,
    release: &'a str,
    /// The projection being built: a copy of the newer model, then undone.
    out: Service,
    /// Shapes and operations whose definition is the frozen release's.
    restored: BTreeSet<ShapeId>,
    /// Members a `retype` restored whole, as the projection names them.
    restored_members: BTreeSet<ShapeId>,
    /// Shapes whose definition undo changed: an operation that reaches one
    /// takes the release's examples.
    changed: BTreeSet<ShapeId>,
    /// Operations whose own facts undo changed: their errors or route.
    changed_operations: BTreeSet<ShapeId>,
    /// The operations `remove operation` restored.
    operations: BTreeSet<ShapeId>,
    /// The newer ids of definitions a reshape restored under another id.
    reshaped: BTreeSet<ShapeId>,
    errors: &'a mut Diagnostics,
}

impl Undo<'_> {
    fn problem(&mut self, id: &ShapeId, message: String) {
        self.errors.push(
            id,
            format!(
                "release \"{}\" cannot be projected: {message}",
                self.release
            ),
        );
    }

    /// Whether a step on `shape` is part of a definition restored whole:
    /// one restored under `shape`, or a reshape's under its older id, which
    /// the newer model's steps name by its newer one.
    fn whole(&self, shape: &ShapeId) -> bool {
        self.restored.contains(shape) || self.reshaped.contains(shape)
    }

    /// `remove operation Op`: the release's operation, with its own input
    /// and output as the release has them, the errors only it used, and its
    /// requirements mapped to today's permissions.
    fn restore_operation(&mut self, id: &ShapeId) {
        let Some(older) = self.frozen_operations.get(id).copied() else {
            self.problem(
                id,
                format!(
                    "`remove operation {}` names an operation the release does not have",
                    id.name()
                ),
            );
            return;
        };
        if self.out.operations.iter().any(|op| &op.id == id) {
            self.problem(
                id,
                format!(
                    "`remove operation {}` restores an operation the newer model has again; the projection would hold two",
                    id.name()
                ),
            );
            return;
        }
        let mut operation = older.clone();
        self.map_requirements(&mut operation);
        // Its own shapes are the release's whole (decision 74): nothing
        // added since has a place in them.
        for own in older.input.iter().chain(&older.output) {
            self.copy_shape(own);
        }
        for error in &older.errors {
            if !self.out.shapes.contains_key(error) {
                self.copy_shape(error);
            }
        }
        self.restored.insert(id.clone());
        self.operations.insert(id.clone());
        self.out.operations.push(operation);
    }

    /// A restored operation's requirements, which a legacy handler checks on
    /// the release's input, name the current permissions (as
    /// docs/permissions.md §7.3 has them).
    fn map_requirements(&mut self, operation: &mut Operation) {
        let id = operation.id.clone();
        let (Requirements::List(clauses) | Requirements::Partial(clauses)) =
            &mut operation.requirements
        else {
            return;
        };
        let mut problems = Vec::new();
        for clause in clauses.iter_mut() {
            match current_permission(&self.out.permissions, &self.frozen.permissions, clause) {
                Ok((resource, symbol)) => {
                    clause.resource = resource;
                    clause.permission = symbol;
                }
                Err(problem) => problems.push(problem),
            }
        }
        for problem in problems {
            self.problem(
                &id,
                format!(
                    "`remove operation {}` restores an operation that requires {problem}",
                    id.name()
                ),
            );
        }
    }

    /// `reshape S`: the release's definition of `S`.
    fn restore_shape(&mut self, old: &ShapeId, new: &ShapeId) {
        if !self.frozen.shapes.contains_key(old) {
            self.problem(
                old,
                format!(
                    "`reshape {}` names a shape the release does not have",
                    old.name()
                ),
            );
            return;
        }
        if !self.out.shapes.contains_key(new) {
            self.problem(
                new,
                format!(
                    "`reshape {}` names a shape the newer model no longer has",
                    new.name()
                ),
            );
            return;
        }
        if old != new {
            self.redirect(new, old);
            self.reshaped.insert(new.clone());
        }
        self.copy_shape(old);
    }

    /// Points what names `from` at `to` instead: a reshaped definition
    /// the two releases give different ids.
    fn redirect(&mut self, from: &ShapeId, to: &ShapeId) {
        for operation in &mut self.out.operations {
            for slot in [&mut operation.input, &mut operation.output] {
                if slot.as_ref() == Some(from) {
                    *slot = Some(to.clone());
                }
            }
        }
        for shape in self.out.shapes.values_mut() {
            for member in members_mut(shape) {
                if &member.target == from {
                    member.target = to.clone();
                }
            }
        }
        for target in self.out.doc_links.values_mut() {
            if &target.root() == from {
                *target = match target.member() {
                    Some(member) => to.with_member(member),
                    None => to.clone(),
                };
            }
        }
    }

    /// Copies the release's `id` into the projection, over what the newer
    /// model has there, then each target it reaches that the projection
    /// lacks.
    fn copy_shape(&mut self, id: &ShapeId) {
        let frozen = self.frozen;
        let Some(shape) = frozen.shapes.get(id) else {
            self.problem(
                id,
                format!("the release does not define {id}, which a restored definition names"),
            );
            return;
        };
        let targets: Vec<ShapeId> = shape.members().iter().map(|m| m.target.clone()).collect();
        self.out.shapes.insert(id.clone(), shape.clone());
        self.restored.insert(id.clone());
        self.changed.insert(id.clone());
        for target in targets {
            self.resolve(&target);
        }
    }

    /// A target of a restored definition: the projection's when it has
    /// the id, else the release's, copied with what it reaches.
    fn resolve(&mut self, target: &ShapeId) {
        if !self.out.shapes.contains_key(target) {
            self.copy_shape(target);
        }
    }

    /// `add Enum.V downgrade W`: `V` is not in the release. A default that
    /// names `V`, on a member added since, names `W`, as the release's
    /// clients receive it.
    fn remove_value(&mut self, enumeration: &ShapeId, value: &str, downgrade: &str) {
        // The wire values of `V` and `W`, as defaults hold them.
        let wires = match self.out.shapes.get_mut(enumeration) {
            Some(Shape::Enum(e)) => {
                let wire = |name: &str| {
                    e.values
                        .iter()
                        .find(|v| v.name == name)
                        .map(|v| Value::String(v.value.clone()))
                };
                let wires = (wire(value), wire(downgrade));
                e.values.retain(|v| v.name != value);
                wires
            }
            Some(Shape::IntEnum(e)) => {
                let wire = |name: &str| {
                    e.values
                        .iter()
                        .find(|v| v.name == name)
                        .map(|v| Value::from(v.value))
                };
                let wires = (wire(value), wire(downgrade));
                e.values.retain(|v| v.name != value);
                wires
            }
            _ => (None, None),
        };
        let (Some(added), downgraded) = wires else {
            self.problem(
                &enumeration.with_member(value),
                format!(
                    "`add {}.{value}` names a value the newer model no longer has",
                    enumeration.name()
                ),
            );
            return;
        };
        self.changed.insert(enumeration.clone());
        let mut stranded = false;
        for shape in self.out.shapes.values_mut() {
            for member in members_mut(shape) {
                if &member.target != enumeration {
                    continue;
                }
                for slot in [&mut member.default, &mut member.wire_default] {
                    if slot.as_ref() == Some(&added) {
                        match &downgraded {
                            Some(wire) => *slot = Some(wire.clone()),
                            None => stranded = true,
                        }
                    }
                }
            }
        }
        if stranded {
            self.problem(
                &enumeration.with_member(downgrade),
                format!(
                    "`add {}.{value} downgrade {downgrade}`: a default names `{value}`, and the newer model no longer has `{downgrade}`",
                    enumeration.name()
                ),
            );
        }
    }

    /// `add Op.errors E downgrade F`: `E` is not among the release's errors
    /// of `Op`; the shapes recomputed later drop it if nothing else reaches it.
    fn remove_error(&mut self, operation: &ShapeId, error: &ShapeId) {
        let Some(op) = self
            .out
            .operations
            .iter_mut()
            .find(|op| &op.id == operation)
        else {
            self.problem(
                operation,
                format!(
                    "`add {}.errors {}` names an operation the newer model no longer has",
                    operation.name(),
                    error.name()
                ),
            );
            return;
        };
        let before = op.errors.len();
        op.errors.retain(|id| id != error);
        if op.errors.len() == before {
            self.problem(
                operation,
                format!(
                    "`add {}.errors {}` names an error the operation no longer has",
                    operation.name(),
                    error.name()
                ),
            );
            return;
        }
        self.changed_operations.insert(operation.clone());
    }

    /// `move Op from M "/old"`: the release's route.
    fn restore_route(&mut self, operation: &ShapeId, method: &str, uri: &Uri) {
        let Some(op) = self
            .out
            .operations
            .iter_mut()
            .find(|op| &op.id == operation)
        else {
            self.problem(
                operation,
                format!(
                    "`move {}` names an operation the newer model no longer has",
                    operation.name()
                ),
            );
            return;
        };
        op.method = method.to_owned();
        op.uri = uri.clone();
        self.changed_operations.insert(operation.clone());
    }

    /// `retype S.m -> T`: the release's member whole (target, constraints,
    /// timestamp format, default, requiredness, documentation), at its
    /// current name and position, with the release's timestamp formats and
    /// header encodings for it.
    fn restore_member(&mut self, old: &ShapeId, new: &ShapeId, member: &str, older: &str) {
        if self.whole(new) {
            return;
        }
        let Some(frozen) = structure_member(self.frozen, old, older).cloned() else {
            self.problem(
                &old.with_member(older),
                format!(
                    "`retype {}.{member}` names a member the release does not have",
                    new.name()
                ),
            );
            return;
        };
        let Some(slot) = structure_member_mut(&mut self.out, new, member) else {
            self.problem(
                &new.with_member(member),
                format!(
                    "`retype {}.{member}` names a member the newer model no longer has",
                    new.name()
                ),
            );
            return;
        };
        let target = frozen.target.clone();
        *slot = frozen;
        slot.name = member.to_owned();
        self.changed.insert(new.clone());
        self.restored_members.insert(new.with_member(member));
        self.resolve(&target);
        // Where the member travels is the release's too, whole: a body
        // member, a header or a payload, with its kind, media type and
        // encodings. A release with no binding to take from leaves it as it
        // is: nothing in the IR could derive one.
        let order: Vec<String> = match self.out.shapes.get(new) {
            Some(Shape::Structure(s) | Shape::Union(s)) => {
                s.members.iter().map(|m| m.name.clone()).collect()
            }
            _ => Vec::new(),
        };
        let frozen_operations = &self.frozen_operations;
        for operation in &mut self.out.operations {
            let Some(older_operation) = frozen_operations.get(&operation.id).copied() else {
                continue;
            };
            if operation.input.as_ref() == Some(new) {
                restore_location(
                    &mut operation.request,
                    &older_operation.request,
                    member,
                    older,
                    &order,
                );
            }
            if operation.output.as_ref() == Some(new) {
                restore_location(
                    &mut operation.response,
                    &older_operation.response,
                    member,
                    older,
                    &order,
                );
            }
        }
        let older_error = error_info(self.frozen, old);
        if let Some(Shape::Structure(s)) = self.out.shapes.get_mut(new)
            && let Some(error) = &mut s.error
            && let Some(then) = older_error
        {
            place(
                &mut error.headers,
                then.headers
                    .iter()
                    .find(|b| b.member == older)
                    .map(|b| crate::ir::Binding {
                        member: member.to_owned(),
                        name: b.name.clone(),
                    }),
                |b| &b.member,
                member,
                &order,
            );
            if error.payload.as_ref().is_some_and(|p| p.member == member)
                || then.payload.as_ref().is_some_and(|p| p.member == older)
            {
                error.payload = renamed_payload(then.payload.as_ref(), member, older);
            }
            restore_entry(
                &mut error.timestamp_formats,
                Some(&then.timestamp_formats),
                member,
                older,
            );
            restore_entry(
                &mut error.header_encodings,
                Some(&then.header_encodings),
                member,
                older,
            );
        }
    }

    /// `require S.m`: the release's requiredness and default for the member.
    fn restore_requiredness(&mut self, old: &ShapeId, new: &ShapeId, member: &str, older: &str) {
        if self.whole(new) {
            return;
        }
        let Some(frozen) = structure_member(self.frozen, old, older).cloned() else {
            self.problem(
                &old.with_member(older),
                format!(
                    "`require {}.{member}` names a member the release does not have",
                    new.name()
                ),
            );
            return;
        };
        let Some(slot) = structure_member_mut(&mut self.out, new, member) else {
            self.problem(
                &new.with_member(member),
                format!(
                    "`require {}.{member}` names a member the newer model no longer has",
                    new.name()
                ),
            );
            return;
        };
        slot.required = frozen.required;
        slot.default = frozen.default;
        slot.wire_default = frozen.wire_default;
        self.changed.insert(new.clone());
    }

    /// `rename S.a -> b`: the member, or enum value, is `a` again, with the
    /// release's bindings for it, and every path through it says `a`.
    fn rename(&mut self, old: &ShapeId, new: &ShapeId, from: &str, to: &str) {
        if self.whole(new) {
            return;
        }
        let (present, taken) = match self.out.shapes.get(new) {
            Some(Shape::Structure(s) | Shape::Union(s)) => {
                (s.member(to).is_some(), s.member(from).is_some())
            }
            Some(Shape::Enum(e)) => (
                e.values.iter().any(|v| v.name == to),
                e.values.iter().any(|v| v.name == from),
            ),
            Some(Shape::IntEnum(e)) => (
                e.values.iter().any(|v| v.name == to),
                e.values.iter().any(|v| v.name == from),
            ),
            _ => (false, false),
        };
        if taken {
            self.problem(
                &new.with_member(from),
                format!(
                    "`rename {}.{from} -> {to}` restores `{from}`, which the newer model has again",
                    new.name()
                ),
            );
            return;
        }
        if !present {
            self.problem(
                &new.with_member(to),
                format!(
                    "`rename {}.{from} -> {to}` names a member the newer model no longer has",
                    new.name()
                ),
            );
            return;
        }
        if !super::has(self.frozen, &old.with_member(from)) {
            self.problem(
                &old.with_member(from),
                format!(
                    "`rename {}.{from} -> {to}` names a member the release does not have",
                    new.name()
                ),
            );
            return;
        }
        if matches!(
            self.out.shapes.get(new),
            Some(Shape::Enum(_) | Shape::IntEnum(_))
        ) {
            self.rename_value(old, new, from, to);
        } else {
            self.rename_member(old, new, from, to);
        }
        self.changed.insert(new.clone());
        let (renamed, restored) = (new.with_member(to), new.with_member(from));
        if self.restored_members.remove(&renamed) {
            self.restored_members.insert(restored.clone());
        }
        for usage in &mut self.out.requirements {
            if usage.shape == renamed {
                usage.shape = restored.clone();
            }
        }
        for target in self.out.doc_links.values_mut() {
            if *target == renamed {
                *target = restored.clone();
            }
        }
    }

    /// An enum value renamed back, with the release's wire value; a default
    /// that names its newer value names the older one.
    fn rename_value(&mut self, old: &ShapeId, new: &ShapeId, from: &str, to: &str) {
        let older_wire = match self.frozen.shapes.get(old) {
            Some(Shape::Enum(e)) => e
                .values
                .iter()
                .find(|v| v.name == from)
                .map(|v| v.value.clone()),
            _ => None,
        };
        let mut newer_wire = None;
        match self.out.shapes.get_mut(new) {
            Some(Shape::Enum(e)) => {
                if let Some(value) = e.values.iter_mut().find(|v| v.name == to) {
                    value.name = from.to_owned();
                    if let Some(wire) = &older_wire {
                        newer_wire = Some(std::mem::replace(&mut value.value, wire.clone()));
                    }
                }
            }
            Some(Shape::IntEnum(e)) => {
                if let Some(value) = e.values.iter_mut().find(|v| v.name == to) {
                    value.name = from.to_owned();
                }
            }
            _ => {}
        }
        let (Some(newer_wire), Some(older_wire)) = (newer_wire, older_wire) else {
            return;
        };
        if newer_wire == older_wire {
            return;
        }
        let (newer_wire, older_wire) = (Value::String(newer_wire), Value::String(older_wire));
        for shape in self.out.shapes.values_mut() {
            for member in members_mut(shape) {
                if &member.target != new {
                    continue;
                }
                for slot in [&mut member.default, &mut member.wire_default] {
                    if slot.as_ref() == Some(&newer_wire) {
                        *slot = Some(older_wire.clone());
                    }
                }
            }
        }
    }

    /// A structure or union member renamed back: its bindings, its route
    /// label, and the pagination, idempotency, signature, permission and
    /// resource paths through it. A permission binding or `when` condition
    /// names the input as the release's clients send it, which is what the
    /// guard compares and the reference shows; the server authorizes a
    /// converted request by the current model's own requirements.
    fn rename_member(&mut self, old: &ShapeId, new: &ShapeId, from: &str, to: &str) {
        let frozen = self.frozen;
        // Paths are walked before the member is renamed, so each segment
        // still resolves; the operations and resources are taken out of the
        // model meanwhile, so its shapes can be read as they change.
        let mut operations = std::mem::take(&mut self.out.operations);
        let mut resources = std::mem::take(&mut self.out.resources);
        let shapes = &self.out.shapes;
        let rename = |root: Option<&ShapeId>, path: &mut Vec<String>| {
            rename_segments(shapes, root, path, new, to, from);
        };
        for operation in &mut operations {
            let input = operation.input.clone();
            let output = operation.output.clone();
            if let Some(p) = &mut operation.paginated {
                p.input_token = renamed_path(shapes, input.as_ref(), &p.input_token, new, to, from);
                p.output_token =
                    renamed_path(shapes, output.as_ref(), &p.output_token, new, to, from);
                if let Some(items) = &mut p.items {
                    *items = renamed_path(shapes, output.as_ref(), items, new, to, from);
                }
                if let Some(size) = &mut p.page_size {
                    *size = renamed_path(shapes, input.as_ref(), size, new, to, from);
                }
            }
            if let Requirements::List(clauses) | Requirements::Partial(clauses) =
                &mut operation.requirements
            {
                for clause in clauses {
                    for binding in &mut clause.bindings {
                        if let PermissionBinding::Member(path) = binding {
                            rename(input.as_ref(), path);
                        }
                    }
                    if let Some(when) = &mut clause.when {
                        for path in when.paths_mut() {
                            rename(input.as_ref(), path);
                        }
                    }
                }
            }
            for resource in resources.values_mut() {
                if let Some(bindings) = resource.bindings.get_mut(&operation.id) {
                    rename_resource_paths(
                        shapes,
                        input.as_ref(),
                        &mut bindings.input,
                        new,
                        to,
                        from,
                    );
                    rename_resource_paths(
                        shapes,
                        output.as_ref(),
                        &mut bindings.output,
                        new,
                        to,
                        from,
                    );
                }
            }
            let older = self.frozen_operations.get(&operation.id).copied();
            if input.as_ref() == Some(new) {
                rename_bindings(
                    &mut operation.request,
                    older.map(|op| &op.request),
                    to,
                    from,
                );
                rename_labels(&mut operation.uri, to, from);
                if operation.idempotency_token.as_deref() == Some(to) {
                    operation.idempotency_token = Some(from.to_owned());
                }
                if let Some(sender) = &mut operation.sender
                    && sender.signature == to
                {
                    sender.signature = from.to_owned();
                }
            }
            if output.as_ref() == Some(new) {
                rename_bindings(
                    &mut operation.response,
                    older.map(|op| &op.response),
                    to,
                    from,
                );
            }
        }
        self.out.operations = operations;
        self.out.resources = resources;
        let older_error = error_info(frozen, old);
        if let Some(Shape::Structure(s) | Shape::Union(s)) = self.out.shapes.get_mut(new) {
            if let Some(member) = s.members.iter_mut().find(|m| m.name == to) {
                member.name = from.to_owned();
            }
            if let Some(error) = &mut s.error {
                rename_error(error, older_error, to, from);
            }
        }
    }

    /// What lowering derived, computed again for the projection, and what
    /// the projection holds that undo restored.
    fn finish(mut self, newer: &Service, warnings: &mut Diagnostics) -> (Service, Vec<ShapeId>) {
        let frozen = self.frozen;
        self.authentication(newer);
        // Where restored input and output structures travel is the
        // release's: their bindings, route, tokens and pagination.
        for operation in &mut self.out.operations {
            if self.operations.contains(&operation.id) {
                continue;
            }
            let Some(older) = self.frozen_operations.get(&operation.id).copied() else {
                continue;
            };
            let input = operation
                .input
                .as_ref()
                .is_some_and(|id| self.restored.contains(id));
            let output = operation
                .output
                .as_ref()
                .is_some_and(|id| self.restored.contains(id));
            if input {
                operation.request = older.request.clone();
                operation.uri = older.uri.clone();
                operation.idempotency_token = older.idempotency_token.clone();
                operation.sender = older.sender.clone();
            }
            if output {
                operation.response = older.response.clone();
            }
            if input || output {
                operation.paginated = match (&operation.paginated, &older.paginated) {
                    (Some(now), Some(then)) => {
                        let inputs = if input { then } else { now };
                        let outputs = if output { then } else { now };
                        Some(Paginated {
                            input_token: inputs.input_token.clone(),
                            page_size: inputs.page_size.clone(),
                            output_token: outputs.output_token.clone(),
                            items: outputs.items.clone(),
                        })
                    }
                    _ => older.paginated.clone(),
                };
            }
        }
        self.payload_bound();
        // The protocol's fixed headers follow from the bindings, which undo
        // may have restored: a body the release had, or did not.
        let protocol = self.out.protocol;
        for operation in &mut self.out.operations {
            operation.protocol_headers = protocol.request_headers(&operation.request);
            let has_value =
                protocol.response_has_value(operation.output.as_ref(), &operation.response);
            operation.response_protocol_headers =
                protocol.response_headers(&operation.response, has_value);
        }
        self.resources();
        self.examples(warnings);

        // Shapes: what the operations and errors reach, restored shapes
        // included, and whatever the newer model holds that nothing reaches,
        // as lowering left it.
        let extras: BTreeSet<ShapeId> = {
            let reached = reached(newer);
            newer
                .shapes
                .keys()
                .filter(|id| !reached.contains(*id))
                .cloned()
                .collect()
        };
        let reached = reached(&self.out);
        self.out
            .shapes
            .retain(|id, _| reached.contains(id) || extras.contains(id));
        self.out.recursive_groups = crate::ir::recursive_groups(&self.out.shapes);
        self.feature_uses();
        let present: BTreeSet<ShapeId> = self.out.shapes.keys().cloned().collect();
        self.out
            .resource_references
            .retain(|id, _| present.contains(id));
        // A restored definition's documentation links to what the release
        // linked it to, where the newer model has no label of its own or its
        // label's target is gone from the projection (a definition restored
        // under its older id).
        for (label, target) in &frozen.doc_links {
            if self.out.doc_link(label).is_some() {
                continue;
            }
            self.out.doc_links.insert(label.clone(), target.clone());
            match frozen.doc_link_operations.get(label) {
                Some(operation) => {
                    self.out
                        .doc_link_operations
                        .insert(label.clone(), operation.clone());
                }
                None => {
                    self.out.doc_link_operations.remove(label);
                }
            }
        }
        self.out.retain_doc_links();
        self.out
            .operations
            .sort_by(|a, b| a.id.name().cmp(b.id.name()));
        self.out.version = self.release.to_owned();
        self.out.api_version = newer.api_version.as_ref().map(|_| self.release.to_owned());
        // What undo took from the release and the projection still holds:
        // a shape nothing reaches any more was left out above.
        let out = &self.out;
        let restored = self
            .restored
            .iter()
            .chain(&self.restored_members)
            .filter(|id| super::has(out, id))
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        (self.out, restored)
    }

    /// Decision 74, for a payload: where the release sent a message's
    /// payload and the projection sends that payload again, the message has
    /// no body, so a member added since that would travel in it does not
    /// reach the release. Its older clients never sent it, and the release's
    /// message stays valid. Members bound to a label, query parameter or
    /// header still reach it, as any addition does; so does a member the
    /// release had.
    fn payload_bound(&mut self) {
        let frozen = self.frozen;
        // (message structure, the release's definition of it) per payload.
        let mut messages: Vec<(ShapeId, ShapeId)> = Vec::new();
        for operation in &mut self.out.operations {
            if self.operations.contains(&operation.id) {
                continue;
            }
            let Some(older) = self.frozen_operations.get(&operation.id).copied() else {
                continue;
            };
            for (shape, now, then, older_shape) in [
                (
                    &operation.input,
                    &mut operation.request,
                    &older.request,
                    &older.input,
                ),
                (
                    &operation.output,
                    &mut operation.response,
                    &older.response,
                    &older.output,
                ),
            ] {
                let (Some(shape), Some(older_shape)) = (shape, older_shape) else {
                    continue;
                };
                if now.payload.is_none() || then.payload.is_none() || now.body.is_empty() {
                    continue;
                }
                let Some(Shape::Structure(release)) = frozen.shapes.get(older_shape) else {
                    continue;
                };
                let dropped: Vec<String> = now
                    .body
                    .iter()
                    .filter(|member| release.member(member).is_none())
                    .cloned()
                    .collect();
                now.body.retain(|member| !dropped.contains(member));
                now.timestamp_formats
                    .retain(|member, _| !dropped.contains(member));
                now.header_encodings
                    .retain(|member, _| !dropped.contains(member));
                messages.push((shape.clone(), older_shape.clone()));
            }
        }
        // An error's body is every member it binds to no header.
        for (id, shape) in &mut self.out.shapes {
            let Shape::Structure(s) = shape else {
                continue;
            };
            let (Some(error), Some(Shape::Structure(release))) = (&s.error, frozen.shapes.get(id))
            else {
                continue;
            };
            let sent = release
                .error
                .as_ref()
                .is_some_and(|then| then.payload.is_some());
            if !sent || error.payload.is_none() {
                continue;
            }
            let headers: BTreeSet<&str> = error.headers.iter().map(|b| b.member.as_str()).collect();
            let payload = error.payload.as_ref().map(|p| p.member.clone());
            let before = s.members.len();
            s.members.retain(|m| {
                release.member(&m.name).is_some()
                    || headers.contains(m.name.as_str())
                    || payload.as_deref() == Some(m.name.as_str())
            });
            if s.members.len() != before {
                let kept: BTreeSet<&str> = s.members.iter().map(|m| m.name.as_str()).collect();
                if let Some(error) = &mut s.error {
                    error
                        .timestamp_formats
                        .retain(|member, _| kept.contains(member.as_str()));
                    error
                        .header_encodings
                        .retain(|member, _| kept.contains(member.as_str()));
                }
                self.changed.insert(id.clone());
            }
        }
        for (shape, older_shape) in messages {
            let operation_bindings: Vec<&Bindings> = self
                .out
                .operations
                .iter()
                .flat_map(|op| {
                    [
                        (op.input.as_ref() == Some(&shape)).then_some(&op.request),
                        (op.output.as_ref() == Some(&shape)).then_some(&op.response),
                    ]
                })
                .flatten()
                .collect();
            let bound: BTreeSet<String> = operation_bindings
                .iter()
                .flat_map(|b| {
                    b.labels
                        .iter()
                        .chain(&b.body)
                        .chain(b.query.iter().map(|q| &q.member))
                        .chain(b.headers.iter().map(|h| &h.member))
                        .chain(b.payload.as_ref().map(|p| &p.member))
                })
                .cloned()
                .collect();
            let Some(Shape::Structure(release)) = frozen.shapes.get(&older_shape) else {
                continue;
            };
            if let Some(Shape::Structure(s)) = self.out.shapes.get_mut(&shape) {
                let before = s.members.len();
                s.members
                    .retain(|m| release.member(&m.name).is_some() || bound.contains(&m.name));
                if s.members.len() != before {
                    self.changed.insert(shape.clone());
                }
            }
        }
    }

    /// An operation the release has authenticates as the release declared
    /// it; one added since as the newer model does. The service declares
    /// every scheme an operation names.
    fn authentication(&mut self, newer: &Service) {
        let frozen = self.frozen;
        let mut released = BTreeSet::new();
        for operation in &mut self.out.operations {
            if let Some(older) = self.frozen_operations.get(&operation.id) {
                operation.auth = older.auth.clone();
                operation.public = older.public;
                released.extend(older.auth.iter().cloned());
            }
        }
        let mut named: Vec<ShapeId> = newer.auth.clone();
        for scheme in self.out.operations.iter().flat_map(|op| &op.auth) {
            if !named.contains(scheme) {
                named.push(scheme.clone());
            }
        }
        // A scheme the release's operations name is the release's; one
        // only operations added since name is the newer model's.
        let mut schemes = BTreeMap::new();
        for scheme in &named {
            let (first, second) = if released.contains(scheme) {
                (&frozen.auth_schemes, &newer.auth_schemes)
            } else {
                (&newer.auth_schemes, &frozen.auth_schemes)
            };
            if let Some(definition) = first.get(scheme).or_else(|| second.get(scheme)) {
                schemes.insert(scheme.clone(), definition.clone());
            }
        }
        self.out.auth = named;
        self.out.auth_schemes = schemes;
    }

    /// Resources keep the operations that remain; a restored operation
    /// rejoins its resource as the release bound it; an operation whose
    /// input or output undo changed binds as the release did.
    fn resources(&mut self) {
        let frozen = self.frozen;
        let operations: BTreeSet<ShapeId> =
            self.out.operations.iter().map(|op| op.id.clone()).collect();
        for resource in self.out.resources.values_mut() {
            for slot in lifecycle_mut(&mut resource.lifecycle) {
                if slot.as_ref().is_some_and(|id| !operations.contains(id)) {
                    *slot = None;
                }
            }
            resource.operations.retain(|id| operations.contains(id));
            resource
                .collection_operations
                .retain(|id| operations.contains(id));
            resource.bindings.retain(|id, _| operations.contains(id));
        }
        for operation in &mut self.out.operations {
            let restored = self.operations.contains(&operation.id);
            let changed = operation
                .input
                .iter()
                .chain(&operation.output)
                .any(|id| self.changed.contains(id));
            if !restored && !changed {
                continue;
            }
            if restored {
                let bound = operation
                    .resource
                    .as_ref()
                    .is_some_and(|id| self.out.resources.contains_key(id));
                if !bound {
                    operation.resource = None;
                }
            }
            for (id, resource) in &mut self.out.resources {
                let Some(older) = frozen.resources.get(id) else {
                    continue;
                };
                if restored {
                    insert_in_order(&mut resource.operations, &older.operations, &operation.id);
                    insert_in_order(
                        &mut resource.collection_operations,
                        &older.collection_operations,
                        &operation.id,
                    );
                    let mut was = older.lifecycle.clone();
                    for (slot, then) in lifecycle_mut(&mut resource.lifecycle)
                        .into_iter()
                        .zip(lifecycle_mut(&mut was))
                    {
                        if slot.is_none() && then.as_ref() == Some(&operation.id) {
                            *slot = then.take();
                        }
                    }
                }
                // A converted operation keeps the resources it binds now,
                // through the release's paths.
                if let Some(bindings) = older.bindings.get(&operation.id)
                    && (restored || resource.bindings.contains_key(&operation.id))
                {
                    resource
                        .bindings
                        .insert(operation.id.clone(), bindings.clone());
                }
            }
        }
        let roots: BTreeMap<ShapeId, (Option<ShapeId>, Option<ShapeId>)> = self
            .out
            .operations
            .iter()
            .map(|op| (op.id.clone(), (op.input.clone(), op.output.clone())))
            .collect();
        for resource in self.out.resources.values_mut() {
            for (id, bindings) in &mut resource.bindings {
                let Some((input, output)) = roots.get(id) else {
                    continue;
                };
                crate::audience::retain_resource_bindings(
                    &self.out.shapes,
                    input.as_ref(),
                    &mut bindings.input,
                );
                crate::audience::retain_resource_bindings(
                    &self.out.shapes,
                    output.as_ref(),
                    &mut bindings.output,
                );
            }
        }
    }

    /// An operation's examples: the release's when undo changed what it
    /// reaches, its own otherwise. One that does not hold at the release is
    /// left out, with a warning.
    fn examples(&mut self, warnings: &mut Diagnostics) {
        let changed = &self.changed;
        let shapes = &self.out.shapes;
        let mut examples = Vec::with_capacity(self.out.operations.len());
        for operation in &self.out.operations {
            let older = self.frozen_operations.get(&operation.id);
            let reaches_change = || {
                self.changed_operations.contains(&operation.id)
                    || reaches(shapes, operation, |id| changed.contains(id))
            };
            examples.push(match older {
                Some(older) if !self.operations.contains(&operation.id) && reaches_change() => {
                    older.examples.clone()
                }
                _ => operation.examples.clone(),
            });
        }
        for (operation, examples) in self.out.operations.iter_mut().zip(examples) {
            operation.examples = examples;
        }
        for index in 0..self.out.operations.len() {
            let operation = &self.out.operations[index];
            let mut kept = Vec::with_capacity(operation.examples.len());
            for example in &operation.examples {
                let problems = crate::validate::example_problems(&self.out, operation, example);
                if problems.is_empty() {
                    kept.push(example.clone());
                } else {
                    warnings.push(
                        &operation.id,
                        format!(
                            "example {:?} does not hold at release \"{}\" and is left out of it: {}",
                            example.title,
                            self.release,
                            problems.join("; ")
                        ),
                    );
                }
            }
            self.out.operations[index].examples = kept;
        }
    }

    /// Feature uses: the newer model's for what it kept, the release's for
    /// what undo restored from it.
    fn feature_uses(&mut self) {
        let out = &self.out;
        let exists = |shape: &ShapeId| {
            let root = shape.root();
            if root == out.id || out.operations.iter().any(|op| op.id == root) {
                return true;
            }
            match (out.shapes.get(&root), shape.member()) {
                (None, _) => false,
                (Some(_), None) => true,
                (Some(found), Some(name)) => found.members().iter().any(|m| m.name == name),
            }
        };
        let restored = |shape: &ShapeId| {
            self.restored.contains(&shape.root()) || self.restored_members.contains(shape)
        };
        let mut uses: BTreeSet<crate::FeatureUse> = out
            .requirements
            .iter()
            .filter(|usage| !restored(&usage.shape) && exists(&usage.shape))
            .cloned()
            .collect();
        uses.extend(
            self.frozen
                .requirements
                .iter()
                .filter(|usage| restored(&usage.shape) && exists(&usage.shape))
                .cloned(),
        );
        // A greedy label belongs to an operation's route, which undo may have
        // restored (`move`, a restored input): computed from the routes as
        // lowering computes it.
        let operations: BTreeSet<&ShapeId> = out.operations.iter().map(|op| &op.id).collect();
        uses.retain(|usage| {
            usage.feature != crate::Feature::GreedyLabels || !operations.contains(&usage.shape)
        });
        uses.extend(
            out.operations
                .iter()
                .filter(|op| !op.uri.greedy_labels.is_empty())
                .map(|op| crate::FeatureUse {
                    feature: crate::Feature::GreedyLabels,
                    shape: op.id.clone(),
                    source: ShapeId::prelude("http"),
                }),
        );
        self.out.requirements = uses.into_iter().collect();
    }
}

/// Every shape the operations' inputs, outputs and errors and the
/// service's errors reach, as the audience projection computes it.
fn reached(service: &Service) -> BTreeSet<ShapeId> {
    crate::audience::reached(
        &service.shapes,
        service
            .operations
            .iter()
            .flat_map(|op| op.input.iter().chain(&op.output).chain(&op.errors))
            .chain(&service.errors)
            .cloned(),
    )
}

/// Whether `operation` reaches a shape `hit` accepts.
fn reaches(
    shapes: &BTreeMap<ShapeId, Shape>,
    operation: &Operation,
    hit: impl Fn(&ShapeId) -> bool,
) -> bool {
    let mut seen = BTreeSet::new();
    let mut pending: Vec<&ShapeId> = operation
        .input
        .iter()
        .chain(&operation.output)
        .chain(&operation.errors)
        .collect();
    while let Some(id) = pending.pop() {
        if hit(id) {
            return true;
        }
        if seen.insert(id)
            && let Some(shape) = shapes.get(id)
        {
            for member in shape.members() {
                pending.push(&member.target);
            }
        }
    }
    false
}

/// Puts `id` into `list` where the release's `order` has it: before the
/// first operation the release lists after it. Nothing when it is there, or
/// the release does not list it.
fn insert_in_order(list: &mut Vec<ShapeId>, order: &[ShapeId], id: &ShapeId) {
    let Some(at) = order.iter().position(|other| other == id) else {
        return;
    };
    if list.contains(id) {
        return;
    }
    let later: BTreeSet<&ShapeId> = order[at + 1..].iter().collect();
    let index = list
        .iter()
        .position(|other| later.contains(other))
        .unwrap_or(list.len());
    list.insert(index, id.clone());
}

fn members_mut(shape: &mut Shape) -> Vec<&mut crate::ir::Member> {
    match shape {
        Shape::Structure(s) | Shape::Union(s) => s.members.iter_mut().collect(),
        Shape::List(list) => vec![&mut list.member],
        Shape::Map(map) => vec![&mut map.key, &mut map.value],
        _ => Vec::new(),
    }
}

fn lifecycle_mut(lifecycle: &mut crate::ir::ResourceLifecycle) -> [&mut Option<ShapeId>; 6] {
    [
        &mut lifecycle.create,
        &mut lifecycle.put,
        &mut lifecycle.read,
        &mut lifecycle.update,
        &mut lifecycle.delete,
        &mut lifecycle.list,
    ]
}

fn structure_member<'s>(
    service: &'s Service,
    shape: &ShapeId,
    name: &str,
) -> Option<&'s crate::ir::Member> {
    match service.shapes.get(shape)? {
        Shape::Structure(s) | Shape::Union(s) => s.member(name),
        _ => None,
    }
}

fn structure_member_mut<'s>(
    service: &'s mut Service,
    shape: &ShapeId,
    name: &str,
) -> Option<&'s mut crate::ir::Member> {
    match service.shapes.get_mut(shape)? {
        Shape::Structure(s) | Shape::Union(s) => s.members.iter_mut().find(|m| m.name == name),
        _ => None,
    }
}

fn error_info<'s>(service: &'s Service, shape: &ShapeId) -> Option<&'s ErrorInfo> {
    match service.shapes.get(shape)? {
        Shape::Structure(s) => s.error.as_ref(),
        _ => None,
    }
}

/// The entry for `name` in `map` becomes the release's entry for the
/// member it called `older`, or goes when the release has none.
fn restore_entry<T: Clone>(
    map: &mut BTreeMap<String, T>,
    then: Option<&BTreeMap<String, T>>,
    name: &str,
    older: &str,
) {
    map.remove(name);
    if let Some(value) = then.and_then(|then| then.get(older)) {
        map.insert(name.to_owned(), value.clone());
    }
}

/// Where a retyped member travels, as the release's operation bound it
/// (by its older name, now `member`): a label, a query parameter, a header,
/// a body member or the payload, and its encodings. Each list keeps member
/// order, `order` being the structure's members.
fn restore_location(
    bindings: &mut Bindings,
    then: &Bindings,
    member: &str,
    older: &str,
    order: &[String],
) {
    let named = |found: bool| found.then(|| member.to_owned());
    place(
        &mut bindings.labels,
        named(then.labels.iter().any(|l| l == older)),
        String::as_str,
        member,
        order,
    );
    place(
        &mut bindings.body,
        named(then.body.iter().any(|b| b == older)),
        String::as_str,
        member,
        order,
    );
    for (list, older_list) in [
        (&mut bindings.query, &then.query),
        (&mut bindings.headers, &then.headers),
    ] {
        place(
            list,
            older_list
                .iter()
                .find(|b| b.member == older)
                .map(|b| crate::ir::Binding {
                    member: member.to_owned(),
                    name: b.name.clone(),
                }),
            |b| &b.member,
            member,
            order,
        );
    }
    if bindings
        .payload
        .as_ref()
        .is_some_and(|p| p.member == member)
        || then.payload.as_ref().is_some_and(|p| p.member == older)
    {
        bindings.payload = renamed_payload(then.payload.as_ref(), member, older);
    }
    restore_entry(
        &mut bindings.timestamp_formats,
        Some(&then.timestamp_formats),
        member,
        older,
    );
    restore_entry(
        &mut bindings.header_encodings,
        Some(&then.header_encodings),
        member,
        older,
    );
}

/// The release's payload binding when it is the member's, under the
/// member's current name; none when the release sent the member otherwise.
fn renamed_payload(
    then: Option<&crate::ir::Payload>,
    member: &str,
    older: &str,
) -> Option<crate::ir::Payload> {
    then.filter(|p| p.member == older)
        .map(|p| crate::ir::Payload {
            member: member.to_owned(),
            ..p.clone()
        })
}

/// `list` without its entry for `member`, then with `entry`, if any, before
/// the first entry for a member `order` lists after `member`.
fn place<T>(
    list: &mut Vec<T>,
    entry: Option<T>,
    name: impl Fn(&T) -> &str,
    member: &str,
    order: &[String],
) {
    list.retain(|item| name(item) != member);
    let Some(entry) = entry else {
        return;
    };
    let rank = |item: &str| order.iter().position(|m| m == item);
    let at = rank(member);
    let index = list
        .iter()
        .position(|item| match (rank(name(item)), at) {
            (Some(other), Some(at)) => other > at,
            _ => false,
        })
        .unwrap_or(list.len());
    list.insert(index, entry);
}

/// A member renamed from `to` back to `from` in an operation's bindings,
/// each binding the release's own where it has one for `from`.
fn rename_bindings(bindings: &mut Bindings, older: Option<&Bindings>, to: &str, from: &str) {
    for label in &mut bindings.labels {
        if label == to {
            *label = from.to_owned();
        }
    }
    for name in &mut bindings.body {
        if name == to {
            *name = from.to_owned();
        }
    }
    for (list, then) in [
        (&mut bindings.query, older.map(|b| &b.query)),
        (&mut bindings.headers, older.map(|b| &b.headers)),
    ] {
        for binding in list.iter_mut().filter(|b| b.member == to) {
            *binding = then
                .and_then(|then| then.iter().find(|b| b.member == from))
                .cloned()
                .unwrap_or_else(|| crate::ir::Binding {
                    member: from.to_owned(),
                    name: binding.name.clone(),
                });
        }
    }
    if let Some(payload) = &mut bindings.payload
        && payload.member == to
    {
        *payload = older
            .and_then(|b| b.payload.as_ref())
            .filter(|p| p.member == from)
            .cloned()
            .unwrap_or_else(|| crate::ir::Payload {
                member: from.to_owned(),
                ..payload.clone()
            });
    }
    rekey(
        &mut bindings.timestamp_formats,
        older.map(|b| &b.timestamp_formats),
        to,
        from,
    );
    rekey(
        &mut bindings.header_encodings,
        older.map(|b| &b.header_encodings),
        to,
        from,
    );
}

/// A member renamed back in an error's own bindings.
fn rename_error(error: &mut ErrorInfo, older: Option<&ErrorInfo>, to: &str, from: &str) {
    for binding in error.headers.iter_mut().filter(|b| b.member == to) {
        *binding = older
            .and_then(|e| e.headers.iter().find(|b| b.member == from))
            .cloned()
            .unwrap_or_else(|| crate::ir::Binding {
                member: from.to_owned(),
                name: binding.name.clone(),
            });
    }
    if let Some(payload) = &mut error.payload
        && payload.member == to
    {
        payload.member = from.to_owned();
    }
    rekey(
        &mut error.timestamp_formats,
        older.map(|e| &e.timestamp_formats),
        to,
        from,
    );
    rekey(
        &mut error.header_encodings,
        older.map(|e| &e.header_encodings),
        to,
        from,
    );
}

/// The entry for `to` becomes the entry for `from`: the release's when it
/// has one, else the newer one renamed.
fn rekey<T: Clone>(
    map: &mut BTreeMap<String, T>,
    older: Option<&BTreeMap<String, T>>,
    to: &str,
    from: &str,
) {
    if let Some(value) = map.remove(to) {
        let value = older
            .and_then(|older| older.get(from))
            .cloned()
            .unwrap_or(value);
        map.insert(from.to_owned(), value);
    }
}

/// A route label renamed back.
fn rename_labels(uri: &mut Uri, to: &str, from: &str) {
    for segment in &mut uri.segments {
        if let Segment::Label(name) = segment
            && name == to
        {
            *name = from.to_owned();
        }
    }
    for name in &mut uri.greedy_labels {
        if name == to {
            *name = from.to_owned();
        }
    }
}

/// `path`, from `root`, with each segment that names member `to` of
/// `shape` naming `from`. A path may cross a list to its element.
fn renamed_path(
    shapes: &BTreeMap<ShapeId, Shape>,
    root: Option<&ShapeId>,
    path: &str,
    shape: &ShapeId,
    to: &str,
    from: &str,
) -> String {
    let mut segments: Vec<String> = path.split('.').map(str::to_owned).collect();
    rename_segments(shapes, root, &mut segments, shape, to, from);
    segments.join(".")
}

fn rename_segments(
    shapes: &BTreeMap<ShapeId, Shape>,
    root: Option<&ShapeId>,
    segments: &mut [String],
    shape: &ShapeId,
    to: &str,
    from: &str,
) {
    rename_path(shapes, root, segments, |id, segment| {
        (id == shape && segment == to).then(|| from.to_owned())
    });
}

/// `segments`, a member path from `root` through `shapes`, with each
/// segment `rename` gives another name for renamed: `rename` is asked for
/// the structure that holds the member and the member's name in `shapes`.
/// Each segment is resolved before it is renamed, so one pass renames a
/// path through several renamed members. A path may cross a list to its
/// element; it stops where a segment does not resolve.
pub fn rename_path(
    shapes: &BTreeMap<ShapeId, Shape>,
    root: Option<&ShapeId>,
    segments: &mut [String],
    rename: impl Fn(&ShapeId, &str) -> Option<String>,
) {
    let mut current = root.cloned();
    for segment in segments.iter_mut() {
        let Some(mut id) = current.take() else {
            return;
        };
        if let Some(Shape::List(list)) = shapes.get(&id) {
            id = list.member.target.clone();
        }
        let Some(member) = shapes
            .get(&id)
            .and_then(|s| s.members().into_iter().find(|m| m.name == *segment))
        else {
            return;
        };
        current = Some(member.target.clone());
        if let Some(name) = rename(&id, segment) {
            *segment = name;
        }
    }
}

fn rename_resource_paths(
    shapes: &BTreeMap<ShapeId, Shape>,
    root: Option<&ShapeId>,
    bindings: &mut ResourceMemberBindings,
    shape: &ShapeId,
    to: &str,
    from: &str,
) {
    for path in bindings
        .identifiers
        .values_mut()
        .chain(bindings.properties.values_mut())
    {
        rename_segments(shapes, root, path, shape, to, from);
    }
    let collections = std::mem::take(&mut bindings.collections);
    for (name, mut inner) in collections {
        // A collection's bindings are relative to each element.
        let element = root
            .and_then(|id| shapes.get(id))
            .and_then(|s| s.members().into_iter().find(|m| m.name == name))
            .and_then(|m| match shapes.get(&m.target) {
                Some(Shape::List(list)) => Some(list.member.target.clone()),
                Some(Shape::Map(map)) => Some(map.value.target.clone()),
                _ => None,
            });
        rename_resource_paths(shapes, element.as_ref(), &mut inner, shape, to, from);
        let name = match root {
            Some(root) if root == shape && name == to => from.to_owned(),
            _ => name,
        };
        bindings.collections.insert(name, inner);
    }
}

/// The current permission an older requirement names: the primary of the
/// permission the release's `clause` requires, found in the current model
/// by its resource's authorization name and its value. A legacy handler
/// checks it on the release's input, so the resource's identifiers (and
/// parents, when the release recorded them) must not have changed, unless
/// the handler discharges the clause itself. `Err` says what the clause
/// requires that the current model cannot check.
pub fn current_permission(
    current: &BTreeMap<ShapeId, ResourcePermissions>,
    older: &BTreeMap<ShapeId, ResourcePermissions>,
    clause: &crate::ir::Requirement,
) -> Result<(ShapeId, String), String> {
    let resource = older
        .get(&clause.resource)
        .ok_or_else(|| format!("permissions of `{}`, which declares none", clause.resource))?;
    let permission = resource
        .permission(&clause.permission)
        .map(|p| resource.primary(p))
        .ok_or_else(|| format!("`{}` on `{}`", clause.permission, resource.name))?;
    let gone = || {
        format!(
            "`{}` on `{}`, which the current model no longer declares",
            permission.value, resource.name
        )
    };
    let (id, now) = current
        .iter()
        .find(|(_, now)| now.name == resource.name)
        .ok_or_else(gone)?;
    let symbol = now
        .permissions
        .iter()
        .find(|p| p.value == permission.value)
        .map(|p| now.primary(p).symbol.clone())
        .ok_or_else(gone)?;
    // A binding is positional: the old input's members bind the release's
    // identifiers in order, and the check names the current description.
    // Unless both list the same identifiers in the same order, a member would
    // bind the wrong one (a resource moved from one parent to another keeps
    // its count), so anything but an unchanged list is refused. A move to a
    // parent whose identifier has the same name (or none) keeps the list, so
    // the parents are compared too, when the release recorded them (one
    // frozen before they were has none, and is compared by identifiers
    // alone).
    if clause.mode != Mode::Handler && now.identifiers != resource.identifiers {
        return Err(format!(
            "`{}` on `{}`, whose identifiers have changed since ({} then, {} now)",
            permission.value,
            resource.name,
            identifier_list(&resource.identifiers),
            identifier_list(&now.identifiers)
        ));
    }
    if clause.mode != Mode::Handler
        && let Some(then) = &resource.parents
        && now.parents.as_ref() != Some(then)
    {
        return Err(format!(
            "`{}` on `{}`, whose parent resources have changed since ({} then, {} now)",
            permission.value,
            resource.name,
            identifier_list(then),
            identifier_list(now.parents.as_deref().unwrap_or_default())
        ));
    }
    Ok((id.clone(), symbol))
}

fn identifier_list(identifiers: &[String]) -> String {
    format!("`({})`", identifiers.join(", "))
}
