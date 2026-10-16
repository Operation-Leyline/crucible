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

use crate::ir::{
    ApiVersion, Bindings, ErrorInfo, Hook, Mode, Operation, Paginated, Permission, Requirements,
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
        let projected = undo(newer, version, &mut errors, &mut warnings);
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
/// migration from `version` to it undone, taking from `version.service`, the
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
    let mut out = newer.clone();
    out.versions = Vec::new();
    let mut undo = Undo {
        frozen: &version.service,
        release: &version.name,
        out,
        restored: BTreeSet::new(),
        restored_members: BTreeSet::new(),
        changed: BTreeSet::new(),
        changed_operations: BTreeSet::new(),
        operations: BTreeSet::new(),
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
            VersionStep::AddEnumValue { new, value, .. } => undo.remove_value(new, value),
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

struct Undo<'a> {
    frozen: &'a Service,
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

    /// Whether a step on `shape` is part of a definition restored whole.
    fn whole(&self, shape: &ShapeId) -> bool {
        self.restored.contains(shape)
    }

    /// `remove operation Op`: the release's operation, with its own input
    /// and output as the release has them, the errors only it used, and its
    /// requirements mapped to today's permissions.
    fn restore_operation(&mut self, id: &ShapeId) {
        let frozen = self.frozen;
        let Some(older) = frozen.operations.iter().find(|op| &op.id == id) else {
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

    /// `add Enum.V downgrade W`: `V` is not in the release.
    fn remove_value(&mut self, enumeration: &ShapeId, value: &str) {
        let removed = match self.out.shapes.get_mut(enumeration) {
            Some(Shape::Enum(e)) => {
                let before = e.values.len();
                e.values.retain(|v| v.name != value);
                before != e.values.len()
            }
            Some(Shape::IntEnum(e)) => {
                let before = e.values.len();
                e.values.retain(|v| v.name != value);
                before != e.values.len()
            }
            _ => false,
        };
        if removed {
            self.changed.insert(enumeration.clone());
        } else {
            self.problem(
                &enumeration.with_member(value),
                format!(
                    "`add {}.{value}` names a value the newer model no longer has",
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
        // Where the member travels is the release's too.
        let frozen_service = self.frozen;
        let frozen_operations = &frozen_service.operations;
        for operation in &mut self.out.operations {
            let older_operation = frozen_operations.iter().find(|op| op.id == operation.id);
            if operation.input.as_ref() == Some(new) {
                restore_encodings(
                    &mut operation.request,
                    older_operation.map(|op| &op.request),
                    member,
                    older,
                );
            }
            if operation.output.as_ref() == Some(new) {
                restore_encodings(
                    &mut operation.response,
                    older_operation.map(|op| &op.response),
                    member,
                    older,
                );
            }
        }
        let older_error = error_info(frozen_service, old);
        if let Some(Shape::Structure(s)) = self.out.shapes.get_mut(new)
            && let Some(error) = &mut s.error
        {
            restore_entry(
                &mut error.timestamp_formats,
                older_error.map(|e| &e.timestamp_formats),
                member,
                older,
            );
            restore_entry(
                &mut error.header_encodings,
                older_error.map(|e| &e.header_encodings),
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
    /// label, and the pagination, idempotency, signature and resource paths
    /// through it.
    fn rename_member(&mut self, old: &ShapeId, new: &ShapeId, from: &str, to: &str) {
        // Paths are walked before the member is renamed, so each segment
        // still resolves.
        let shapes = &self.out.shapes;
        let frozen = self.frozen;
        let mut paths = Vec::new();
        for operation in &self.out.operations {
            if let Some(p) = &operation.paginated {
                let input = operation.input.as_ref();
                let output = operation.output.as_ref();
                paths.push(Paginated {
                    input_token: renamed_path(shapes, input, &p.input_token, new, to, from),
                    output_token: renamed_path(shapes, output, &p.output_token, new, to, from),
                    items: p
                        .items
                        .as_deref()
                        .map(|path| renamed_path(shapes, output, path, new, to, from)),
                    page_size: p
                        .page_size
                        .as_deref()
                        .map(|path| renamed_path(shapes, input, path, new, to, from)),
                });
            }
        }
        let roots: BTreeMap<ShapeId, (Option<ShapeId>, Option<ShapeId>)> = self
            .out
            .operations
            .iter()
            .map(|op| (op.id.clone(), (op.input.clone(), op.output.clone())))
            .collect();
        let mut resources = self.out.resources.clone();
        for resource in resources.values_mut() {
            for (operation, bindings) in &mut resource.bindings {
                let Some((input, output)) = roots.get(operation) else {
                    continue;
                };
                rename_resource_paths(shapes, input.as_ref(), &mut bindings.input, new, to, from);
                rename_resource_paths(shapes, output.as_ref(), &mut bindings.output, new, to, from);
            }
        }
        self.out.resources = resources;
        let mut paths = paths.into_iter();
        for operation in &mut self.out.operations {
            if operation.paginated.is_some() {
                operation.paginated = paths.next();
            }
            let older = frozen.operations.iter().find(|op| op.id == operation.id);
            if operation.input.as_ref() == Some(new) {
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
            if operation.output.as_ref() == Some(new) {
                rename_bindings(
                    &mut operation.response,
                    older.map(|op| &op.response),
                    to,
                    from,
                );
            }
        }
        let older_error = error_info(frozen, old).cloned();
        if let Some(Shape::Structure(s) | Shape::Union(s)) = self.out.shapes.get_mut(new) {
            if let Some(member) = s.members.iter_mut().find(|m| m.name == to) {
                member.name = from.to_owned();
            }
            if let Some(error) = &mut s.error {
                rename_error(error, older_error.as_ref(), to, from);
            }
        }
    }

    /// What lowering derived, computed again for the projection.
    fn finish(mut self, newer: &Service, warnings: &mut Diagnostics) -> Service {
        let frozen = self.frozen;
        self.authentication(newer);
        // Where restored input and output structures travel is the
        // release's: their bindings, route, tokens and pagination.
        for operation in &mut self.out.operations {
            if self.operations.contains(&operation.id) {
                continue;
            }
            let Some(older) = frozen.operations.iter().find(|op| op.id == operation.id) else {
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
        // linked it to, where the newer model has no label of its own.
        for (label, target) in &frozen.doc_links {
            self.out
                .doc_links
                .entry(label.clone())
                .or_insert_with(|| target.clone());
        }
        for (label, operation) in &frozen.doc_link_operations {
            self.out
                .doc_link_operations
                .entry(label.clone())
                .or_insert_with(|| operation.clone());
        }
        self.out.retain_doc_links();
        self.out
            .operations
            .sort_by(|a, b| a.id.name().cmp(b.id.name()));
        self.out.version = self.release.to_owned();
        self.out.api_version = newer.api_version.as_ref().map(|_| self.release.to_owned());
        self.out
    }

    /// An operation the release has authenticates as the release declared
    /// it; one added since as the newer model does. The service declares
    /// every scheme an operation names.
    fn authentication(&mut self, newer: &Service) {
        let frozen = self.frozen;
        let mut released = BTreeSet::new();
        for operation in &mut self.out.operations {
            if let Some(older) = frozen.operations.iter().find(|op| op.id == operation.id) {
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
                    if older.operations.contains(&operation.id)
                        && !resource.operations.contains(&operation.id)
                    {
                        resource.operations.push(operation.id.clone());
                    }
                    if older.collection_operations.contains(&operation.id)
                        && !resource.collection_operations.contains(&operation.id)
                    {
                        resource.collection_operations.push(operation.id.clone());
                    }
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
        let frozen = self.frozen;
        let changed = &self.changed;
        let shapes = &self.out.shapes;
        let mut examples = Vec::with_capacity(self.out.operations.len());
        for operation in &self.out.operations {
            let older = frozen.operations.iter().find(|op| op.id == operation.id);
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
        self.out.requirements = uses.into_iter().collect();
    }
}

/// Every shape the operations' inputs, outputs and errors and the
/// service's errors reach, as the audience projection computes it.
fn reached(service: &Service) -> BTreeSet<ShapeId> {
    let mut reached = BTreeSet::new();
    let mut pending: Vec<ShapeId> = service
        .operations
        .iter()
        .flat_map(|op| op.input.iter().chain(&op.output).chain(&op.errors))
        .chain(&service.errors)
        .cloned()
        .collect();
    while let Some(id) = pending.pop() {
        if let Some(shape) = service.shapes.get(&id)
            && reached.insert(id)
        {
            pending.extend(shape.members().into_iter().map(|m| m.target.clone()));
        }
    }
    reached
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

/// A retyped member's timestamp format and header encoding, as the
/// release's operation encoded it.
fn restore_encodings(bindings: &mut Bindings, older: Option<&Bindings>, member: &str, from: &str) {
    restore_entry(
        &mut bindings.timestamp_formats,
        older.map(|b| &b.timestamp_formats),
        member,
        from,
    );
    restore_entry(
        &mut bindings.header_encodings,
        older.map(|b| &b.header_encodings),
        member,
        from,
    );
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
        if &id == shape && segment == to {
            *segment = from.to_owned();
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
        .map(|p| primary(resource, p))
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
        .map(|p| primary(now, p).symbol.clone())
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

/// The primary a permission stands for: itself, or the one it aliases.
fn primary<'a>(resource: &'a ResourcePermissions, permission: &'a Permission) -> &'a Permission {
    permission
        .alias_of
        .as_deref()
        .and_then(|symbol| resource.permission(symbol))
        .unwrap_or(permission)
}

fn identifier_list(identifiers: &[String]) -> String {
    format!("`({})`", identifiers.join(", "))
}
