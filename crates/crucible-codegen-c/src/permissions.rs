//! `permissions.h` and `permissions.c` (docs/permissions.md §6.3): per
//! resource an open string type, `weather_graph_permission`, with a
//! `_known_` function per permission the way an enum has one per value, a
//! `_canonical` function mapping a deprecated alias to its primary, and
//! `weather_permissions`, the registry a console's grant picker reads.
//! Internal permissions never appear, whatever the audience. Every symbol
//! shares C's one namespace with the models and operations, so `lib.rs`
//! diagnoses a collision through [`symbols`].

use crate::{File, Options, names};
use crucible_codegen_common::permissions::{
    ClientResource, client_resources, code_span, notes, primary,
};
use crucible_smithy::ShapeId;
use crucible_smithy::ir::Service;

/// `weather_graph_permission` for the resource shape `Graph`.
fn type_name(service: &Service, client: &ClientResource<'_>, prefix: &str) -> String {
    format!(
        "{prefix}_{}_permission",
        names::field(service.name_of(client.id))
    )
}

/// The constant function for one permission of a resource type.
fn known(native: &str, symbol: &str) -> String {
    format!("{native}_known_{}", names::field(symbol))
}

/// Every symbol the permission files declare, each with the shape a
/// collision is reported against and what it names: the shared records and
/// the registry against the service, a resource type's symbols against its
/// resource shape.
pub fn symbols(service: &Service, prefix: &str) -> Vec<(ShapeId, String, String)> {
    let resources = client_resources(service);
    if resources.is_empty() {
        return Vec::new();
    }
    let mut result: Vec<(ShapeId, String, String)> = [
        "permission_description",
        "resource_permissions",
        "permissions",
        "permissions_count",
    ]
    .into_iter()
    .map(|name| {
        (
            service.id.clone(),
            format!("{prefix}_{name}"),
            "the permission registry".to_owned(),
        )
    })
    .collect();
    for client in &resources {
        let native = type_name(service, client, prefix);
        let shape = service.name_of(client.id);
        for name in [native.clone(), format!("{native}_canonical")] {
            result.push((
                client.id.clone(),
                name,
                format!("the permission type of `{shape}`"),
            ));
        }
        for permission in &client.permissions {
            result.push((
                client.id.clone(),
                known(&native, &permission.symbol),
                format!("permission `{}` of `{shape}`", permission.symbol),
            ));
        }
    }
    result
}

/// A C string view of static storage: `{"graph:read", 10}`. The literal is
/// NUL-terminated, so `data` also prints with `%s`.
fn view(value: &str) -> String {
    names::view(value)
}

pub fn emit(service: &Service, options: &Options) -> Vec<File> {
    let resources = client_resources(service);
    if resources.is_empty() {
        return Vec::new();
    }
    let p = &options.prefix;
    let guard = format!("{}_PERMISSIONS_H", p.to_ascii_uppercase());
    let mut header = format!(
        "#ifndef {guard}\n#define {guard}\n#include \"runtime/types.h\"\n\n#ifdef __cplusplus\nextern \"C\" {{\n#endif\n\n"
    );
    header.push_str(&format!("/** One permission in the registry. Every view has static storage and is NUL-terminated; never free one. */\ntypedef struct {{\n    /** What grants store. */\n    {p}_string value;\n    /** The model's documentation for it, empty when it has none. */\n    {p}_string description;\n    /** \"public\", \"permissionOnly\" or \"notGrantable\"; a string, since a newer server may add tiers. */\n    {p}_string tier;\n    /** False for a bundle the service confers whole: never offer it in a picker. */\n    bool grantable;\n    bool deprecated;\n    /** For a deprecated alias, the value it stands for; data is NULL otherwise. */\n    {p}_string alias_of;\n}} {p}_permission_description;\n\n/** One resource type's permissions, sorted by value. */\ntypedef struct {{\n    {p}_string resource_type;\n    size_t permission_count;\n    const {p}_permission_description *permissions;\n}} {p}_resource_permissions;\n"));
    let mut source = format!(
        "#include \"{}/permissions.h\"\n#include <string.h>\n\n",
        options.package_name
    );
    let mut registry = String::new();
    for (index, client) in resources.iter().enumerate() {
        let shape = service.name_of(client.id);
        let native = type_name(service, client, p);
        header.push('\n');
        header.push_str(&crucible_codegen_common::doxygen(
            &format!(
                "The permissions `{shape}` accepts (`{}`), by the values grants store: a known one, or any string a newer server writes, kept as it is. A value is a borrowed view.",
                client.resource.name
            ),
            "",
        ));
        header.push_str(&format!("typedef {p}_string {native};\n"));
        for permission in &client.permissions {
            // Model text: `doxygen_for` escapes it like every other model
            // comment, so `*/` closes nothing and `@param` is no command.
            let mut docs = permission.docs.clone().unwrap_or_default();
            if permission.alias_of.is_some() {
                if !docs.trim().is_empty() {
                    docs.push_str("\n\n");
                }
                let primary = &primary(client.resource, permission).value;
                docs.push_str(&format!("An alias of {}.", code_span(primary)));
            }
            header.push_str(&crucible_codegen_common::doxygen_for(
                Some(docs.as_str()).filter(|d| !d.trim().is_empty()),
                &notes(permission),
                "",
            ));
            header.push_str(&format!(
                "static inline {native} {}(void) {{ {native} result = {}; return result; }}\n",
                known(&native, &permission.symbol),
                view(&permission.value)
            ));
        }
        header.push_str(&format!("/** Returns the permission value stands for: a deprecated alias's primary, or value itself, unknown values included. The result is value or a view of static storage. */\n{native} {native}_canonical({native} value);\n"));
        source.push_str(&format!("{native} {native}_canonical({native} value) {{\n"));
        for (alias, primary) in client.aliases() {
            source.push_str(&format!(
                "    if (value.length == {} && memcmp(value.data, {}, {}) == 0) {{ {native} primary = {}; return primary; }}\n",
                alias.value.len(),
                names::literal(&alias.value),
                alias.value.len(),
                view(&primary.value)
            ));
        }
        source.push_str("    return value;\n}\n");
        let entries = client.entries();
        source.push_str(&format!(
            "static const {p}_permission_description entries_{index}[] = {{\n"
        ));
        for entry in &entries {
            source.push_str(&format!(
                "    {{{}, {}, {}, {}, {}, {}}},\n",
                view(entry.value),
                view(entry.description),
                view(entry.tier),
                entry.grantable,
                entry.deprecated,
                entry.alias_of.map_or_else(|| "{NULL, 0}".to_owned(), view)
            ));
        }
        source.push_str("};\n");
        registry.push_str(&format!(
            "    {{{}, {}, entries_{index}}},\n",
            view(&client.resource.name),
            entries.len()
        ));
    }
    let count = resources.len();
    header.push_str(&format!("\n/** Every permission this client knows, by resource type, deprecated aliases included; static storage, never freed. A grant picker offers only the grantable ones. */\nextern const {p}_resource_permissions {p}_permissions[{count}];\n/** The number of resource types in {p}_permissions. */\nextern const size_t {p}_permissions_count;\n\n#ifdef __cplusplus\n}}\n#endif\n#endif\n"));
    source.push_str(&format!(
        "const {p}_resource_permissions {p}_permissions[{count}] = {{\n{registry}}};\nconst size_t {p}_permissions_count = {count};\n"
    ));
    vec![
        File {
            path: format!("include/{}/permissions.h", options.package_name),
            contents: header,
        },
        File {
            path: "src/permissions.c".into(),
            contents: source,
        },
    ]
}
