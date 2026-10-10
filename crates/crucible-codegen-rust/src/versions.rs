//! The older API versions a service still serves (docs/api-versions.md):
//! each one's release as its own module of types and operations, the
//! version constants handlers branch on, and the `'static` conversions the
//! framework applies between one version and the next.

use std::collections::BTreeSet;

use crucible_smithy::ir::{Operation, Service, Shape, VersionStep};
use crucible_smithy::{Diagnostics, ShapeId};

use crate::info::constant;
use crate::names::literal;

const RT: &str = "::crucible_server_runtime_rust::__generated";

/// The `versions` module, or nothing for a service with one version.
pub(crate) fn emit(
    service: &Service,
    options: &crate::Options,
    diagnostics: &mut Diagnostics,
) -> String {
    if service.versions.is_empty() {
        return String::new();
    }
    let current = service.versions.len();
    // Every version's model, oldest first, the current last.
    let models: Vec<&Service> = service
        .versions
        .iter()
        .map(|version| version.service.as_ref())
        .chain([service])
        .collect();
    let names: Vec<&str> = service
        .versions
        .iter()
        .map(|version| version.name.as_str())
        .chain([service.version.as_str()])
        .collect();

    let mut out = String::from(
        "/// The API versions this service serves, oldest first. A request names\n/// one in the `Crucible-Version` header; a handler that changed behaviour\n/// branches on `self.context().api_version()`.\npub mod versions {\n",
    );
    let mut constants = BTreeSet::new();
    for (order, name) in names.iter().enumerate() {
        let constant = version_constant(name);
        if !constants.insert(constant.clone()) {
            diagnostics.push(
                &service.id,
                format!("API version \"{name}\" has the same Rust name, {constant}, as another"),
            );
        }
        out.push_str(&format!(
            "    pub const {constant}: {RT}::versioning::ApiVersion = {RT}::versioning::ApiVersion::new({order}, {});\n",
            literal(name)
        ));
    }
    out.push_str(&format!(
        "    /// The version this build implements.\n    pub const CURRENT: {RT}::versioning::ApiVersion = {};\n",
        version_constant(names[current])
    ));

    // Each older release, generated as the current version is but without
    // the extras only the current version serves.
    for version in &service.versions {
        let old = version.service.as_ref();
        let old_options = crate::Options {
            mcp: false,
            explorer: None,
            validation_error: present(old, options.validation_error.as_ref()),
            internal_error: present(old, options.internal_error.as_ref()),
            unknown_operation_error: present(old, options.unknown_operation_error.as_ref()),
            strict_inputs: options.strict_inputs,
            require_rejection: false,
        };
        let body = crate::body(old, &old_options, old, diagnostics);
        out.push_str(&format!(
            "\n    /// Release `{}`, as it declared its operations and shapes.\n    pub mod {} {{\n{}    }}\n",
            version.name,
            version_module(&version.name),
            indent(&body, 2)
        ));
    }

    // One conversion per operation and version step, shared by every
    // older version's chain that crosses it.
    let mut conversions = String::new();
    let mut registrations = String::new();
    let mut built = BTreeSet::new();
    for (index, version) in service.versions.iter().enumerate() {
        let module = version_module(&version.name);
        let constant = version_constant(&version.name);
        for old_op in &version.service.operations {
            let Some(current_op) = operation(service, &old_op.id) else {
                // Not served at the current version: nothing to reach.
                continue;
            };
            let mut chain = Vec::new();
            for step in index..current {
                let name = format!("{}_{step}", constant_name(old_op));
                if built.insert(name.clone()) {
                    let conversion = conversion(&models, service, step, &old_op.id, diagnostics);
                    conversions.push_str(&format!(
                        "        pub(super) static {name}: Conversion = {conversion};\n"
                    ));
                }
                chain.push(format!("&{name}"));
            }
            // A static, so the chain is `'static` without promotion.
            let chain_name = format!("{}_FROM_{index}", constant_name(old_op));
            conversions.push_str(&format!(
                "        pub(super) static {chain_name}: &[&Conversion] = &[{}];\n",
                chain.join(", ")
            ));
            registrations.push_str(&format!(
                "\n            .add_versioned::<{module}::{old}, super::{new}>({constant}, __conversions::{chain_name})",
                old = old_op.name(),
                new = current_op.name(),
            ));
        }
    }
    out.push_str(&format!(
        "\n    #[allow(unused_imports)]\n    mod __conversions {{\n        use {RT}::versioning::{{Conversion, ErrorDowngrade, Seg, Transform, downgrade_to}};\n{conversions}    }}\n"
    ));
    let older: Vec<String> = service
        .versions
        .iter()
        .map(|version| {
            format!(
                "({}, {})",
                version_constant(&version.name),
                version
                    .sunset
                    .as_deref()
                    .map_or("None".to_owned(), |date| format!("Some({})", literal(date)))
            )
        })
        .collect();
    out.push_str(&format!(
        "\n    pub(super) fn register(builder: {RT}::OperationsBuilder) -> {RT}::OperationsBuilder {{\n        builder\n            .versions(CURRENT, &[{}]){registrations}\n    }}\n}}\n",
        older.join(", ")
    ));
    out
}

