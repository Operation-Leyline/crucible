//! The older API versions a service still serves (docs/api-versions.md):
//! each one's release as its own module of types and operations, the
//! version constants handlers branch on, the `'static` conversions the
//! framework applies between one version and the next, and the `Migration`
//! trait of each migration whose steps need the service's hooks.

use std::collections::{BTreeMap, BTreeSet};

use crucible_smithy::ir::{Hook, Operation, Service, Shape, VersionStep};
use crucible_smithy::{Diagnostics, ShapeId};

use crate::info::constant;
use crate::names::{function, literal};

const RT: &str = "::crucible_server_runtime_rust::__generated";

/// One hook a migration's `Migration` trait declares.
struct HookMethod {
    /// The method, as the trait declares it.
    decl: String,
    /// Its registration with the framework, in the `versions` module.
    register: String,
}

/// The hooks of each migration, by its older version's index, then by
/// method name.
type Methods = Vec<BTreeMap<String, HookMethod>>;

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
    let mut constants = BTreeSet::from(["CURRENT".to_owned()]);
    for (order, name) in names.iter().enumerate() {
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_graphic()) {
            diagnostics.push(
                &service.id,
                format!(
                    "API version \"{name}\" travels in a header: use visible ASCII without spaces"
                ),
            );
        }
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

    // One conversion per operation and version step, shared by every
    // older version's chain that crosses it.
    let mut methods: Methods = (0..current).map(|_| BTreeMap::new()).collect();
    let mut steps = String::new();
    let mut chains = String::new();
    let mut registrations = String::new();
    let mut built = BTreeSet::new();
    for (index, version) in service.versions.iter().enumerate() {
        let module = version_module(&version.name);
        let constant = version_constant(&version.name);
        for old_op in &version.service.operations {
            // The last version that has the operation serves it.
            let mut last = index;
            while last < current && operation(models[last + 1], &old_op.id).is_some() {
                last += 1;
            }
            let served_by = if last == current {
                None
            } else if service.versions[last].steps.iter().any(|step| {
                matches!(
                    step,
                    VersionStep::Hooked { hook: Hook::RemoveOperation { operation }, .. }
                        if operation == &old_op.id
                )
            }) {
                // The handler takes the last version's types.
                let at_last = operation(models[last], &old_op.id).expect("the last version has it");
                if streams(old_op) || streams(at_last) {
                    // Streams cross no JSON conversion: not served.
                    continue;
                }
                Some(legacy(&models, last, at_last, &mut methods, diagnostics))
            } else {
                // Gone without a handler: the guard refuses such a model.
                continue;
            };
            let mut chain = Vec::new();
            for step in index..last {
                let name = format!("{}_{step}", constant_name(old_op));
                let name = format!("steps::{name}");
                if built.insert(name.clone()) {
                    let conversion = conversion(
                        &models,
                        service,
                        step,
                        &old_op.id,
                        &mut methods,
                        diagnostics,
                    );
                    steps.push_str(&format!(
                        "            pub(in super::super) static {}: Conversion = {conversion};\n",
                        name.trim_start_matches("steps::")
                    ));
                }
                chain.push(format!("&super::{name}"));
            }
            // A static, so the chain is `'static` without promotion.
            let chain_name = format!("chains::{}_{index}", constant_name(old_op));
            chains.push_str(&format!(
                "            pub(in super::super) static {}: &[&Conversion] = &[{}];\n",
                chain_name.trim_start_matches("chains::"),
                chain.join(", ")
            ));
            let (serving, served_by) = match served_by {
                None => (format!("super::{}", old_op.name()), "None".to_owned()),
                Some(key) => (
                    format!(
                        "{}::{}",
                        version_module(&service.versions[last].name),
                        old_op.name()
                    ),
                    format!(
                        "Some(({}, {}))",
                        version_constant(&service.versions[last].name),
                        literal(&key)
                    ),
                ),
            };
            registrations.push_str(&format!(
                "\n            .add_versioned::<{module}::{old}, {serving}>({constant}, __conversions::{chain_name}, {served_by})",
                old = old_op.name(),
            ));
        }
    }

    // Each older release, generated as the current version is but without
    // the extras only the current version serves, with the hooks its
    // migration to the next version needs.
    for (index, version) in service.versions.iter().enumerate() {
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
        // A module inside the current server: the release it serves is the
        // `versions` table's, not one of its own.
        let mut module = old.clone();
        module.api_version = None;
        let parent = older_requirements(service, &mut module, version, diagnostics);
        let mut body = crate::body_with(&module, &old_options, &module, false, parent, diagnostics);
        if !methods[index].is_empty()
            && (old.shapes.keys().any(|id| id.name() == "Migration")
                || old.operations.iter().any(|op| op.name() == "Migration"))
        {
            diagnostics.push(
                &service.id,
                format!(
                    "release `{}` declares `Migration`, the name of the trait its migration's hooks generate; rename it in the current model and the migration",
                    version.name
                ),
            );
        }
        if !methods[index].is_empty() {
            let decls: String = methods[index]
                .values()
                .map(|method| method.decl.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            body.push_str(&format!(
                "\n/// The hooks the migration from `{}` to `{}` declares: the\n/// conversions the framework cannot write itself. `operations(..)`\n/// requires them; a split builder registers them with `{}_migration`.\npub trait Migration: Send + Sync + 'static {{\n{decls}}}\n",
                version.name,
                names[index + 1],
                version_module(&version.name),
            ));
        }
        out.push_str(&format!(
            "\n    /// Release `{}`, as it declared its operations and shapes.\n    pub mod {} {{\n{}    }}\n",
            version.name,
            version_module(&version.name),
            indent(&body, 2)
        ));
    }

    out.push_str(&format!(
        "\n    #[allow(unused_imports)]\n    mod __conversions {{\n        use {RT}::versioning::Conversion;\n        pub(super) mod steps {{\n            use {RT}::versioning::{{Conversion, ErrorDowngrade, Seg, Transform, downgrade_to}};\n{steps}        }}\n        pub(super) mod chains {{\n            use {RT}::versioning::Conversion;\n{chains}        }}\n    }}\n"
    ));
    let older: Vec<String> = service
        .versions
        .iter()
        .map(|version| {
            format!(
                "({}, {}, &{}::SERVICE)",
                version_constant(&version.name),
                version
                    .sunset
                    .as_deref()
                    .map_or("None".to_owned(), |date| format!("Some({})", literal(date))),
                version_module(&version.name)
            )
        })
        .collect();
    out.push_str(&format!(
        "\n    /// The older versions, oldest first: sunset and release.\n    static OLDER: &[({RT}::versioning::ApiVersion, ::std::option::Option<&str>, &{RT}::ServiceInfo)] = &[{}];\n\n    pub(super) fn register(builder: {RT}::OperationsBuilder) -> {RT}::OperationsBuilder {{\n        builder\n            .versions(CURRENT, OLDER){registrations}\n    }}\n",
        older.join(", ")
    ));
    out.push_str(&registration(service, &methods, diagnostics));
    out.push_str("}\n");
    out
}

