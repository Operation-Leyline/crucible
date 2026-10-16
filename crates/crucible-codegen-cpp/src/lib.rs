//! C++20 client generation from the shared lowered IR.
mod client;
mod links;
mod models;
mod names;
mod permissions;
mod surface;

pub use surface::surface;

use crucible_ir::{
    Diagnostics,
    ir::{Service, Shape},
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub struct Options {
    pub package_name: String,
    pub namespace: String,
    pub package_version: String,
    pub endpoint: Option<String>,
}
pub struct File {
    pub path: String,
    pub contents: String,
}

fn diagnose(service: &Service, options: &Options) -> Diagnostics {
    let mut result = Diagnostics::default();
    let package_upper = options.package_name.to_ascii_uppercase();
    if !names::identifier(&options.package_name)
        || ["CON", "PRN", "AUX", "NUL"].contains(&package_upper.as_str())
        || (1..=9).any(|number| {
            package_upper == format!("COM{number}") || package_upper == format!("LPT{number}")
        })
        || matches!(
            options.package_name.as_str(),
            "all"
                | "clean"
                | "help"
                | "install"
                | "test"
                | "package"
                | "edit_cache"
                | "rebuild_cache"
        )
    {
        result.push(
            &service.id,
            "C++ package name must be a non-reserved identifier",
        );
    }
    let version: Vec<_> = options.package_version.split('.').collect();
    if version.len() > 4
        || version
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        result.push(
            &service.id,
            "C++ package version must have one to four numeric components",
        );
    }
    if options
        .namespace
        .split("::")
        .any(|part| !names::identifier(part) || part == "std")
    {
        result.push(
            &service.id,
            "C++ namespace must contain non-reserved identifiers",
        );
    }
    let mut generated = BTreeSet::from([client::client_name(service)]);
    let mut aliases = BTreeMap::new();
    for (id, shape) in &service.shapes {
        let name = service.name_of(id);
        if models::model(shape)
            && (!names::identifier(name) || name == "std" || !generated.insert(name.to_owned()))
        {
            result.push(id, "invalid or colliding C++ model name");
        }
        match shape {
            Shape::Structure(structure) => {
                let mut fields = BTreeSet::new();
                for member in &structure.members {
                    let field = names::field(&member.name);
                    if !names::identifier(&field) || field == name || !fields.insert(field) {
                        result.push(
                            &id.with_member(&member.name),
                            "invalid or colliding C++ member name",
                        );
                    }
                }
            }
            Shape::Union(union) => {
                if name == "value" {
                    result.push(id, "C++ union name collides with its value member");
                }
                for variant in std::iter::once(format!("{name}Unknown"))
                    .chain(union.members.iter().map(|m| models::variant(name, &m.name)))
                {
                    if !names::identifier(&variant) || !generated.insert(variant) {
                        result.push(id, "C++ union variant name collision");
                    }
                }
            }
            Shape::Enum(_) | Shape::IntEnum(_) => {
                if name == "value" {
                    result.push(id, "C++ enum name collides with its value member");
                }
                let mut constants = BTreeSet::new();
                let values = match shape {
                    Shape::Enum(e) => e.values.iter().map(|value| &value.name).collect::<Vec<_>>(),
                    Shape::IntEnum(e) => e.values.iter().map(|value| &value.name).collect(),
                    _ => unreachable!(),
                };
                for value in values {
                    let factory = format!("known_{}", names::field(value));
                    if !names::identifier(&factory) || factory == name || !constants.insert(factory)
                    {
                        result.push(id, "C++ enum value name collision");
                    }
                }
            }
            Shape::List(list) => {
                aliases.insert(id, &list.member.target);
            }
            Shape::Map(map) => {
                aliases.insert(id, &map.value.target);
            }
            _ => {}
        }
    }
    for id in aliases.keys() {
        let mut next = *id;
        let mut seen = BTreeSet::new();
        while let Some(target) = aliases.get(next) {
            if target == id {
                result.push(id, "C++ cannot represent a recursive collection alias");
                break;
            }
            if !seen.insert(target) {
                break;
            }
            next = target;
        }
    }
    let mut methods = BTreeSet::from(["close".to_owned(), "runtime_".to_owned()]);
    methods.insert(client::client_name(service));
    for operation in &service.operations {
        if operation.method == "HEAD"
            && (!operation.request.body.is_empty() || operation.request.payload.is_some())
        {
            result.push(
                &operation.id,
                "C++ libcurl transport cannot send HEAD request body or payload bindings",
            );
        }
        for method in std::iter::once(client::method(operation)).chain(client::pages(operation)) {
            if !names::identifier(&method) || !methods.insert(method) {
                result.push(&operation.id, "C++ operation name collision");
            }
        }
    }
    permissions::diagnose(service, &mut result);
    result
}