/// How `operation` crosses from version `step` to the next.
fn conversion(
    models: &[&Service],
    current: &Service,
    step: usize,
    operation: &ShapeId,
    diagnostics: &mut Diagnostics,
) -> String {
    let (old, new) = (models[step], models[step + 1]);
    let steps: &[VersionStep] = &current.versions[step].steps;
    let (Some(old_op), Some(new_op)) = (
        self::operation(old, operation),
        self::operation(new, operation),
    ) else {
        return refused("the operation is not in every later version");
    };
    for item in steps {
        if let VersionStep::Hooked { kind, operations } = item
            && operations.contains(operation)
        {
            return refused(&format!("a `{kind}` step needs a hook"));
        }
    }
    if streams(old_op) || streams(new_op) {
        return refused("streams are served at the current version only");
    }
    let mut upgrade = Vec::new();
    let mut downgrade = Vec::new();
    let mut errors = Vec::new();
    for item in steps {
        match item {
            VersionStep::Rename {
                old: old_shape,
                new: new_shape,
                from,
                to,
            } => {
                let (Some(from_wire), Some(to_wire)) =
                    (wire(old, old_shape, from), wire(new, new_shape, to))
                else {
                    continue;
                };
                for at in places(
                    old,
                    old_op.input.as_ref(),
                    old_shape,
                    operation,
                    diagnostics,
                ) {
                    upgrade.push((
                        at.len(),
                        format!(
                            "Transform::Rename {{ at: {}, from: {}, to: {} }}",
                            segs(&at),
                            literal(&from_wire),
                            literal(&to_wire)
                        ),
                    ));
                }
                for at in places(
                    new,
                    new_op.output.as_ref(),
                    new_shape,
                    operation,
                    diagnostics,
                ) {
                    downgrade.push((
                        at.len(),
                        format!(
                            "Transform::Rename {{ at: {}, from: {}, to: {} }}",
                            segs(&at),
                            literal(&to_wire),
                            literal(&from_wire)
                        ),
                    ));
                }
                for error in &new_op.errors {
                    if !places(new, Some(error), new_shape, operation, diagnostics).is_empty() {
                        diagnostics.push(
                            new_shape,
                            format!(
                                "a member renamed inside error {error} is not converted for API version {} yet",
                                current.versions[step].name
                            ),
                        );
                    }
                }
            }
            VersionStep::Require {
                new: new_shape,
                member,
                default,
            } => {
                let Some(member_wire) = wire(new, new_shape, member) else {
                    continue;
                };
                let value = serde_json::to_string(default).expect("JSON value");
                for at in places(
                    old,
                    old_op.input.as_ref(),
                    new_shape,
                    operation,
                    diagnostics,
                ) {
                    upgrade.push((
                        at.len(),
                        format!(
                            "Transform::Default {{ at: {}, member: {}, value: {} }}",
                            segs(&at),
                            literal(&member_wire),
                            literal(&value)
                        ),
                    ));
                }
            }
            VersionStep::AddEnumValue {
                new: new_shape,
                value,
                downgrade: replacement,
            } => {
                let (Some(from), Some(to)) = (
                    enum_value(new, new_shape, value),
                    enum_value(new, new_shape, replacement),
                ) else {
                    continue;
                };
                for at in places(
                    new,
                    new_op.output.as_ref(),
                    new_shape,
                    operation,
                    diagnostics,
                ) {
                    downgrade.push((
                        at.len(),
                        format!(
                            "Transform::Map {{ at: {}, from: {}, to: {} }}",
                            segs(&at),
                            literal(&from),
                            literal(&to)
                        ),
                    ));
                }
            }
            VersionStep::AddError {
                operation: target,
                error,
                downgrade: replacement,
                members,
            } if target == operation => {
                // Errors cross as the current version's types: the error the
                // older version knows is built as the current model declares it.
                let Some(Shape::Structure(older)) = current.shape(replacement) else {
                    diagnostics.push(
                        replacement,
                        format!(
                            "error {replacement}, which older clients get in place of {error}, is not in the current model"
                        ),
                    );
                    continue;
                };
                let pairs: Vec<String> = members
                    .iter()
                    .filter_map(|(to, from)| {
                        Some(format!(
                            "({}, {})",
                            literal(&older.member(to)?.wire_name),
                            literal(&wire(current, error, from)?)
                        ))
                    })
                    .collect();
                errors.push(format!(
                    "ErrorDowngrade {{ from: {}, to: downgrade_to::<super::super::{}>, members: &[{}] }}",
                    literal(&error.to_string()),
                    current.name_of(replacement),
                    pairs.join(", ")
                ));
            }
            _ => {}
        }
    }
    // Deepest places first: renaming a member never moves a place still to
    // be converted, which only deeper places pass through.
    upgrade.sort_by_key(|(depth, _)| std::cmp::Reverse(*depth));
    downgrade.sort_by_key(|(depth, _)| std::cmp::Reverse(*depth));
    let list = |items: Vec<(usize, String)>| {
        items
            .into_iter()
            .map(|(_, item)| item)
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "Conversion {{ upgrade: &[{}], downgrade: &[{}], errors: &[{}], refused: None }}",
        list(upgrade),
        list(downgrade),
        errors.join(", ")
    )
}

