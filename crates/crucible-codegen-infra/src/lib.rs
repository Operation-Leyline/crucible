//! Explicit provisioning policy, resolved against Crucible's shared model.
//!
//! APIs describe requests, not resource ownership or safe replacement. A
//! binding selects the supported deployment surface without guessing those
//! policies from operation names or HTTP methods.

use std::collections::{BTreeMap, BTreeSet};

use crucible_smithy::ir::{
    Bounds, Constraints, Member, Service, Shape, SimpleKind, TimestampFormat,
};
use crucible_smithy::{Diagnostics, ShapeId};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bindings {
    pub resources: BTreeMap<ShapeId, ResourceBinding>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceBinding {
    pub name: String,
    #[serde(default)]
    pub properties: BTreeMap<String, PropertyBinding>,
    #[serde(default)]
    pub attributes: BTreeMap<String, PropertyBinding>,
    pub cdk: Option<CdkConfig>,
    /// Validated by the Terraform emitter, which owns its lifecycle policy.
    pub terraform: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CdkConfig {
    pub resource_type: String,
    pub class_name: Option<String>,
    #[serde(default)]
    pub context: BTreeMap<String, PropertyBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberPath {
    pub shape: ShapeId,
    pub path: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyBinding {
    pub source: Option<MemberPath>,
    /// Only provider context may declare a type without a model member.
    pub target: Option<ShapeId>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub immutable: bool,
    /// An explicit provider refinement; values must be a subset of the model.
    pub enum_values: Option<Vec<String>>,
    #[serde(default)]
    pub integer: bool,
    /// Representation accepted by the deployment provider, when it differs
    /// from the HTTP member's timestamp format.
    pub timestamp_format: Option<String>,
    /// Provider constraints may narrow the API's accepted lengths.
    pub length: Option<Length>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Length {
    pub min: Option<u64>,
    pub max: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub resources: Vec<Resource>,
}

#[derive(Debug, Clone)]
pub struct Resource {
    pub id: ShapeId,
    pub name: String,
    pub properties: BTreeMap<String, Property>,
    pub attributes: BTreeMap<String, Property>,
    pub cdk: Option<CdkBinding>,
    pub terraform: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct CdkBinding {
    pub resource_type: String,
    pub class_name: Option<String>,
    pub context: BTreeMap<String, Property>,
}

#[derive(Debug, Clone)]
pub struct Property {
    pub target: ShapeId,
    pub source: Option<MemberPath>,
    pub constraints: Constraints,
    pub docs: Option<String>,
    pub required: bool,
    pub sensitive: bool,
    pub immutable: bool,
    pub enum_values: Option<Vec<String>>,
    pub integer: bool,
    pub timestamp_format: Option<TimestampFormat>,
}

/// Select and resolve model members once, before invoking any emitter.
pub fn resolve(service: &Service, bindings: &Bindings) -> Result<Plan, Diagnostics> {
    let mut diagnostics = Diagnostics::default();
    let mut names = BTreeSet::new();
    let mut resources = Vec::new();
    if bindings.resources.is_empty() {
        diagnostics.push(&service.id, "provisioning bindings select no resources");
    }
    for (id, binding) in &bindings.resources {
        if !service.resources.contains_key(id) {
            diagnostics.push(id, "provisioning resource is not bound to this service");
            continue;
        }
        if !identifier(&binding.name) || !names.insert(binding.name.clone()) {
            diagnostics.push(id, "resource name must be a unique identifier");
        }
        let properties =
            resolve_properties(service, id, &binding.properties, false, &mut diagnostics);
        let attributes =
            resolve_properties(service, id, &binding.attributes, false, &mut diagnostics);
        for (name, attr) in &attributes {
            if attr.sensitive {
                diagnostics.push(
                    id,
                    format!("attribute {name} exposes a sensitive model member"),
                );
            }
            if attr
                .source
                .as_ref()
                .is_some_and(|source| write_only_path(service, source))
            {
                diagnostics.push(
                    id,
                    format!("attribute {name} exposes a write-only model member"),
                );
            }
        }
        let cdk = binding.cdk.as_ref().map(|config| {
            let context = resolve_properties(service, id, &config.context, true, &mut diagnostics);
            for name in properties.keys().chain(context.keys()) {
                if name == "serviceToken" || name == "ServiceToken" {
                    diagnostics.push(id, "serviceToken is reserved for the CDK provider");
                }
            }
            for name in context.keys() {
                if properties.contains_key(name) {
                    diagnostics.push(
                        id,
                        format!("provider context {name} shadows a resource property"),
                    );
                }
            }
            CdkBinding {
                resource_type: config.resource_type.clone(),
                class_name: config.class_name.clone(),
                context,
            }
        });
        resources.push(Resource {
            id: id.clone(),
            name: binding.name.clone(),
            properties,
            attributes,
            cdk,
            terraform: binding.terraform.clone(),
        });
    }
    if diagnostics.is_empty() {
        Ok(Plan { resources })
    } else {
        Err(diagnostics)
    }
}

fn identifier(value: &str) -> bool {
    value.starts_with(|c: char| c.is_ascii_alphabetic())
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn resolve_properties(
    service: &Service,
    id: &ShapeId,
    bindings: &BTreeMap<String, PropertyBinding>,
    context: bool,
    diagnostics: &mut Diagnostics,
) -> BTreeMap<String, Property> {
    bindings
        .iter()
        .filter_map(|(name, binding)| {
            if !identifier(name) {
                diagnostics.push(id, format!("property {name:?} must be an identifier"));
            }
            resolve_property(service, binding, context)
                .map(|property| (name.clone(), property))
                .map_err(|message| diagnostics.push(id, format!("property {name}: {message}")))
                .ok()
        })
        .collect()
}

fn resolve_property(
    service: &Service,
    binding: &PropertyBinding,
    context: bool,
) -> Result<Property, String> {
    let mut immutable = binding.immutable;
    let mut timestamp_format = None;
    let (target, mut constraints, docs, sensitive) = match (&binding.source, &binding.target) {
        (Some(source), None) => {
            let (member, sensitive) = member_at(service, source)?;
            immutable |= matches!(
                member
                    .traits
                    .get(&ShapeId::new("aws.cloudformation", "cfnMutability"))
                    .and_then(Value::as_str),
                Some("create" | "create-and-read")
            );
            if matches!(service.shape(&member.target), Some(Shape::Simple(simple)) if simple.kind == SimpleKind::Timestamp)
            {
                timestamp_format = Some(service.timestamp_format(member));
            }
            (
                member.target.clone(),
                member.constraints.clone(),
                member.docs.clone(),
                sensitive || sensitive_shape(service, &member.target, &mut BTreeSet::new()),
            )
        }
        (None, Some(target)) if context => {
            let shape = service.shape(target);
            let (constraints, docs, sensitive) = match shape {
                Some(Shape::Simple(simple)) => (
                    simple.constraints.clone(),
                    simple.docs.clone(),
                    shape.is_some_and(Shape::sensitive),
                ),
                None if target.is_prelude()
                    && matches!(
                        target.name(),
                        "String" | "Boolean" | "Integer" | "Long" | "Double"
                    ) =>
                {
                    (Constraints::default(), None, false)
                }
                _ => return Err("provider context must target a scalar model type".into()),
            };
            (target.clone(), constraints, docs, sensitive)
        }
        _ => {
            return Err(
                "provide exactly one source member path (or a scalar target for provider context)"
                    .into(),
            );
        }
    };
    if let Some(length) = &binding.length {
        if !matches!(service.shape(&target), Some(Shape::Simple(s)) if s.kind == SimpleKind::String)
            && !matches!(
                service.shape(&target),
                Some(Shape::Enum(_) | Shape::List(_) | Shape::Map(_))
            )
            && !(target.is_prelude() && target.name() == "String")
        {
            return Err("length refinement requires a string or collection target".into());
        }
        let old = constraints.length.as_ref();
        let min = length.min.or_else(|| old.and_then(|b| b.min));
        let max = length.max.or_else(|| old.and_then(|b| b.max));
        if min.zip(max).is_some_and(|(min, max)| min > max)
            || length
                .min
                .zip(old.and_then(|b| b.min))
                .is_some_and(|(new, old)| new < old)
            || length
                .max
                .zip(old.and_then(|b| b.max))
                .is_some_and(|(new, old)| new > old)
        {
            return Err("length refinement must narrow the model's bounds".into());
        }
        constraints.length = Some(Bounds { min, max });
    }
    if let Some(format) = &binding.timestamp_format {
        if !matches!(service.shape(&target), Some(Shape::Simple(simple)) if simple.kind == SimpleKind::Timestamp)
        {
            return Err("timestamp_format requires a timestamp source".into());
        }
        timestamp_format = Some(match format.as_str() {
            "date-time" => TimestampFormat::DateTime,
            "http-date" => TimestampFormat::HttpDate,
            "epoch-seconds" => TimestampFormat::EpochSeconds,
            _ => {
                return Err(
                    "timestamp_format must be date-time, http-date, or epoch-seconds".into(),
                );
            }
        });
    }
    if binding.integer
        && !matches!(service.shape(&target), Some(Shape::Simple(simple)) if matches!(simple.kind, SimpleKind::Byte | SimpleKind::Short | SimpleKind::Integer | SimpleKind::Long | SimpleKind::Float | SimpleKind::Double))
        && !(target.is_prelude() && matches!(target.name(), "Integer" | "Long" | "Double"))
    {
        return Err("integer refinement requires a numeric model type".into());
    }
    if let Some(values) = &binding.enum_values {
        let Some(Shape::Enum(model)) = service.shape(&target) else {
            return Err("enum_values refinement requires a model enum".into());
        };
        if values.is_empty()
            || values.iter().collect::<BTreeSet<_>>().len() != values.len()
            || values
                .iter()
                .any(|v| !model.values.iter().any(|m| m.value == *v))
        {
            return Err("enum_values must be a nonempty unique subset of the model enum".into());
        }
    }
    Ok(Property {
        target,
        source: binding.source.clone(),
        constraints,
        docs,
        required: binding.required,
        sensitive,
        immutable,
        enum_values: binding.enum_values.clone(),
        integer: binding.integer,
        timestamp_format,
    })
}

fn write_only(member: &Member) -> bool {
    matches!(
        member
            .traits
            .get(&ShapeId::new("aws.cloudformation", "cfnMutability"))
            .and_then(Value::as_str),
        Some("write" | "create")
    )
}

fn write_only_shape(service: &Service, id: &ShapeId, seen: &mut BTreeSet<ShapeId>) -> bool {
    if !seen.insert(id.clone()) {
        return false;
    }
    let mut hidden =
        |member: &Member| write_only(member) || write_only_shape(service, &member.target, seen);
    match service.shape(id) {
        Some(Shape::Structure(s) | Shape::Union(s)) => s.members.iter().any(&mut hidden),
        Some(Shape::List(s)) => hidden(&s.member),
        Some(Shape::Map(s)) => hidden(&s.key) || hidden(&s.value),
        _ => false,
    }
}

fn write_only_path(service: &Service, source: &MemberPath) -> bool {
    let mut target = &source.shape;
    for name in &source.path {
        let Some(Shape::Structure(s)) = service.shape(target) else {
            return false;
        };
        let Some(member) = s.member(name) else {
            return false;
        };
        if write_only(member) {
            return true;
        }
        target = &member.target;
    }
    write_only_shape(service, target, &mut BTreeSet::new())
}

fn sensitive_shape(service: &Service, id: &ShapeId, seen: &mut BTreeSet<ShapeId>) -> bool {
    if !seen.insert(id.clone()) {
        return false;
    }
    let Some(shape) = service.shape(id) else {
        return false;
    };
    if shape.sensitive() {
        return true;
    }
    let mut member_sensitive = |member: &Member| {
        service.sensitive(member) || sensitive_shape(service, &member.target, seen)
    };
    match shape {
        Shape::Structure(s) | Shape::Union(s) => s.members.iter().any(&mut member_sensitive),
        Shape::List(s) => member_sensitive(&s.member),
        Shape::Map(s) => member_sensitive(&s.key) || member_sensitive(&s.value),
        _ => false,
    }
}

/// Traverse structure members, preserving sensitivity of every enclosing member.
pub fn member_at<'a>(
    service: &'a Service,
    source: &MemberPath,
) -> Result<(&'a Member, bool), String> {
    if source.path.is_empty() {
        return Err("source path must name at least one member".into());
    }
    let mut target = &source.shape;
    let mut sensitive = service.shape(target).is_some_and(Shape::sensitive);
    let mut result = None;
    for name in &source.path {
        let Some(Shape::Structure(structure)) = service.shape(target) else {
            return Err(format!("source {target} is not a reachable structure"));
        };
        let member = structure
            .member(name)
            .ok_or_else(|| format!("source {target} has no member {name}"))?;
        sensitive |= service.sensitive(member);
        target = &member.target;
        result = Some(member);
    }
    Ok((result.expect("nonempty path"), sensitive))
}