pub fn capabilities() -> crucible_ir::Capabilities {
    use crucible_ir::Feature;
    crucible_ir::Capabilities::new([
        Feature::NonBlobPayload,
        Feature::MediaTypeHeaders,
        Feature::ErrorPayload,
        Feature::GreedyLabels,
        Feature::IntEnum,
        Feature::BigInteger,
        Feature::BigDecimal,
        Feature::Sparse,
    ])
}

pub fn emit(service: &Service, options: &Options) -> Result<Vec<File>, Diagnostics> {
    service.require_capabilities(&capabilities())?;
    crucible_codegen_common::check_service_exception_name(service)?;
    let diagnostics = diagnose(service, options);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let layout = models::Layout::new(service, options);
    let mut files = layout.emit();
    files.extend(client::emit(service, options, &layout));
    files.extend(permissions::emit(service, options));
    let package = &options.package_name;
    for (name, source) in [
        ("json.hpp", include_str!("../runtime/json.hpp")),
        ("json.cpp", include_str!("../runtime/json.cpp")),
        ("timestamp.hpp", include_str!("../runtime/timestamp.hpp")),
        ("timestamp.cpp", include_str!("../runtime/timestamp.cpp")),
        ("types.hpp", include_str!("../runtime/types.hpp")),
        ("debug.hpp", include_str!("../runtime/debug.hpp")),
        ("codec.hpp", include_str!("../runtime/codec.hpp")),
        ("codec.cpp", include_str!("../runtime/codec.cpp")),
        ("call_scope.hpp", include_str!("../runtime/call_scope.hpp")),
        ("transport.hpp", include_str!("../runtime/transport.hpp")),
        ("transport.cpp", include_str!("../runtime/transport.cpp")),
        (
            "client_runtime.hpp",
            include_str!("../runtime/client_runtime.hpp"),
        ),
        (
            "client_runtime.cpp",
            include_str!("../runtime/client_runtime.cpp"),
        ),
    ] {
        let mut contents = crucible_codegen_common::service_exception_runtime(service, source)
            .replace("crucible_generated", &options.namespace);
        let path = if name.ends_with(".hpp") {
            format!("include/{package}/runtime/{name}")
        } else {
            for header in [
                "json.hpp",
                "timestamp.hpp",
                "types.hpp",
                "codec.hpp",
                "transport.hpp",
                "client_runtime.hpp",
            ] {
                contents = contents.replace(
                    &format!("\"{header}\""),
                    &format!("\"{package}/runtime/{header}\""),
                );
            }
            format!("src/{name}")
        };
        files.push(File { path, contents });
    }
    let schema = serde_json::to_string(&crucible_codegen_common::schema(service, names::field))
        .expect("schema");
    files.push(File {path:"crucible.json".into(),contents:serde_json::to_string_pretty(&serde_json::json!({
        "generatorVersion":env!("CARGO_PKG_VERSION"), "service":service.id.to_string(),"serviceVersion":service.version,
        "packageVersion":options.package_version,"schemaSha256":format!("{:x}",Sha256::digest(schema.as_bytes()))
    })).expect("manifest")});
    files.push(File {path:format!("include/{package}/schema.hpp"),contents:format!("#pragma once\n#include \"runtime/codec.hpp\"\nnamespace {} {{ const runtime::Schema& schema(); }}\n",options.namespace)});
    // MSVC bounds individual string literals; join bounded chunks once at initialization.
    let characters: Vec<_> = schema.chars().collect();
    let chunks = characters
        .chunks(1024)
        .map(|part| {
            format!(
                "            {},\n",
                names::literal(&part.iter().collect::<String>())
            )
        })
        .collect::<String>();
    files.push(File {path:"src/schema.cpp".into(),contents:format!("#include \"{package}/schema.hpp\"\nnamespace {} {{\nconst runtime::Schema& schema() {{\n    static const runtime::Schema value([] {{\n        const char* chunks[] = {{\n{chunks}        }};\n        std::string text;\n        for (const auto* chunk : chunks) text += chunk;\n        return runtime::Json::parse(text);\n    }}());\n    return value;\n}}\n}}\n",options.namespace)});
    files.push(File {path:"CMakeLists.txt".into(),contents:format!("cmake_minimum_required(VERSION 3.20)\nproject({package} VERSION {} LANGUAGES CXX)\nfind_package(CURL 7.68 REQUIRED)\nfind_package(OpenSSL 1.1.1 REQUIRED)\nfind_package(Threads REQUIRED)\ninclude(GNUInstallDirs)\ninclude(CMakePackageConfigHelpers)\nadd_library({package} STATIC src/json.cpp src/timestamp.cpp src/codec.cpp src/models.cpp src/schema.cpp src/transport.cpp src/client_runtime.cpp src/client.cpp)\ntarget_link_libraries({package} PRIVATE CURL::libcurl OpenSSL::Crypto PUBLIC Threads::Threads)\nadd_library({package}::{package} ALIAS {package})\ntarget_compile_features({package} PUBLIC cxx_std_20)\nset_target_properties({package} PROPERTIES CXX_EXTENSIONS OFF POSITION_INDEPENDENT_CODE ON DEBUG_POSTFIX d)\ntarget_include_directories({package} PUBLIC $<BUILD_INTERFACE:${{CMAKE_CURRENT_SOURCE_DIR}}/include> $<INSTALL_INTERFACE:${{CMAKE_INSTALL_INCLUDEDIR}}>)\nif(MSVC)\n  target_compile_options({package} PUBLIC /utf-8 PRIVATE /W4 /WX)\nelse()\n  target_compile_options({package} PRIVATE -Wall -Wextra -Werror)\nendif()\ninstall(TARGETS {package} EXPORT {package}Targets ARCHIVE DESTINATION ${{CMAKE_INSTALL_LIBDIR}})\ninstall(DIRECTORY include/ DESTINATION ${{CMAKE_INSTALL_INCLUDEDIR}})\ninstall(EXPORT {package}Targets NAMESPACE {package}:: DESTINATION ${{CMAKE_INSTALL_LIBDIR}}/cmake/{package})\nconfigure_package_config_file(cmake/Config.cmake.in ${{CMAKE_CURRENT_BINARY_DIR}}/{package}Config.cmake INSTALL_DESTINATION ${{CMAKE_INSTALL_LIBDIR}}/cmake/{package})\nwrite_basic_package_version_file(${{CMAKE_CURRENT_BINARY_DIR}}/{package}ConfigVersion.cmake VERSION ${{PROJECT_VERSION}} COMPATIBILITY SameMajorVersion)\ninstall(FILES ${{CMAKE_CURRENT_BINARY_DIR}}/{package}Config.cmake ${{CMAKE_CURRENT_BINARY_DIR}}/{package}ConfigVersion.cmake DESTINATION ${{CMAKE_INSTALL_LIBDIR}}/cmake/{package})\ninstall(FILES crucible.json DESTINATION ${{CMAKE_INSTALL_DATADIR}}/{package})\n",options.package_version)});
    files.push(File {path:"cmake/Config.cmake.in".into(),contents:format!("@PACKAGE_INIT@\ninclude(CMakeFindDependencyMacro)\nfind_dependency(CURL 7.68)\nfind_dependency(OpenSSL 1.1.1)\nfind_dependency(Threads)\ninclude(\"${{CMAKE_CURRENT_LIST_DIR}}/{package}Targets.cmake\")\ncheck_required_components({package})\n")});
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}