fn refused(reason: &str) -> String {
    format!(
        "Conversion {{ upgrade: &[], downgrade: &[], errors: &[], refused: Some({}) }}",
        literal(reason)
    )
}

fn streams(operation: &Operation) -> bool {
    [&operation.request, &operation.response]
        .iter()
        .any(|bindings| {
            bindings.event_stream.is_some()
                || bindings
                    .payload
                    .as_ref()
                    .is_some_and(|payload| payload.streaming)
        })
}

/// One step of a place in a value's JSON.
enum Step {
    Field(String),
    Items,
    Values,
}

/// Every place `target` sits in a value of `root`. A place inside a
/// recursive shape has no finite path; it is reported.
fn places(
    service: &Service,
    root: Option<&ShapeId>,
    target: &ShapeId,
    operation: &ShapeId,
    diagnostics: &mut Diagnostics,
) -> Vec<Vec<Step>> {
    let Some(root) = root else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut stack = Vec::new();
    let mut recursive = false;
    walk(
        service,
        root,
        target,
        &mut stack,
        &mut Vec::new(),
        &mut out,
        &mut recursive,
    );
    if recursive {
        diagnostics.push(
            target,
            format!(
                "{target} sits inside a recursive shape of {operation}; converting it between API versions is not supported yet"
            ),
        );
    }
    out
}

