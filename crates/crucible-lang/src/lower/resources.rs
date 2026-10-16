//! Resources in the IR (decisions 31-34): what each resource the service
//! reaches declares, the operations that attach themselves to it, and which
//! of their members carry its identifiers and properties.
//!
//! The resource side is assembled from the operation side: a resource lists
//! no operations in the source (decision 32), and a member binds an entry
//! only by elision (decisions 33 and 34), which member lowering records in
//! [`Carried`] as it lowers each `$name`.
//!
//! The targets of identifiers and properties are not visited here: a type
//! only a resource names stays out of `Service::shapes`, so lowering
//! resources leaves the contract's shapes, and every SDK generated from
//! them, as they were (decision 67).

use std::collections::{BTreeMap, BTreeSet};

use crucible_ir::ShapeId;
use crucible_ir::ir::{
    Operation as IrOperation, Resource as IrResource, ResourceLifecycle, ResourceMemberBindings,
    ResourceOperationBindings, Shape, Traits,
};

use super::Lowerer;
use crate::ast::{Decl, Ident, ResourceEntry, ResourceEntryKind};

/// Owner shape → lowered member name → resource name → the entry the member
/// carries and whether it is an identifier or a property.
pub(super) type Carried =
    BTreeMap<ShapeId, BTreeMap<String, BTreeMap<String, (String, ResourceEntryKind)>>>;

impl<'a> Lowerer<'a> {
    /// Records that `owner`'s member `member` carries `resource`'s `entry`.
    pub(super) fn carry(
        &mut self,
        owner: &ShapeId,
        member: &str,
        resource: &str,
        entry: &ResourceEntry,
    ) {
        self.carried
            .entry(owner.clone())
            .or_default()
            .entry(member.to_owned())
            .or_default()
            .insert(resource.to_owned(), (entry.name.name.clone(), entry.kind));
    }

    /// The service's resources and its root resources: every resource in
    /// `reached` (the roots `roots` lists and their descendants), with the
    /// lowered `operations` attached to it.
    pub(super) fn resources(
        &mut self,
        reached: &BTreeSet<String>,
        roots: &[&Ident],
        operations: &[IrOperation],
    ) -> (BTreeMap<ShapeId, IrResource>, Vec<ShapeId>) {
        let mut warnings = Vec::new();
        let lowered = self.lower_resources(reached, roots, operations, &mut warnings);
        for (at, message) in warnings {
            self.warning_at(&at, &message);
        }
        lowered
    }

    fn lower_resources(
        &self,
        reached: &BTreeSet<String>,
        roots: &[&Ident],
        operations: &[IrOperation],
        warnings: &mut Vec<(ShapeId, String)>,
    ) -> (BTreeMap<ShapeId, IrResource>, Vec<ShapeId>) {
        let mut resources = BTreeMap::new();
        for decl in self.decls.values() {
            let Decl::Resource(resource) = &decl.item.decl else {
                continue;
            };
            let name = resource.name.name.as_str();
            if !reached.contains(name) {
                continue;
            }
            let id = self.ref_id(name);
            let children: BTreeSet<ShapeId> = self
                .decls
                .values()
                .filter_map(|child| match &child.item.decl {
                    Decl::Resource(r)
                        if reached.contains(&r.name.name)
                            && r.parent.as_ref().is_some_and(|p| p.name == name) =>
                    {
                        Some(self.ref_id(&r.name.name))
                    }
                    _ => None,
                })
                .collect();
            let properties = resource
                .entries
                .iter()
                .filter(|entry| entry.kind == ResourceEntryKind::Property)
                .filter_map(|entry| {
                    let target = self.type_id(&entry.ty.name.name)?;
                    Some((entry.name.name.clone(), target))
                })
                .collect();
            let mut lowered = IrResource {
                internal: false,
                id: id.clone(),
                identifiers: self.identifier_types(name).into_iter().collect(),
                properties,
                resources: children.into_iter().collect(),
                lifecycle: ResourceLifecycle::default(),
                operations: Vec::new(),
                collection_operations: Vec::new(),
                bindings: BTreeMap::new(),
                traits: Traits::new(),
                // The doc's links were checked with the resource.
                docs: decl.item.doc.as_ref().map(|doc| doc.text.clone()),
                notes: self.resource_notes.get(name).cloned().unwrap_or_default(),
            };
            let mut instance = BTreeSet::new();
            let mut collection = BTreeSet::new();
            for operation in operations {
                if operation.resource.as_ref() != Some(&id) {
                    continue;
                }
                let Some(Decl::Operation(source)) =
                    self.decl(operation.id.name()).map(|d| &d.item.decl)
                else {
                    continue;
                };
                let Some(attachment) = &source.attachment else {
                    continue;
                };
                let lifecycle = &mut lowered.lifecycle;
                let slot = match attachment.role.name.as_str() {
                    "creates" => Some(&mut lifecycle.create),
                    "puts" => Some(&mut lifecycle.put),
                    "reads" => Some(&mut lifecycle.read),
                    "updates" => Some(&mut lifecycle.update),
                    "deletes" => Some(&mut lifecycle.delete),
                    "lists" => Some(&mut lifecycle.list),
                    "on" if attachment.collection => {
                        collection.insert(operation.id.clone());
                        None
                    }
                    "on" => {
                        instance.insert(operation.id.clone());
                        None
                    }
                    // Not a role: reported by `check_role`.
                    _ => continue,
                };
                // A second holder of a role is reported by `check_resources`.
                if let Some(slot) = slot.filter(|slot| slot.is_none()) {
                    *slot = Some(operation.id.clone());
                }
                lowered.bindings.insert(
                    operation.id.clone(),
                    self.operation_bindings(operation, name, warnings),
                );
            }
            lowered.operations = instance.into_iter().collect();
            lowered.collection_operations = collection.into_iter().collect();
            resources.insert(id, lowered);
        }
        let roots: BTreeSet<ShapeId> = roots
            .iter()
            .map(|root| self.ref_id(&root.name))
            .filter(|id| resources.contains_key(id))
            .collect();
        (resources, roots.into_iter().collect())
    }

