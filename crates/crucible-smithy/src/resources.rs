//! Resource semantics shared by provisioning emitters. Deployment policy is
//! deliberately absent: lifecycle slots come from the model, never names.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::ast::{self, Kind};
use crate::diagnostics::Diagnostics;
use crate::ir::{
    Resource, ResourceLifecycle, ResourceMemberBindings, ResourceOperationBindings,
    ResourceReference,
};
use crate::shape_id::ShapeId;

type Shapes = BTreeMap<ShapeId, ast::Shape>;

fn smithy(name: &str) -> ShapeId {
    ShapeId::new("smithy.api", name)
}

pub(crate) fn sorted_refs(refs: &[ast::Ref]) -> Vec<ShapeId> {
    refs.iter()
        .map(|r| r.target.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn lower(
    model: &Shapes,
    service: &ast::Shape,
    diagnostics: &mut Diagnostics,
) -> BTreeMap<ShapeId, Resource> {
    lower_for_output(model, service, diagnostics, true)
}

pub(crate) fn lower_for_output(
    model: &Shapes,
    service: &ast::Shape,
    diagnostics: &mut Diagnostics,
    bindings: bool,
) -> BTreeMap<ShapeId, Resource> {
    let mut resources = BTreeMap::new();
    let mut visiting = BTreeSet::new();
    for id in sorted_refs(&service.resources) {
        visit(
            model,
            &id,
            &mut resources,
            &mut visiting,
            diagnostics,
            bindings,
        );
    }
    resources
}

fn visit(
    model: &Shapes,
    id: &ShapeId,
    resources: &mut BTreeMap<ShapeId, Resource>,
    visiting: &mut BTreeSet<ShapeId>,
    diagnostics: &mut Diagnostics,
    bindings_enabled: bool,
) {
    if visiting.contains(id) {
        diagnostics.push(id, "resource containment cycle is not supported");
        return;
    }
    if resources.contains_key(id) {
        return;
    }
    let Some(shape) = model.get(id).filter(|shape| shape.kind == Kind::Resource) else {
        diagnostics.push(id, "resource binding must target a defined resource shape");
        return;
    };
    visiting.insert(id.clone());
    if !shape.mixins.is_empty() {
        diagnostics.push(
            id,
            "resource uses mixins; build the model with the flattenAndRemoveMixins transform",
        );
    }
    let identifiers: BTreeMap<_, _> = shape
        .identifiers
        .iter()
        .map(|(name, r)| (name.clone(), r.target.clone()))
        .collect();
    let properties: BTreeMap<_, _> = shape
        .properties
        .iter()
        .map(|(name, r)| (name.clone(), r.target.clone()))
        .collect();
    for (name, target) in &identifiers {
        if !model
            .get(target)
            .is_some_and(|shape| matches!(shape.kind, Kind::String | Kind::Enum))
        {
            diagnostics.push(id, format!("resource identifier `{name}` must target a defined string shape; found {target}"));
        }
        if properties.contains_key(name) {
            diagnostics.push(
                id,
                format!("resource property `{name}` conflicts with an identifier"),
            );
        }
    }
    let target = |r: &Option<ast::Ref>| r.as_ref().map(|r| r.target.clone());
    let lifecycle = ResourceLifecycle {
        create: target(&shape.create),
        put: target(&shape.put),
        read: target(&shape.read),
        update: target(&shape.update),
        delete: target(&shape.delete),
        list: target(&shape.list),
    };
    let mut resource = Resource {
        internal: shape.traits.contains_key(&smithy("internal")),
        id: id.clone(),
        identifiers,
        properties,
        resources: sorted_refs(&shape.resources),
        lifecycle,
        operations: sorted_refs(&shape.operations),
        collection_operations: sorted_refs(&shape.collection_operations),
        bindings: BTreeMap::new(),
        traits: shape.traits.clone(),
        docs: shape
            .traits
            .get(&smithy("documentation"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        notes: crate::ir::DocNotes::lower(id, &shape.traits, diagnostics),
    };
    let instance: BTreeSet<ShapeId> = [
        &shape.create,
        &shape.put,
        &shape.read,
        &shape.update,
        &shape.delete,
    ]
    .into_iter()
    .flatten()
    .chain(&shape.operations)
    .map(|r| r.target.clone())
    .collect();
    let bound: BTreeSet<ShapeId> = instance
        .iter()
        .cloned()
        .chain(shape.list.iter().map(|r| r.target.clone()))
        .chain(resource.collection_operations.iter().cloned())
        .collect();
    for operation_id in bound.into_iter().filter(|_| bindings_enabled) {
        let Some(operation) = model
            .get(&operation_id)
            .filter(|shape| shape.kind == Kind::Operation)
        else {
            diagnostics.push(
                id,
                format!(
                    "resource operation binding {operation_id} must target a defined operation"
                ),
            );
            continue;
        };
        let bind = |io: &Option<ast::Ref>, diagnostics: &mut Diagnostics| {
            io.as_ref()
                .map(|r| {
                    member_bindings(
                        model,
                        &resource,
                        &r.target,
                        instance.contains(&operation_id),
                        diagnostics,
                    )
                })
                .unwrap_or_default()
        };
        let mut bindings = ResourceOperationBindings {
            input: bind(&operation.input, diagnostics),
            output: bind(&operation.output, diagnostics),
        };
        if resource.lifecycle.list.as_ref() == Some(&operation_id)
            && let Some(output) = operation.output.as_ref().and_then(|r| model.get(&r.target))
            && !output
                .members
                .values()
                .any(|m| m.traits.contains_key(&smithy("nestedProperties")))
        {
            let lists: Vec<_> = output
                .members
                .iter()
                .filter_map(|(name, member)| {
                    collection_element(model, member).map(|element| (name, element))
                })
                .collect();
            if let [(name, element)] = lists.as_slice() {
                // Smithy's automatic list inference tolerates derived fields and
                // conflicts; only validated matching properties are projected.
                let inferred =
                    member_bindings(model, &resource, element, true, &mut Diagnostics::default());
                bindings
                    .output
                    .collections
                    .insert((*name).clone(), inferred);
            }
        }
        resource.bindings.insert(operation_id, bindings);
    }
    for child_id in &resource.resources {
        if let Some(child) = model.get(child_id) {
            for (name, target) in &resource.identifiers {
                if child.identifiers.get(name).map(|r| &r.target) != Some(target) {
                    diagnostics.push(child_id, format!("nested resource must retain parent {id} identifier `{name}` targeting {target}"));
                }
            }
        }
        visit(
            model,
            child_id,
            resources,
            visiting,
            diagnostics,
            bindings_enabled,
        );
    }
    visiting.remove(id);
    resources.insert(id.clone(), resource);
}

fn member_bindings(
    model: &Shapes,
    resource: &Resource,
    io: &ShapeId,
    properties: bool,
    diagnostics: &mut Diagnostics,
) -> ResourceMemberBindings {
    let mut result = ResourceMemberBindings::default();
    let Some(shape) = model.get(io).filter(|shape| shape.kind == Kind::Structure) else {
        return result;
    };
    let nested_count = shape
        .members
        .values()
        .filter(|member| member.traits.contains_key(&smithy("nestedProperties")))
        .count();
    if nested_count > 1 {
        diagnostics.push(
            io,
            "only one member can establish @nestedProperties bindings",
        );
    }
    // Explicit identifier bindings override implicit bindings regardless of member order.
    for (name, member) in &shape.members {
        if resource.identifiers.get(name) == Some(&member.target)
            && member.traits.contains_key(&smithy("required"))
            && !member.traits.contains_key(&smithy("resourceIdentifier"))
        {
            result.identifiers.insert(name.clone(), vec![name.clone()]);
        }
    }
    let mut explicit = BTreeSet::new();
    for (name, member) in &shape.members {
        let member_id = io.with_member(name);
        if let Some(value) = member.traits.get(&smithy("resourceIdentifier")) {
            let Some(identifier) = value.as_str() else {
                diagnostics.push(
                    &member_id,
                    "@resourceIdentifier must contain an identifier name",
                );
                continue;
            };
            if !resource.identifiers.contains_key(identifier) {
                diagnostics.push(
                    &member_id,
                    format!(
                        "@resourceIdentifier names unknown identifier `{identifier}` on {}",
                        resource.id
                    ),
                );
            } else if !member.traits.contains_key(&smithy("required"))
                || !model
                    .get(&member.target)
                    .is_some_and(|s| matches!(s.kind, Kind::String | Kind::Enum))
            {
                diagnostics.push(
                    &member_id,
                    "@resourceIdentifier must mark a required string member",
                );
            } else if !explicit.insert(identifier.to_owned()) {
                diagnostics.push(
                    &member_id,
                    format!("ambiguous explicit binding for resource identifier `{identifier}`"),
                );
            } else {
                result
                    .identifiers
                    .insert(identifier.to_owned(), vec![name.clone()]);
            }
        }
        if !properties {
            if member.traits.contains_key(&smithy("nestedProperties")) {
                if let Some(element) = collection_element(model, member) {
                    result.collections.insert(
                        name.clone(),
                        member_bindings(model, resource, element, true, diagnostics),
                    );
                    continue;
                }
            } else {
                continue;
            }
        }
        if member.traits.contains_key(&smithy("nestedProperties")) {
            let Some(nested) = model
                .get(&member.target)
                .filter(|s| s.kind == Kind::Structure)
            else {
                diagnostics.push(&member_id, "@nestedProperties must target a structure");
                continue;
            };
            for (nested_name, nested_member) in &nested.members {
                let nested_id = member.target.with_member(nested_name);
                if ["property", "notProperty", "nestedProperties"]
                    .iter()
                    .any(|t| nested_member.traits.contains_key(&smithy(t)))
                {
                    diagnostics.push(&nested_id, "nested resource properties cannot remap, exclude, or further nest property bindings");
                    continue;
                }
                bind_property(
                    resource,
                    &nested_id,
                    nested_name,
                    nested_member,
                    vec![name.clone(), nested_name.clone()],
                    &mut result,
                    diagnostics,
                );
            }
        } else if nested_count == 0
            && !not_property(model, member)
            && !result
                .identifiers
                .values()
                .any(|p| p.as_slice() == std::slice::from_ref(name))
        {
            let property = match member.traits.get(&smithy("property")) {
                Some(value) => match value.get("name").and_then(Value::as_str) {
                    Some(property) if resource.properties.contains_key(property) => property,
                    _ => {
                        diagnostics.push(
                            &member_id,
                            "@property must name a declared resource property",
                        );
                        continue;
                    }
                },
                None => name,
            };
            bind_property(
                resource,
                &member_id,
                property,
                member,
                vec![name.clone()],
                &mut result,
                diagnostics,
            );
        }
    }
    result
}

fn collection_element<'a>(model: &'a Shapes, member: &ast::Member) -> Option<&'a ShapeId> {
    let list = model.get(&member.target).filter(|s| s.kind == Kind::List)?;
    let element = &list.member.as_ref()?.target;
    model
        .get(element)
        .filter(|s| s.kind == Kind::Structure)
        .map(|_| element)
}

fn not_property(model: &Shapes, member: &ast::Member) -> bool {
    member.traits.keys().any(|id| {
        (id.is_prelude()
            && [
                "notProperty",
                "resourceIdentifier",
                "idempotencyToken",
                "httpResponseCode",
            ]
            .contains(&id.name()))
            || model
                .get(id)
                .is_some_and(|s| s.traits.contains_key(&smithy("notProperty")))
    })
}

fn bind_property(
    resource: &Resource,
    member_id: &ShapeId,
    property: &str,
    member: &ast::Member,
    path: Vec<String>,
    bindings: &mut ResourceMemberBindings,
    diagnostics: &mut Diagnostics,
) {
    let Some(target) = resource.properties.get(property) else {
        return;
    };
    if target != &member.target {
        diagnostics.push(
            member_id,
            format!(
                "resource property `{property}` must target {target}, not {}",
                member.target
            ),
        );
    } else if bindings
        .properties
        .insert(property.to_owned(), path)
        .is_some()
    {
        diagnostics.push(
            member_id,
            format!("ambiguous binding for resource property `{property}`"),
        );
    }
}

pub(crate) fn references<'a>(
    model: &Shapes,
    reachable: impl Iterator<Item = &'a ShapeId>,
    diagnostics: &mut Diagnostics,
) -> BTreeMap<ShapeId, Vec<ResourceReference>> {
    let mut result = BTreeMap::new();
    for id in reachable {
        let shape = &model[id];
        let Some(value) = shape.traits.get(&smithy("references")) else {
            continue;
        };
        let Some(values) = value.as_array() else {
            diagnostics.push(id, "@references must be a list");
            continue;
        };
        if !matches!(shape.kind, Kind::String | Kind::Enum | Kind::Structure) {
            diagnostics.push(
                id,
                "@references is supported only on strings and structures",
            );
            continue;
        }
        let mut refs = Vec::new();
        for value in values {
            if let Some(reference) = reference(model, id, shape, value, diagnostics) {
                refs.push(reference);
            }
        }
        refs.sort_by(|a, b| {
            (&a.resource, &a.service, &a.rel).cmp(&(&b.resource, &b.service, &b.rel))
        });
        result.insert(id.clone(), refs);
    }
    result
}

fn reference(
    model: &Shapes,
    id: &ShapeId,
    shape: &ast::Shape,
    value: &Value,
    diagnostics: &mut Diagnostics,
) -> Option<ResourceReference> {
    let parse = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<ShapeId>().ok())
            .filter(|id| id.member().is_none())
    };
    let Some(resource) = parse("resource") else {
        diagnostics.push(
            id,
            "@references.resource must be an absolute resource shape ID",
        );
        return None;
    };
    let service = parse("service");
    if value.get("service").is_some() && service.is_none() {
        diagnostics.push(
            id,
            "@references.service must be an absolute service shape ID",
        );
    }
    if let Some(service_id) = &service {
        if model
            .get(service_id)
            .is_some_and(|s| s.kind != Kind::Service)
        {
            diagnostics.push(
                id,
                format!("reference service {service_id} is not a service"),
            );
        }
    }
    let target = model.get(&resource);
    if target.is_some_and(|s| s.kind != Kind::Resource) {
        diagnostics.push(id, format!("reference target {resource} is not a resource"));
        return None;
    }
    let mut identifiers = BTreeMap::new();
    if matches!(shape.kind, Kind::String | Kind::Enum) {
        if value.get("ids").is_some() {
            diagnostics.push(id, "a string @references must omit ids");
        }
        if let Some(target) = target {
            if target.identifiers.len() != 1 {
                diagnostics.push(
                    id,
                    "a string reference requires a resource with exactly one identifier",
                );
            } else {
                identifiers.insert(
                    target.identifiers.keys().next().unwrap().clone(),
                    Vec::new(),
                );
            }
        }
    } else {
        match value.get("ids") {
            Some(Value::Object(ids)) => {
                for (identifier, member) in ids {
                    let Some(member) = member.as_str() else {
                        diagnostics.push(id, "@references.ids values must be member names");
                        continue;
                    };
                    identifiers.insert(identifier.clone(), vec![member.to_owned()]);
                }
            }
            Some(_) => diagnostics.push(id, "@references.ids must be a map"),
            None => {
                if let Some(target) = target {
                    identifiers.extend(
                        target
                            .identifiers
                            .keys()
                            .map(|name| (name.clone(), vec![name.clone()])),
                    );
                }
            }
        }
        for (identifier, path) in &identifiers {
            if !shape
                .members
                .get(&path[0])
                .and_then(|m| model.get(&m.target))
                .is_some_and(|s| matches!(s.kind, Kind::String | Kind::Enum))
            {
                diagnostics.push(id, format!("reference identifier `{identifier}` must map to a string member; found `{}`", path[0]));
            }
            if target.is_some_and(|s| !s.identifiers.contains_key(identifier)) {
                diagnostics.push(
                    id,
                    format!("reference maps unknown identifier `{identifier}` on {resource}"),
                );
            }
        }
        if let Some(target) = target {
            for name in target.identifiers.keys() {
                if !identifiers.contains_key(name) {
                    diagnostics.push(
                        id,
                        format!("reference omits identifier `{name}` on {resource}"),
                    );
                }
            }
        }
    }
    Some(ResourceReference {
        resource,
        service,
        identifiers,
        rel: value.get("rel").and_then(Value::as_str).map(str::to_owned),
        resolved: target.is_some(),
    })
}