/// The `Migrations` trait every hook adds up to, and the builder methods
/// that register them.
/// What an older release's module states about authorization
/// (docs/permissions.md §7.3). A converted operation states nothing: its
/// requests are authorized by the current requirements, which bind the
/// current input they are converted to. An operation the next release
/// removes is served by a legacy handler on this release's input, so it
/// keeps its own requirements and `CHECKS`, naming the current
/// `permissions` statics: whether the module then reads `super::permissions`.
fn older_requirements(
    current: &Service,
    module: &mut Service,
    version: &crucible_smithy::ir::ApiVersion,
    diagnostics: &mut Diagnostics,
) -> bool {
    let removed: BTreeSet<&ShapeId> = version
        .steps
        .iter()
        .filter_map(|step| match step {
            VersionStep::Hooked {
                hook: Hook::RemoveOperation { operation },
                ..
            } => Some(operation),
            _ => None,
        })
        .collect();
    let old = std::mem::take(&mut module.permissions);
    module.permission_types = None;
    let mut kept = false;
    for operation in &mut module.operations {
        if !removed.contains(&operation.id) || streams(operation) {
            operation.requirements = Default::default();
            continue;
        }
        let (id, name) = (operation.id.clone(), operation.name().to_owned());
        if let crucible_smithy::ir::Requirements::List(clauses) = &mut operation.requirements {
            for clause in clauses.iter_mut() {
                match current_permission(current, &old, clause) {
                    Ok((resource, symbol)) => {
                        clause.resource = resource;
                        clause.permission = symbol;
                    }
                    Err(problem) => diagnostics.push(
                        &id,
                        format!(
                            "release `{}` serves `{name}` with a legacy handler, which requires {problem}",
                            version.name,
                        ),
                    ),
                }
            }
        }
        kept |= !matches!(
            operation.requirements,
            crucible_smithy::ir::Requirements::Undeclared
        );
    }
    if kept {
        module.permissions = current.permissions.clone();
        for id in current.permissions.keys() {
            module
                .renames
                .insert(id.clone(), current.name_of(id).to_owned());
        }
    }
    kept
}