    /// Which of `operation`'s members carry `resource`'s entries. An input
    /// binds by its own elisions only, since a view typed member of an
    /// input binds nothing (decision 33); an output also through members
    /// typed by a view, a list or map of which binds per element (34).
    fn operation_bindings(
        &self,
        operation: &IrOperation,
        resource: &str,
        warnings: &mut Vec<(ShapeId, String)>,
    ) -> ResourceOperationBindings {
        let mut bind = |shape: Option<&ShapeId>, nested: bool| {
            shape
                .map(|shape| self.member_bindings(shape, resource, nested, warnings))
                .unwrap_or_default()
        };
        ResourceOperationBindings {
            input: bind(operation.input.as_ref(), false),
            output: bind(operation.output.as_ref(), true),
        }
    }

    /// Every member of `shape` that carries one of `resource`'s entries, in
    /// member order, as a path from `shape`: its own members, then, with
    /// `nested`, the members of a structure a member is typed by.
    fn carriers(&self, shape: &ShapeId, resource: &str, nested: bool) -> Vec<Carrier> {
        let mut out = Vec::new();
        let Some(Shape::Structure(structure)) = self.shapes.get(shape) else {
            return out;
        };
        let carried = self.carried.get(shape);
        for member in &structure.members {
            if let Some((entry, kind)) = carried
                .and_then(|members| members.get(&member.name))
                .and_then(|resources| resources.get(resource))
            {
                out.push(Carrier {
                    entry: entry.clone(),
                    kind: *kind,
                    path: vec![member.name.clone()],
                    internal: member.internal,
                    at: shape.with_member(&member.name),
                });
            }
        }
        if !nested {
            return out;
        }
        for member in &structure.members {
            if !matches!(self.shapes.get(&member.target), Some(Shape::Structure(_))) {
                continue;
            }
            for inner in self.carriers(&member.target, resource, false) {
                out.push(Carrier {
                    path: std::iter::once(member.name.clone())
                        .chain(inner.path)
                        .collect(),
                    internal: member.internal || inner.internal,
                    at: shape.with_member(&member.name),
                    ..inner
                });
            }
        }
        out
    }

    /// The bindings of `shape`'s members for `resource` ([`Self::carriers`]),
    /// and of the elements of its list and map members with `nested`. An
    /// entry several members carry binds at the first public one, which the
    /// external projection keeps; the others are reported.
    fn member_bindings(
        &self,
        shape: &ShapeId,
        resource: &str,
        nested: bool,
        warnings: &mut Vec<(ShapeId, String)>,
    ) -> ResourceMemberBindings {
        let mut out = ResourceMemberBindings::default();
        let mut by_entry: BTreeMap<(String, bool), Vec<Carrier>> = BTreeMap::new();
        for carrier in self.carriers(shape, resource, nested) {
            let key = (carrier.entry.clone(), carrier.kind == ResourceEntryKind::Id);
            by_entry.entry(key).or_default().push(carrier);
        }
        for ((entry, identifier), carriers) in by_entry {
            let chosen = carriers
                .iter()
                .position(|c| !c.internal)
                .unwrap_or_default();
            let path = carriers[chosen].path.join(".");
            for (index, other) in carriers.iter().enumerate() {
                if index != chosen {
                    warnings.push((
                        other.at.clone(),
                        format!(
                            "`{path}` and `{}` both carry `{resource}`'s `{entry}`, which the resource binds once, at `{path}`; make the other a plain member",
                            other.path.join("."),
                        ),
                    ));
                }
            }
            let slot = if identifier {
                &mut out.identifiers
            } else {
                &mut out.properties
            };
            slot.insert(entry, carriers[chosen].path.clone());
        }
        if !nested {
            return out;
        }
        let Some(Shape::Structure(structure)) = self.shapes.get(shape) else {
            return out;
        };
        for member in &structure.members {
            let element = match self.shapes.get(&member.target) {
                Some(Shape::List(list)) => &list.member.target,
                Some(Shape::Map(map)) => &map.value.target,
                _ => continue,
            };
            let inner = self.member_bindings(element, resource, false, warnings);
            if !(inner.identifiers.is_empty() && inner.properties.is_empty()) {
                out.collections.insert(member.name.clone(), inner);
            }
        }
        out
    }
}

/// A member, at `path` from the input or output, that carries an entry.
struct Carrier {
    entry: String,
    kind: ResourceEntryKind,
    path: Vec<String>,
    /// The path passes through an internal member, which the external
    /// projection removes.
    internal: bool,
    /// The top-level member, where a problem with it is reported.
    at: ShapeId,
}
