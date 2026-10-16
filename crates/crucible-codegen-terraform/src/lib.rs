//! Terraform/OpenTofu providers backed by the generated Go client's wire runtime.
use crucible_codegen_infra::{Plan, Property, Resource};
use crucible_ir::ir::{Service, Shape, SimpleKind};
use crucible_ir::{Diagnostics, ShapeId};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub struct Options {
    pub module_path: String,
    pub provider_name: String,
    pub provider_address: String,
}

pub struct File {
    pub path: String,
    pub contents: String,
}

/// Opt-in policy: the API must complete mutations synchronously, and read must
/// return the resulting state immediately. Only named modelled errors mean absent.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerraformBinding {
    pub name: String,
    pub synchronous: bool,
    pub read_after_write_consistent: bool,
    pub not_found_errors: Vec<String>,
}

/// A Go string literal: JSON's quoting would leave a byte order mark raw,
/// which Go refuses anywhere but a file's start.
fn literal(value: &str) -> String {
    crucible_codegen_common::quoted(value)
}

fn valid_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn kind(service: &Service, property: &Property) -> Option<&'static str> {
    match service.shape(&property.target)? {
        Shape::Simple(s) => match s.kind {
            SimpleKind::String => Some("string"),
            SimpleKind::Boolean => Some("bool"),
            SimpleKind::Byte | SimpleKind::Short | SimpleKind::Integer | SimpleKind::Long => {
                Some("int64")
            }
            SimpleKind::Double => Some("float64"),
            _ => None,
        },
        Shape::Enum(_) => Some("string"),
        _ => None,
    }
}

fn member_at<'a>(
    service: &'a Service,
    root: &ShapeId,
    path: &[String],
) -> Option<(&'a crucible_ir::ir::Member, bool)> {
    let mut target = root;
    let mut sensitive = false;
    let mut result = None;
    for part in path {
        let Shape::Structure(shape) = service.shape(target)? else {
            return None;
        };
        let member = shape.member(part)?;
        sensitive |= shape.sensitive() || service.sensitive(member);
        result = Some(member);
        target = &member.target;
    }
    result.map(|member| (member, sensitive))
}