/// The current model's resource and primary symbol for an older clause,
/// matched by authorization name and stored value.
fn current_permission(
    current: &Service,
    old: &BTreeMap<ShapeId, crucible_smithy::ir::ResourcePermissions>,
    clause: &crucible_smithy::ir::Requirement,
) -> Result<(ShapeId, String), String> {
    let resource = old
        .get(&clause.resource)
        .ok_or_else(|| format!("permissions of `{}`, which declares none", clause.resource))?;
    let permission = resource
        .permission(&clause.permission)
        .map(|p| crucible_codegen_common::permissions::primary(resource, p))
        .ok_or_else(|| format!("`{}` on `{}`", clause.permission, resource.name))?;
    let gone = || {
        format!(
            "`{}` on `{}`, which the current model no longer declares",
            permission.value, resource.name
        )
    };
    let (id, now) = current
        .permissions
        .iter()
        .find(|(_, now)| now.name == resource.name)
        .ok_or_else(gone)?;
    let symbol = now
        .permissions
        .iter()
        .find(|p| p.value == permission.value)
        .map(|p| {
            crucible_codegen_common::permissions::primary(now, p)
                .symbol
                .clone()
        })
        .ok_or_else(gone)?;
    if clause.mode != crucible_smithy::ir::Mode::Handler
        && now.identifiers.len() != resource.identifiers.len()
    {
        return Err(format!(
            "`{}` on `{}`, whose identifiers have changed since",
            permission.value, resource.name
        ));
    }
    Ok((id.clone(), symbol))
}

fn registration(service: &Service, methods: &Methods, diagnostics: &mut Diagnostics) -> String {
    let hooked: Vec<(usize, String)> = methods
        .iter()
        .enumerate()
        .filter(|(_, methods)| !methods.is_empty())
        .map(|(index, _)| (index, version_module(&service.versions[index].name)))
        .collect();
    let operation_methods: BTreeSet<String> = service
        .operations
        .iter()
        .map(|op| function(op.name()))
        .collect();
    for name in ["migrations".to_owned()].into_iter().chain(
        hooked
            .iter()
            .map(|(_, module)| format!("{module}_migration")),
    ) {
        if operation_methods.contains(&name) {
            diagnostics.push(
                &service.id,
                format!("an operation's builder method `{name}` collides with the migration hooks' method"),
            );
        }
    }
    let bounds: Vec<String> = hooked
        .iter()
        .map(|(_, module)| format!("{module}::Migration"))
        .collect();
    let mut out = String::new();
    if bounds.is_empty() {
        out.push_str(
            "\n    /// No migration needs a hook.\n    pub trait Migrations {}\n    impl<T: ?Sized> Migrations for T {}\n",
        );
    } else {
        out.push_str(&format!(
            "\n    /// Every migration's hooks, for a service that writes them on one type.\n    pub trait Migrations: {bounds} {{}}\n    impl<T: {bounds}> Migrations for T {{}}\n",
            bounds = bounds.join(" + ")
        ));
    }
    let mut register_all = "builder".to_owned();
    let mut builder_methods = String::new();
    for (index, module) in &hooked {
        let hooks: String = methods[*index]
            .values()
            .map(|method| method.register.as_str())
            .collect();
        out.push_str(&format!(
            "\n    fn {module}_hooks<S: {module}::Migration>(builder: {RT}::OperationsBuilder, implementation: ::std::sync::Arc<S>) -> {RT}::OperationsBuilder {{\n        builder{hooks}\n    }}\n"
        ));
        register_all = format!("{module}_hooks({register_all}, implementation.clone())");
        builder_methods.push_str(&format!(
            "\n        /// Serves the hooks of the migration from `{}`.\n        pub fn {module}_migration<S: {module}::Migration>(self, implementation: ::std::sync::Arc<S>) -> Self {{\n            Self({module}_hooks(self.0, implementation))\n        }}\n",
            service.versions[*index].name
        ));
    }
    out.push_str(&format!(
        "\n    #[allow(unused_variables)]\n    fn hooks<S: Migrations + Send + Sync + 'static>(builder: {RT}::OperationsBuilder, implementation: ::std::sync::Arc<S>) -> {RT}::OperationsBuilder {{\n        {register_all}\n    }}\n\n    impl super::OperationsBuilder {{\n        /// Serves every migration's hooks with one implementation.\n        pub fn migrations<S: Migrations + Send + Sync + 'static>(self, implementation: ::std::sync::Arc<S>) -> Self {{\n            Self(hooks(self.0, implementation))\n        }}\n{builder_methods}    }}\n"
    ));
    out
}

