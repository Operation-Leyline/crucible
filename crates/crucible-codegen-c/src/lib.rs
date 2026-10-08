//! Native C17 client generation from the shared lowered IR.
mod client;
pub mod models;
mod names;

use crucible_smithy::{
    Diagnostics,
    ir::{Service, Shape},
};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub struct Options {
    pub package_name: String,
    pub prefix: String,
    pub package_version: String,
    pub endpoint: Option<String>,
    pub content_sha256: bool,
}

pub struct File {
    pub path: String,
    pub contents: String,
}

fn diagnose(service: &Service, options: &Options) -> Diagnostics {
    let mut errors = Diagnostics::default();
    for (label, value) in [
        ("package name", &options.package_name),
        ("symbol prefix", &options.prefix),
    ] {
        if !names::identifier(value) {
            errors.push(
                &service.id,
                format!("C {label} must be a non-reserved identifier"),
            );
        }
    }
    if [
        "all",
        "clean",
        "help",
        "install",
        "test",
        "package",
        "edit_cache",
        "rebuild_cache",
    ]
    .contains(&options.package_name.as_str())
    {
        errors.push(&service.id, "C package name is reserved by CMake");
    }
    let version: Vec<_> = options.package_version.split('.').collect();
    if version.len() > 4
        || version
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        errors.push(
            &service.id,
            "C package version needs one to four numeric components",
        );
    }
    let mut symbols = BTreeSet::new();
    let mut tags = BTreeSet::new();
    for (id, shape) in &service.shapes {
        if !models::complex(shape) && !matches!(shape, Shape::Enum(_) | Shape::IntEnum(_)) {
            continue;
        }
        let native = models::native_name(service, id, &options.prefix);
        if models::complex(shape) && !tags.insert(native.clone()) {
            errors.push(id, "C generated tag collision");
        }
        let mut emitted = vec![
            native.clone(),
            format!("{native}_encode"),
            format!("{native}_decode"),
            format!("{native}_debug"),
        ];
        match shape {
            Shape::Structure(s) | Shape::Union(s) => {
                let mut members = BTreeSet::new();
                for member in &s.members {
                    let field = names::field(&member.name);
                    if !names::identifier(&field) || !members.insert(field.clone()) {
                        errors.push(&id.with_member(&member.name), "C member name collision");
                    }
                    if matches!(shape, Shape::Union(_)) {
                        emitted.push(format!("{native}_tag_{field}"));
                    } else {
                        for helper in ["set", "get", "has", "clear", "null"] {
                            emitted.push(format!("{native}_{helper}_{field}"));
                        }
                    }
                }
                if matches!(shape, Shape::Union(_)) {
                    emitted.push(format!("{native}_unknown"));
                    if !tags.insert(format!("{native}_tag")) {
                        errors.push(id, "C generated tag collision");
                    }
                }
            }
            Shape::Map(_) => emitted.push(format!("{native}_entry")),
            Shape::List(list) if list.sparse => emitted.push(format!("{native}_item")),
            Shape::IntEnum(e) => {
                for value in &e.values {
                    emitted.push(format!("{native}_known_{}", names::field(&value.name)));
                }
            }
            Shape::Enum(e) => {
                for value in &e.values {
                    emitted.push(format!("{native}_known_{}", names::field(&value.name)));
                }
            }
            _ => {}
        }
        for symbol in emitted {
            if !names::identifier(&symbol) || !symbols.insert(symbol) {
                errors.push(id, "C generated symbol collision");
            }
        }
    }
    for (id, symbol) in client::symbols(service, &options.prefix) {
        if !names::identifier(&symbol) || !symbols.insert(symbol) {
            errors.push(&id, "C operation symbol collision");
        }
    }
    for op in &service.operations {
        if op.method == "HEAD" && (!op.request.body.is_empty() || op.request.payload.is_some()) {
            errors.push(&op.id, "C transport cannot send a HEAD request body");
        }
    }
    errors
}