fn descriptor(service: &Service, resource: &Resource, errors: &mut Diagnostics) -> Option<Value> {
    let raw = resource.terraform.as_ref()?;
    let binding: TerraformBinding = match serde_json::from_value(raw.clone()) {
        Ok(binding) => binding,
        Err(e) => {
            errors.push(&resource.id, format!("invalid Terraform binding: {e}"));
            return None;
        }
    };
    if !valid_name(&binding.name) {
        errors.push(
            &resource.id,
            "Terraform resource name must be a lowercase identifier",
        );
    }
    if !binding.synchronous || !binding.read_after_write_consistent {
        errors.push(&resource.id, "Terraform requires explicit synchronous mutations and read_after_write_consistent; asynchronous/eventually consistent lifecycles need a custom provider");
    }
    let model = service.resources.get(&resource.id)?;
    if model.identifiers.len() != 1 {
        errors.push(
            &resource.id,
            "Terraform currently requires exactly one server-generated string identifier",
        );
        return None;
    }
    let (identifier, target) = model.identifiers.first_key_value().expect("one identifier");
    match service.shape(target) {
        Some(Shape::Simple(s)) if s.kind == SimpleKind::String => {}
        // A type only the resource names is not among the shapes (decision 67).
        None => errors.push(
            &resource.id,
            format!("Terraform identifier `{identifier}` has type {target}, which no operation reaches; bind it in an operation input"),
        ),
        Some(_) => errors.push(&resource.id, "Terraform identifier must be a string"),
    }
    if service.shape(target).is_some_and(Shape::sensitive) {
        errors.push(
            &resource.id,
            "Terraform resource identifiers cannot be sensitive",
        );
    }
    let mut properties = BTreeMap::new();
    for (name, property) in &resource.properties {
        if !valid_name(name)
            || [
                "id",
                "count",
                "for_each",
                "depends_on",
                "provider",
                "lifecycle",
                "provisioner",
                "connection",
            ]
            .contains(&name.as_str())
        {
            errors.push(
                &resource.id,
                format!("Terraform property `{name}` must be a non-reserved lowercase identifier"),
            );
        }
        if model.properties.get(name) != Some(&property.target) {
            errors.push(
                &resource.id,
                format!(
                    "Terraform property `{name}` must match the lowered resource property's target"
                ),
            );
        }
        let Some(kind) = kind(service, property) else {
            errors.push(
                &resource.id,
                format!("Terraform property `{name}` must use string, enum, boolean, integer, or double; other scalar and collection types are unsupported"),
            );
            continue;
        };
        if !property.required && !property.immutable {
            errors.push(&resource.id, format!("optional mutable Terraform property `{name}` requires null/reset semantics; mark immutable or use a custom provider"));
        }
        if property.constraints.pattern.is_some() {
            errors.push(&resource.id, format!("Terraform property `{name}` has a pattern constraint; regex dialect translation is unsupported"));
        }
        if property.integer && kind != "int64" {
            errors.push(&resource.id, format!("Terraform property `{name}` uses an unsupported integer refinement of a non-integer model type"));
        }
        let enum_values =
            property
                .enum_values
                .clone()
                .or_else(|| match service.shape(&property.target) {
                    Some(Shape::Enum(e)) => {
                        Some(e.values.iter().map(|v| v.value.clone()).collect())
                    }
                    _ => None,
                });
        let mut description = json!({"Kind":kind,"Required":property.required,"Immutable":property.immutable,"Sensitive":property.sensitive,"Description":property.docs.clone().unwrap_or_default(),"Enum":enum_values,"MinLength":property.constraints.length.as_ref().and_then(|b|b.min),"MaxLength":property.constraints.length.as_ref().and_then(|b|b.max),"Min":property.constraints.range.as_ref().and_then(|b|b.min.as_ref()),"Max":property.constraints.range.as_ref().and_then(|b|b.max.as_ref())});
        // Terraform int64 must not admit values outside the narrower Smithy type.
        let limits = match service.shape(&property.target) {
            Some(Shape::Simple(s)) => match s.kind {
                SimpleKind::Byte => Some((-128i64, 127i64)),
                SimpleKind::Short => Some((-32768, 32767)),
                SimpleKind::Integer => Some((i32::MIN as i64, i32::MAX as i64)),
                _ => None,
            },
            _ => None,
        };
        if let Some((min, max)) = limits {
            if description["Min"].as_f64().is_none_or(|v| v < min as f64) {
                description["Min"] = min.into();
            }
            if description["Max"].as_f64().is_none_or(|v| v > max as f64) {
                description["Max"] = max.into();
            }
        }
        properties.insert(name, description);
    }
    for (name, property) in &resource.attributes {
        if name != "id" || &property.target != target {
            errors.push(&resource.id, "Terraform currently supports only the string identifier attribute named `id`; other computed attributes need explicit output mappings");
        }
        if property.sensitive {
            errors.push(
                &resource.id,
                "Terraform resource identifiers cannot be sensitive",
            );
        }
    }
    if model.lifecycle.update.is_none() && resource.properties.values().any(|p| !p.immutable) {
        errors.push(
            &resource.id,
            "Terraform resources without update must mark every property immutable",
        );
    }
    let mut operations = BTreeMap::new();
    for (stage, id) in [
        ("Create", &model.lifecycle.create),
        ("Read", &model.lifecycle.read),
        ("Update", &model.lifecycle.update),
        ("Delete", &model.lifecycle.delete),
    ] {
        let Some(id) = id else {
            if stage != "Update" {
                errors.push(
                    &resource.id,
                    format!("Terraform requires an explicit {stage} lifecycle operation"),
                );
            }
            continue;
        };
        let Some(op) = service.operations.iter().find(|op| &op.id == id) else {
            continue;
        };
        let Some(mappings) = model.bindings.get(id) else {
            errors.push(id, "Terraform lifecycle lacks resolved resource bindings");
            continue;
        };
        let mut input = BTreeMap::new();
        let mut output = BTreeMap::new();
        if stage != "Create" {
            if let Some(path) = mappings.input.identifiers.get(identifier) {
                input.insert("id".to_owned(), path.clone());
            } else {
                errors.push(
                    id,
                    "Terraform lifecycle requires an input identifier mapping",
                );
            }
        } else if !mappings.input.identifiers.is_empty() {
            errors.push(
                id,
                "Terraform create currently requires a server-generated identifier",
            );
        }
        if stage == "Create" || stage == "Read" {
            let identifier_path = mappings
                .output
                .identifiers
                .get(identifier)
                .cloned()
                .or_else(|| {
                    let source = resource.attributes.get("id")?.source.as_ref()?;
                    (op.output.as_ref() == Some(&source.shape)).then(|| source.path.clone())
                });
            if let Some(path) = identifier_path {
                output.insert("id".to_owned(), path);
            } else {
                errors.push(
                    id,
                    "Terraform lifecycle requires an output identifier mapping",
                );
            }
        }
        for name in resource.properties.keys() {
            if stage == "Create" || stage == "Update" {
                if let Some(path) = mappings.input.properties.get(name) {
                    input.insert(name.clone(), path.clone());
                } else if stage == "Create" || !resource.properties[name].immutable {
                    errors.push(
                        id,
                        format!("Terraform lifecycle lacks input property `{name}`"),
                    );
                }
            }
            if stage == "Read" {
                if let Some(path) = mappings.output.properties.get(name) {
                    output.insert(name.clone(), path.clone());
                } else {
                    errors.push(id, format!("Terraform read lacks output property `{name}`; refresh must observe every managed property"));
                }
            }
        }
        if let Some(shape) = &op.input {
            check_required_inputs(
                service,
                shape,
                &[],
                &input.values().cloned().collect::<Vec<_>>(),
                op.idempotency_token.as_deref(),
                &mut BTreeSet::new(),
                id,
                errors,
            );
        }
        for (is_input, root, paths) in [
            (true, op.input.as_ref(), &input),
            (false, op.output.as_ref(), &output),
        ] {
            let Some(root) = root else {
                continue;
            };
            for (name, path) in paths {
                let Some((member, sensitive)) = member_at(service, root, path) else {
                    errors.push(id,format!("Terraform lifecycle path for `{name}` must resolve to a structure member"));
                    continue;
                };
                if name == "id" {
                    if &member.target != target || sensitive {
                        errors.push(id,"Terraform identifier mapping must target the non-sensitive resource identifier type");
                    }
                    continue;
                }
                let property = &resource.properties[name];
                if member.target != property.target
                    || member.constraints != property.constraints
                    || sensitive != property.sensitive
                {
                    errors.push(id,format!("Terraform property `{name}` has inconsistent type, constraints, or sensitivity across lifecycle mappings"));
                }
                if is_input && member.required && !property.required {
                    errors.push(
                        id,
                        format!(
                            "Terraform property `{name}` must be required by its lifecycle input"
                        ),
                    );
                }
                if stage == "Create"
                    && is_input
                    && !property
                        .source
                        .as_ref()
                        .is_some_and(|source| &source.shape == root && &source.path == path)
                {
                    errors.push(id,format!("Terraform property `{name}` source must be its canonical create input mapping"));
                }
            }
        }
        if stage == "Read" || stage == "Delete" {
            for error in &binding.not_found_errors {
                if !op.errors.iter().any(|id| id.name() == error) {
                    errors.push(
                        id,
                        format!("Terraform not-found error `{error}` must be modelled by {stage}"),
                    );
                }
            }
        }
        operations.insert(
            stage,
            json!({"Name":op.name(),"Input":input,"Output":output}),
        );
    }
    Some(
        json!({"Name":binding.name,"Properties":properties,"Operations":operations,"NotFoundErrors":binding.not_found_errors}),
    )
}