/// Where version `index`'s types are, seen from the `versions` module.
fn module_path(models: &[&Service], current: &Service, index: usize) -> String {
    if index + 1 == models.len() {
        "super::".to_owned()
    } else {
        format!("{}::", version_module(&current.versions[index].name))
    }
}

/// The Rust type of shape `id` in version `index`'s model, seen from the
/// `versions` module and from inside a version's module.
fn type_paths(
    models: &[&Service],
    current: &Service,
    index: usize,
    id: &ShapeId,
) -> (String, String) {
    let prefix = module_path(models, current, index);
    (
        crate::types::rust_type(models[index], id, &prefix),
        crate::types::rust_type(models[index], id, &format!("super::{prefix}")),
    )
}

/// The registry key of a hook, which also names it when it is missing.
fn hook_key(current: &Service, step: usize, method: &str) -> String {
    format!(
        "versions::{}::Migration::{method}",
        version_module(&current.versions[step].name)
    )
}

/// Records hook `method` of the migration from version `step`, converting
/// `input` (a value of version `from`) into `output` (of version `to`).
#[allow(clippy::too_many_arguments)]
fn hook_method(
    models: &[&Service],
    current: &Service,
    methods: &mut Methods,
    step: usize,
    method: &str,
    doc: &str,
    input: (usize, &ShapeId),
    output: (usize, &ShapeId),
    is_async: bool,
    diagnostics: &mut Diagnostics,
) -> String {
    let key = hook_key(current, step, method);
    let (input_outer, input_inner) = type_paths(models, current, input.0, input.1);
    let (_, output_inner) = type_paths(models, current, output.0, output.1);
    let (returns, wait) = if is_async {
        (
            format!("impl ::std::future::Future<Output = {RT}::Result<{output_inner}>> + Send"),
            ".await",
        )
    } else {
        (format!("{RT}::Result<{output_inner}>"), "")
    };
    let decl =
        format!("    /// {doc}\n    fn {method}(&self, value: {input_inner}) -> {returns};\n");
    if let Some(existing) = methods[step].get(method) {
        if existing.decl != decl {
            collision(current, step, method, input.1, diagnostics);
        }
        return key;
    }
    let register = format!(
        "\n            .hook({key}, ::std::sync::Arc::new({{\n                let implementation = implementation.clone();\n                move |value| {{\n                    let implementation = implementation.clone();\n                    ::std::boxed::Box::pin(async move {{\n                        let value: {input_outer} = {RT}::versioning::hook_input({key}, value)?;\n                        let value = implementation.{method}(value){wait}?;\n                        {RT}::versioning::hook_output({key}, value)\n                    }})\n                }}\n            }}))",
        key = literal(&key),
    );
    methods[step].insert(method.to_owned(), HookMethod { decl, register });
    key
}