pub fn capabilities() -> crucible_smithy::Capabilities {
    crucible_smithy::Capabilities::new([
        crucible_smithy::Feature::NonBlobPayload,
        crucible_smithy::Feature::MediaTypeHeaders,
        crucible_smithy::Feature::ErrorPayload,
        crucible_smithy::Feature::JsonName,
        crucible_smithy::Feature::GreedyLabels,
        crucible_smithy::Feature::QueryParams,
        crucible_smithy::Feature::PrefixHeaders,
        crucible_smithy::Feature::ResponseCode,
        crucible_smithy::Feature::IntEnum,
        crucible_smithy::Feature::BigInteger,
        crucible_smithy::Feature::BigDecimal,
        crucible_smithy::Feature::Sparse,
        crucible_smithy::Feature::Sets,
        crucible_smithy::Feature::BasicAuth,
        crucible_smithy::Feature::ApiKeyAuth,
        crucible_smithy::Feature::SigV4,
        crucible_smithy::Feature::OptionalAuth,
        crucible_smithy::Feature::Waiters,
        crucible_smithy::Feature::EndpointRules,
        crucible_smithy::Feature::EndpointHostPrefix,
        crucible_smithy::Feature::AwsRestJson1,
        crucible_smithy::Feature::AwsJson10,
        crucible_smithy::Feature::AwsJson11,
        crucible_smithy::Feature::HttpChecksum,
    ])
}