fn walk(
    service: &Service,
    shape: &ShapeId,
    target: &ShapeId,
    stack: &mut Vec<ShapeId>,
    path: &mut Vec<Step>,
    out: &mut Vec<Vec<Step>>,
    recursive: &mut bool,
) {
    if shape == target {
        out.push(
            path.iter()
                .map(|step| match step {
                    Step::Field(name) => Step::Field(name.clone()),
                    Step::Items => Step::Items,
                    Step::Values => Step::Values,
                })
                .collect(),
        );
    }
    if stack.contains(shape) {
        if reaches(service, shape, target, &mut BTreeSet::new()) {
            *recursive = true;
        }
        return;
    }
    stack.push(shape.clone());
    match service.shape(shape) {
        Some(Shape::Structure(structure) | Shape::Union(structure)) => {
            for member in &structure.members {
                path.push(Step::Field(member.wire_name.clone()));
                walk(service, &member.target, target, stack, path, out, recursive);
                path.pop();
            }
        }
        Some(Shape::List(list)) => {
            path.push(Step::Items);
            walk(
                service,
                &list.member.target,
                target,
                stack,
                path,
                out,
                recursive,
            );
            path.pop();
        }
        Some(Shape::Map(map)) => {
            path.push(Step::Values);
            walk(
                service,
                &map.value.target,
                target,
                stack,
                path,
                out,
                recursive,
            );
            path.pop();
        }
        _ => {}
    }
    stack.pop();
}

/// Whether a value of `shape` can hold a `target`.
fn reaches(
    service: &Service,
    shape: &ShapeId,
    target: &ShapeId,
    seen: &mut BTreeSet<ShapeId>,
) -> bool {
    if shape == target {
        return true;
    }
    if !seen.insert(shape.clone()) {
        return false;
    }
    match service.shape(shape) {
        Some(Shape::Structure(structure) | Shape::Union(structure)) => structure
            .members
            .iter()
            .any(|member| reaches(service, &member.target, target, seen)),
        Some(Shape::List(list)) => reaches(service, &list.member.target, target, seen),
        Some(Shape::Map(map)) => reaches(service, &map.value.target, target, seen),
        _ => false,
    }
}

fn segs(path: &[Step]) -> String {
    let items: Vec<String> = path
        .iter()
        .map(|step| match step {
            Step::Field(name) => format!("Seg::Field({})", literal(name)),
            Step::Items => "Seg::Items".to_owned(),
            Step::Values => "Seg::Values".to_owned(),
        })
        .collect();
    format!("&[{}]", items.join(", "))
}

/// The JSON name of `member` of structure `shape`.
fn wire(service: &Service, shape: &ShapeId, member: &str) -> Option<String> {
    match service.shape(shape)? {
        Shape::Structure(structure) | Shape::Union(structure) => {
            Some(structure.member(member)?.wire_name.clone())
        }
        _ => None,
    }
}

/// The wire value of enum `shape`'s member `name`.
fn enum_value(service: &Service, shape: &ShapeId, name: &str) -> Option<String> {
    match service.shape(shape)? {
        Shape::Enum(values) => values
            .values
            .iter()
            .find(|value| value.name == name)
            .map(|value| value.value.clone()),
        _ => None,
    }
}

fn operation<'a>(service: &'a Service, id: &ShapeId) -> Option<&'a Operation> {
    service
        .operations
        .iter()
        .find(|operation| &operation.id == id)
}

fn present(service: &Service, id: Option<&ShapeId>) -> Option<ShapeId> {
    id.filter(|id| service.shape(id).is_some()).cloned()
}

fn constant_name(operation: &Operation) -> String {
    constant(operation.name())
}

/// `2026-09-04` as `V2026_09_04`.
pub(crate) fn version_constant(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    if !out.starts_with(|c: char| c.is_ascii_alphabetic()) {
        out.insert(0, 'V');
    }
    out
}

/// `2026-09-04` as `v2026_09_04`.
fn version_module(name: &str) -> String {
    crate::names::escape_identifier(&version_constant(name).to_ascii_lowercase())
}

fn indent(code: &str, levels: usize) -> String {
    let pad = "    ".repeat(levels);
    code.lines()
        .map(|line| {
            if line.is_empty() {
                "\n".to_owned()
            } else {
                format!("{pad}{line}\n")
            }
        })
        .collect()
}