/// Records the handler that serves `operation` at version `last` and
/// older, the last version that had it; returns its key.
fn legacy(
    models: &[&Service],
    last: usize,
    operation: &Operation,
    methods: &mut Methods,
    diagnostics: &mut Diagnostics,
) -> String {
    let current = models[models.len() - 1];
    let old = models[last];
    let method = function(operation.name());
    let key = hook_key(current, last, &method);
    if methods[last].contains_key(&method) {
        // The same operation's handler, recorded for an older version, or
        // a hook that happens to share its name.
        if !methods[last][&method].register.contains(".add_legacy::<") {
            collision(current, last, &method, &operation.id, diagnostics);
        }
        return key;
    }
    let module = version_module(&current.versions[last].name);
    let input = operation
        .input
        .as_ref()
        .map(|id| old.name_of(id).to_owned());
    let output = operation.output.as_ref().map_or("()".to_owned(), |id| {
        format!("super::{module}::{}", old.name_of(id))
    });
    let (param, call) = match &input {
        Some(input) => (
            format!(", input: super::{module}::{input}"),
            format!(
                "|implementation, input| async move {{ implementation.{method}(input).await }}"
            ),
        ),
        None => (
            String::new(),
            format!("|implementation, _| async move {{ implementation.{method}().await }}"),
        ),
    };
    let decl = format!(
        "    /// Serves `{}`, which the next version removed, for this version and older.\n    fn {method}(&self{param}) -> impl ::std::future::Future<Output = {RT}::Result<{output}>> + Send;\n",
        operation.name()
    );
    let register = format!(
        "\n            .add_legacy::<{module}::{name}, S, _>({constant}, &{module}::SERVICE, implementation.clone(), {call})",
        name = operation.name(),
        constant = version_constant(&current.versions[last].name),
    );
    methods[last].insert(method, HookMethod { decl, register });
    key
}

/// One conversion of a JSON value at a place: `hook` marks one that calls
/// the service, which owns everything inside its place.
struct Item {
    at: Vec<Step>,
    text: String,
    hook: bool,
    /// A `require .. by hook`: adds one member, reading the older value
    /// as it arrived, so it runs before the step's other conversions.
    fill: bool,
}