pub fn emit(service: &Service, options: &Options) -> Result<Vec<File>, Diagnostics> {
    service.require_capabilities(&capabilities())?;
    let diagnostics = diagnose(service, options);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let mut files = models::emit(service, options);
    let schema = serde_json::to_string(&crucible_codegen_common::schema(service, names::field))
        .expect("schema");
    files.extend(client::emit(service, options, &schema));
    for (name, source) in [
        ("types.h", include_str!("../runtime/types.h")),
        ("internal.h", include_str!("../runtime/internal.h")),
        ("schema.h", include_str!("../runtime/schema.h")),
        ("arena.c", include_str!("../runtime/arena.c")),
        ("json.c", include_str!("../runtime/json.c")),
        ("number.c", include_str!("../runtime/number.c")),
        ("timestamp.c", include_str!("../runtime/timestamp.c")),
        ("codec.c", include_str!("../runtime/codec.c")),
        ("http.h", include_str!("../runtime/http.h")),
        ("auth.h", include_str!("../runtime/auth.h")),
        ("endpoints.h", include_str!("../runtime/endpoints.h")),
        ("scope.c", include_str!("../runtime/scope.c")),
        ("headers.c", include_str!("../runtime/headers.c")),
        ("transport.c", include_str!("../runtime/transport.c")),
        (
            "client_runtime.h",
            include_str!("../runtime/client_runtime.h"),
        ),
        (
            "client_runtime.c",
            include_str!("../runtime/client_runtime.c"),
        ),
        ("bindings.h", include_str!("../runtime/bindings.h")),
        ("bindings.c", include_str!("../runtime/bindings.c")),
        ("features.h", include_str!("../runtime/features.h")),
        ("encoding.c", include_str!("../runtime/encoding.c")),
        ("auth.c", include_str!("../runtime/auth.c")),
        ("checksums.c", include_str!("../runtime/checksums.c")),
        ("query.c", include_str!("../runtime/query.c")),
        ("endpoints.c", include_str!("../runtime/endpoints.c")),
        ("waiters.c", include_str!("../runtime/waiters.c")),
    ] {
        let contents = source
            .replace("crucible_generated", &options.prefix)
            .replace("CRUCIBLE_GENERATED", &options.prefix.to_ascii_uppercase());
        let public =
            name.ends_with(".h") && !["internal.h", "bindings.h", "features.h"].contains(&name);
        let path = if public {
            format!("include/{}/runtime/{name}", options.package_name)
        } else {
            format!("src/{name}")
        };
        let mut contents = contents;
        if !public {
            for header in [
                "types.h",
                "schema.h",
                "http.h",
                "auth.h",
                "endpoints.h",
                "client_runtime.h",
            ] {
                contents = contents.replace(
                    &format!("\"{header}\""),
                    &format!("\"{}/runtime/{header}\"", options.package_name),
                );
            }
        }
        files.push(File { path, contents });
    }
    let package = &options.package_name;
    files.push(File {path:"crucible.json".into(),contents:serde_json::to_string_pretty(&serde_json::json!({
        "generatorVersion":env!("CARGO_PKG_VERSION"),"service":service.id.to_string(),"serviceVersion":service.version,
        "packageVersion":options.package_version,"schemaSha256":format!("{:x}",Sha256::digest(schema.as_bytes()))
    })).expect("manifest")});
    files.push(File {
        path: "schema.json".into(),
        contents: schema,
    });
    let sources = files
        .iter()
        .filter(|f| f.path.ends_with(".c"))
        .map(|f| f.path.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    files.push(File {path:"CMakeLists.txt".into(),contents:format!("cmake_minimum_required(VERSION 3.20)\nproject({package} VERSION {} LANGUAGES C)\nfind_package(yyjson CONFIG REQUIRED)\nfind_package(CURL 7.68 REQUIRED)\nfind_package(OpenSSL 1.1.1 REQUIRED)\nfind_package(Threads REQUIRED)\ninclude(GNUInstallDirs)\ninclude(CMakePackageConfigHelpers)\nadd_library({package} STATIC {sources})\nadd_library({package}::{package} ALIAS {package})\ntarget_compile_features({package} PUBLIC c_std_17)\nset_target_properties({package} PROPERTIES C_EXTENSIONS OFF POSITION_INDEPENDENT_CODE ON DEBUG_POSTFIX d)\ntarget_link_libraries({package} PRIVATE yyjson::yyjson CURL::libcurl OpenSSL::Crypto Threads::Threads)\ntarget_include_directories({package} PUBLIC $<BUILD_INTERFACE:${{CMAKE_CURRENT_SOURCE_DIR}}/include> $<INSTALL_INTERFACE:${{CMAKE_INSTALL_INCLUDEDIR}}>)\nif(MSVC)\n  target_compile_definitions({package} PRIVATE _CRT_SECURE_NO_WARNINGS)\n  target_compile_options({package} PRIVATE /W4 /WX)\nelse()\n  target_compile_options({package} PRIVATE -Wall -Wextra -Werror)\n  target_link_libraries({package} PRIVATE m)\nendif()\ninstall(TARGETS {package} EXPORT {package}Targets ARCHIVE DESTINATION ${{CMAKE_INSTALL_LIBDIR}})\ninstall(DIRECTORY include/ DESTINATION ${{CMAKE_INSTALL_INCLUDEDIR}})\ninstall(EXPORT {package}Targets NAMESPACE {package}:: DESTINATION ${{CMAKE_INSTALL_LIBDIR}}/cmake/{package})\nconfigure_package_config_file(cmake/Config.cmake.in ${{CMAKE_CURRENT_BINARY_DIR}}/{package}Config.cmake INSTALL_DESTINATION ${{CMAKE_INSTALL_LIBDIR}}/cmake/{package})\nwrite_basic_package_version_file(${{CMAKE_CURRENT_BINARY_DIR}}/{package}ConfigVersion.cmake VERSION ${{PROJECT_VERSION}} COMPATIBILITY SameMajorVersion)\ninstall(FILES ${{CMAKE_CURRENT_BINARY_DIR}}/{package}Config.cmake ${{CMAKE_CURRENT_BINARY_DIR}}/{package}ConfigVersion.cmake DESTINATION ${{CMAKE_INSTALL_LIBDIR}}/cmake/{package})\ninstall(FILES crucible.json schema.json DESTINATION ${{CMAKE_INSTALL_DATADIR}}/{package})\n",options.package_version)});
    files.push(File {path:"cmake/Config.cmake.in".into(),contents:format!("@PACKAGE_INIT@\ninclude(CMakeFindDependencyMacro)\nfind_dependency(yyjson CONFIG)\nfind_dependency(CURL 7.68)\nfind_dependency(OpenSSL 1.1.1)\nfind_dependency(Threads)\ninclude(\"${{CMAKE_CURRENT_LIST_DIR}}/{package}Targets.cmake\")\ncheck_required_components({package})\n")});
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}