#[allow(clippy::too_many_arguments)]
fn check_required_inputs(
    service: &Service,
    shape: &ShapeId,
    prefix: &[String],
    paths: &[Vec<String>],
    token: Option<&str>,
    seen: &mut BTreeSet<ShapeId>,
    operation: &ShapeId,
    errors: &mut Diagnostics,
) {
    if !seen.insert(shape.clone()) {
        return;
    }
    if let Some(Shape::Structure(s)) = service.shape(shape) {
        for member in &s.members {
            let mut path = prefix.to_vec();
            path.push(member.name.clone());
            let covered = paths.iter().any(|p| p == &path);
            let descendant = paths.iter().any(|p| p.starts_with(&path));
            if prefix.is_empty() && token == Some(member.name.as_str()) {
                continue;
            }
            if member.required && !covered && !descendant {
                errors.push(
                    operation,
                    format!(
                        "Terraform lifecycle has unmapped required input `{}`",
                        path.join(".")
                    ),
                );
            }
            if descendant && !covered {
                check_required_inputs(
                    service,
                    &member.target,
                    &path,
                    paths,
                    None,
                    seen,
                    operation,
                    errors,
                );
            }
        }
    }
    seen.remove(shape);
}

pub fn emit(service: &Service, plan: &Plan, options: &Options) -> Result<Vec<File>, Diagnostics> {
    let mut errors = Diagnostics::default();
    for id in service.shapes.keys().filter(|id| !id.is_prelude()) {
        if service
            .name_of(id)
            .trim_start_matches('_')
            .eq_ignore_ascii_case("new")
        {
            errors.push(
                id,
                "Go declaration conflicts with the Terraform provider constructor New",
            );
        }
    }
    if !valid_name(&options.provider_name) {
        errors.push(
            &service.id,
            "Terraform provider name must be a lowercase identifier",
        );
    }
    let address: Vec<_> = options.provider_address.split('/').collect();
    if address.len() != 3
        || address.iter().any(|part| {
            part.is_empty()
                || !part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
        })
    {
        errors.push(
            &service.id,
            "Terraform provider address must be hostname/namespace/name",
        );
    }
    let mut names = BTreeSet::new();
    let mut descriptors = Vec::new();
    for resource in &plan.resources {
        if let Some(desc) = descriptor(service, resource, &mut errors) {
            if !names.insert(desc["Name"].as_str().unwrap_or_default().to_string()) {
                errors.push(&resource.id, "duplicate Terraform resource name");
            }
            descriptors.push(desc);
        }
    }
    if descriptors.is_empty() {
        errors.push(
            &service.id,
            "no resources opted in with a Terraform binding",
        );
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    // The permission registry is for consoles built on the Go SDK; a
    // provider never names a permission.
    let sdk = crucible_codegen_go::emit_without_permissions(
        service,
        &crucible_codegen_go::Options {
            module_path: options.module_path.clone(),
            package_name: "provider".into(),
            endpoint: None,
        },
    )?;
    let mut files: Vec<_> = sdk
        .into_iter()
        .filter(|f| f.path != "go.mod")
        .map(|f| File {
            path: format!("internal/provider/{}", f.path),
            contents: f.contents,
        })
        .collect();
    files.push(File{path:"go.mod".into(),contents:format!("module {}\n\ngo 1.24.0\n\nrequire github.com/hashicorp/terraform-plugin-framework v1.16.1\n",options.module_path)});
    let encoded = serde_json::to_string(&descriptors).expect("descriptors serialize");
    files.push(File {
        path: "internal/provider/terraform.go".into(),
        contents: include_str!("../runtime/terraform.go")
            .replace("\r\n", "\n")
            .replace("__PROVIDER_NAME__", &literal(&options.provider_name))
            .replace("__DESCRIPTORS__", &literal(&encoded)),
    });
    files.push(File{path:"main.go".into(),contents:format!("package main\nimport (\n\"context\"\n\"log\"\n\"github.com/hashicorp/terraform-plugin-framework/providerserver\"\n{}\n)\nfunc main() {{ if err := providerserver.Serve(context.Background(), provider.New, providerserver.ServeOpts{{Address: {}, ProtocolVersion: 6}}); err != nil {{ log.Fatal(err) }} }}\n",literal(&format!("{}/internal/provider",options.module_path)),literal(&options.provider_address))});
    Ok(files)
}