/// How `operation` crosses from version `step` to the next.
fn conversion(
    models: &[&Service],
    current: &Service,
    step: usize,
    operation: &ShapeId,
    methods: &mut Methods,
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
    if streams(old_op) || streams(new_op) {
        return refused("streams are served at the current version only");
    }
    let mut upgrade: Vec<Item> = Vec::new();
    let mut downgrade: Vec<Item> = Vec::new();
    let mut errors = Vec::new();
    let version = &current.versions[step].name;
    let inside_error = |shape: &ShapeId, what: &str, diagnostics: &mut Diagnostics| {
        for error in &new_op.errors {
            if !places(new, Some(error), shape, operation, diagnostics).is_empty() {
                diagnostics.push(
                    shape,
                    format!(
                        "{what} inside error {error} is not converted for API version {version} yet"
                    ),
                );
            }
        }
    };
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
                    let text = format!(
                        "Transform::Rename {{ at: {}, from: {}, to: {} }}",
                        segs(&at),
                        literal(&from_wire),
                        literal(&to_wire)
                    );
                    upgrade.push(Item {
                        at,
                        text,
                        hook: false,
                        fill: false,
                    });
                }
                for at in places(
                    new,
                    new_op.output.as_ref(),
                    new_shape,
                    operation,
                    diagnostics,
                ) {
                    let text = format!(
                        "Transform::Rename {{ at: {}, from: {}, to: {} }}",
                        segs(&at),
                        literal(&to_wire),
                        literal(&from_wire)
                    );
                    downgrade.push(Item {
                        at,
                        text,
                        hook: false,
                        fill: false,
                    });
                }
                inside_error(new_shape, "a member renamed", diagnostics);
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
                    let text = format!(
                        "Transform::Default {{ at: {}, member: {}, value: {} }}",
                        segs(&at),
                        literal(&member_wire),
                        literal(&value)
                    );
                    upgrade.push(Item {
                        at,
                        text,
                        hook: false,
                        fill: false,
                    });
                }
            }
            VersionStep::AddEnumValue {
                new: new_shape,
                value,
                downgrade: replacement,
            } => {
                if !matches!(new.shape(new_shape), Some(Shape::Enum(_))) {
                    diagnostics.push(
                        new_shape,
                        format!(
                            "`add {}.{value} downgrade ..` on a union or intEnum is not converted for API version {version} yet",
                            new_shape.name()
                        ),
                    );
                    continue;
                }
                // The value is the next version's; the downgrade, this one's.
                let (Some(from), Some(to)) = (
                    enum_value(new, new_shape, value),
                    enum_value(old, new_shape, replacement),
                ) else {
                    continue;
                };
                if keyed(new, new_op.output.as_ref(), new_shape) {
                    diagnostics.push(
                        new_shape,
                        format!(
                            "{} keys a map in {operation}'s output; map keys are not converted for API version {version} yet",
                            new_shape.name()
                        ),
                    );
                }
                inside_error(new_shape, "an added enum value", diagnostics);
                for at in places(
                    new,
                    new_op.output.as_ref(),
                    new_shape,
                    operation,
                    diagnostics,
                ) {
                    let text = format!(
                        "Transform::Map {{ at: {}, from: {}, to: {} }}",
                        segs(&at),
                        literal(&from),
                        literal(&to)
                    );
                    downgrade.push(Item {
                        at,
                        text,
                        hook: false,
                        fill: false,
                    });
                }
            }
            VersionStep::AddError {
                operation: target,
                error,
                downgrade: replacement,
                members,
            } if target == operation => {
                if models[step + 1..]
                    .iter()
                    .any(|model| self::operation(model, operation).is_none())
                {
                    // A legacy handler answers with its release's errors,
                    // which the current model may not declare.
                    diagnostics.push(
                        error,
                        format!(
                            "an error downgrade on {}, which a later version removes, is not converted yet",
                            operation.name()
                        ),
                    );
                    continue;
                }
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
                let mut pairs = Vec::new();
                for (to, from) in members {
                    if let (Some(target), Some(source)) =
                        (older.member(to), member_of(current, error, from))
                        && target.required
                        && target.default.is_none()
                        && !(source.required || source.default.is_some())
                    {
                        diagnostics.push(
                            replacement,
                            format!(
                                "{} requires `{to}`, but `{from}` is optional on {}; an error without it could not reach older clients",
                                replacement.name(),
                                error.name()
                            ),
                        );
                    }
                    match (older.member(to), wire(current, error, from)) {
                        (Some(to), Some(from)) => pairs.push(format!(
                            "({}, {})",
                            literal(&to.wire_name),
                            literal(&from)
                        )),
                        _ => diagnostics.push(
                            replacement,
                            format!(
                                "the current model has no `{to}` on {} or no `{from}` on {}, which the step from API version {version} maps",
                                replacement.name(),
                                error.name()
                            ),
                        ),
                    }
                }
                let unfilled: Vec<&str> = older
                    .members
                    .iter()
                    .filter(|member| member.required && member.default.is_none())
                    .filter(|member| !members.iter().any(|(to, _)| to == &member.name))
                    .map(|member| member.name.as_str())
                    .collect();
                if !unfilled.is_empty() {
                    diagnostics.push(
                        replacement,
                        format!(
                            "{} now requires `{}`, which the step from API version {version} does not fill; older clients could not be sent it",
                            replacement.name(),
                            unfilled.join("`, `")
                        ),
                    );
                }
                errors.push(format!(
                    "ErrorDowngrade {{ from: {}, to: downgrade_to::<super::super::super::{}>, members: &[{}] }}",
                    literal(&error.to_string()),
                    current.name_of(replacement),
                    pairs.join(", ")
                ));
            }
            VersionStep::Hooked {
                kind,
                hook,
                is_async,
                ..
            } => match hook {
                Hook::Reshape {
                    old: old_shape,
                    new: new_shape,
                } => {
                    let name = function(old_shape.name());
                    let ups = places(
                        old,
                        old_op.input.as_ref(),
                        old_shape,
                        operation,
                        diagnostics,
                    );
                    if !ups.is_empty() {
                        let key = hook_method(
                            models,
                            current,
                            methods,
                            step,
                            &format!("upgrade_{name}"),
                            &format!(
                                "`reshape {}`: an older request's value, as the next version reads it.",
                                old_shape.name()
                            ),
                            (step, old_shape),
                            (step + 1, new_shape),
                            *is_async,
                            diagnostics,
                        );
                        for at in ups {
                            let text = format!(
                                "Transform::Hook {{ at: {}, hook: {} }}",
                                segs(&at),
                                literal(&key)
                            );
                            upgrade.push(Item {
                                at,
                                text,
                                hook: true,
                                fill: false,
                            });
                        }
                    }
                    let downs = places(
                        new,
                        new_op.output.as_ref(),
                        new_shape,
                        operation,
                        diagnostics,
                    );
                    if !downs.is_empty() {
                        let key = hook_method(
                            models,
                            current,
                            methods,
                            step,
                            &format!("downgrade_{name}"),
                            &format!(
                                "`reshape {}`: a response's value, as this version's clients read it.",
                                old_shape.name()
                            ),
                            (step + 1, new_shape),
                            (step, old_shape),
                            *is_async,
                            diagnostics,
                        );
                        for at in downs {
                            let text = format!(
                                "Transform::Hook {{ at: {}, hook: {} }}",
                                segs(&at),
                                literal(&key)
                            );
                            downgrade.push(Item {
                                at,
                                text,
                                hook: true,
                                fill: false,
                            });
                        }
                    }
                    inside_error(new_shape, "a reshaped structure", diagnostics);
                }
                Hook::Retype {
                    old: old_shape,
                    new: new_shape,
                    member,
                } => {
                    let (Some(old_member), Some(new_member)) = (
                        member_of(old, old_shape, member),
                        member_of(new, new_shape, member),
                    ) else {
                        continue;
                    };
                    if crate::types::codec(old, old_member).is_some()
                        || crate::types::codec(new, new_member).is_some()
                    {
                        diagnostics.push(
                            &new_shape.with_member(member),
                            "a retyped member with a wire encoding of its own (a timestamp, blob or sparse collection) is not converted by a hook yet",
                        );
                        continue;
                    }
                    let name = format!("{}_{}", function(old_shape.name()), function(member));
                    let doc = format!("`retype {}.{member}`", old_shape.name());
                    let ups = places(
                        old,
                        old_op.input.as_ref(),
                        old_shape,
                        operation,
                        diagnostics,
                    );
                    if !ups.is_empty() {
                        let key = hook_method(
                            models,
                            current,
                            methods,
                            step,
                            &format!("upgrade_{name}"),
                            &format!(
                                "{doc}: an older request's value, as the next version reads it."
                            ),
                            (step, &old_member.target),
                            (step + 1, &new_member.target),
                            *is_async,
                            diagnostics,
                        );
                        for mut at in ups {
                            at.push(Step::Field(old_member.wire_name.clone()));
                            let text = format!(
                                "Transform::Hook {{ at: {}, hook: {} }}",
                                segs(&at),
                                literal(&key)
                            );
                            upgrade.push(Item {
                                at,
                                text,
                                hook: true,
                                fill: false,
                            });
                        }
                    }
                    let downs = places(
                        new,
                        new_op.output.as_ref(),
                        new_shape,
                        operation,
                        diagnostics,
                    );
                    if !downs.is_empty() {
                        let key = hook_method(
                            models,
                            current,
                            methods,
                            step,
                            &format!("downgrade_{name}"),
                            &format!(
                                "{doc}: a response's value, as this version's clients read it."
                            ),
                            (step + 1, &new_member.target),
                            (step, &old_member.target),
                            *is_async,
                            diagnostics,
                        );
                        for mut at in downs {
                            at.push(Step::Field(new_member.wire_name.clone()));
                            let text = format!(
                                "Transform::Hook {{ at: {}, hook: {} }}",
                                segs(&at),
                                literal(&key)
                            );
                            downgrade.push(Item {
                                at,
                                text,
                                hook: true,
                                fill: false,
                            });
                        }
                    }
                    inside_error(new_shape, "a retyped member", diagnostics);
                }
                Hook::Require {
                    old: old_shape,
                    new: new_shape,
                    member,
                } => {
                    let Some(new_member) = member_of(new, new_shape, member) else {
                        continue;
                    };
                    // The fill runs first and writes the next version's
                    // value; another step on the same member would read it
                    // as the older one.
                    let conflicts = steps.iter().any(|other| match other {
                        VersionStep::Rename { new, to, .. } => new == new_shape && to == member,
                        VersionStep::Hooked {
                            hook: Hook::Retype { new, member: m, .. },
                            ..
                        } => new == new_shape && m == member,
                        _ => false,
                    });
                    if conflicts {
                        diagnostics.push(
                            &new_shape.with_member(member),
                            format!(
                                "`{}.{member}` is both required by hook and renamed or retyped in the migration from API version {version}; declare one step for it",
                                new_shape.name()
                            ),
                        );
                        continue;
                    }
                    if crate::types::codec(new, new_member).is_some() {
                        diagnostics.push(
                            &new_shape.with_member(member),
                            "a member with a wire encoding of its own (a timestamp, blob or sparse collection) is not filled by a hook yet",
                        );
                        continue;
                    }
                    let ups = places(
                        old,
                        old_op.input.as_ref(),
                        old_shape,
                        operation,
                        diagnostics,
                    );
                    if !ups.is_empty() {
                        let key = hook_method(
                            models,
                            current,
                            methods,
                            step,
                            &format!(
                                "require_{}_{}",
                                function(old_shape.name()),
                                function(member)
                            ),
                            &format!(
                                "`require {}.{member} by hook`: the value an older request could not send, from the rest of it.",
                                old_shape.name()
                            ),
                            (step, old_shape),
                            (step + 1, &new_member.target),
                            *is_async,
                            diagnostics,
                        );
                        for at in ups {
                            let text = format!(
                                "Transform::Fill {{ at: {}, member: {}, hook: {} }}",
                                segs(&at),
                                literal(&new_member.wire_name),
                                literal(&key)
                            );
                            upgrade.push(Item {
                                at,
                                text,
                                hook: false,
                                fill: true,
                            });
                        }
                    }
                }
                Hook::RemoveOperation { .. } => {}
                Hook::Conversion => {
                    diagnostics.push(
                        operation,
                        format!(
                            "a `{kind}` step without `by hook` needs a declared `conversion`, which is not generated yet; add `by hook` to the step"
                        ),
                    );
                    return refused("a conversion is not generated yet");
                }
            },
            _ => {}
        }
    }
    format!(
        "Conversion {{ upgrade: &[{}], downgrade: &[{}], errors: &[{}], refused: None }}",
        ordered(upgrade),
        ordered(downgrade),
        errors.join(", ")
    )
}

/// The transforms in the order they run. A `reshape` or `retype` hook owns
/// everything inside its place, so what it covers is dropped. Fills run
/// first, outermost first, on the value as it arrived; then the rest,
/// deepest places first, so renaming a member never moves a place still
/// to be converted.
fn ordered(mut items: Vec<Item>) -> String {
    let hooks: Vec<Vec<Step>> = items
        .iter()
        .filter(|item| item.hook)
        .map(|item| item.at.clone())
        .collect();
    items.retain(|item| {
        !hooks
            .iter()
            .any(|at| item.at.starts_with(at) && (item.at.len() > at.len() || !item.hook))
    });
    items.sort_by_key(|item| {
        let depth = item.at.len() as isize;
        if item.fill { (0, depth) } else { (1, -depth) }
    });
    items
        .into_iter()
        .map(|item| item.text)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Member `name` of structure `shape`.
fn member_of<'s>(
    service: &'s Service,
    shape: &ShapeId,
    name: &str,
) -> Option<&'s crucible_smithy::ir::Member> {
    match service.shape(shape)? {
        Shape::Structure(structure) | Shape::Union(structure) => structure.member(name),
        _ => None,
    }
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
#[derive(Clone, PartialEq)]
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

/// Two hooked steps of one migration need one method name for different
/// conversions.
fn collision(
    current: &Service,
    step: usize,
    method: &str,
    subject: &ShapeId,
    diagnostics: &mut Diagnostics,
) {
    diagnostics.push(
        subject,
        format!(
            "two steps of the migration from `{}` need a hook named `{method}` for different conversions; rename one of the shapes to tell them apart",
            current.versions[step].name
        ),
    );
}

/// Whether a map reachable from `root` is keyed by `target`.
fn keyed(service: &Service, root: Option<&ShapeId>, target: &ShapeId) -> bool {
    let Some(root) = root else {
        return false;
    };
    let mut stack = vec![root.clone()];
    let mut seen = BTreeSet::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        match service.shape(&id) {
            Some(Shape::Structure(structure) | Shape::Union(structure)) => {
                stack.extend(structure.members.iter().map(|m| m.target.clone()));
            }
            Some(Shape::List(list)) => stack.push(list.member.target.clone()),
            Some(Shape::Map(map)) => {
                if &map.key.target == target {
                    return true;
                }
                stack.push(map.value.target.clone());
            }
            _ => {}
        }
    }
    false
}
