//! The weather fixture's generated client, checked in, so every change to the
//! output shows in review. `CRUCIBLE_BLESS=1 cargo test` rewrites it.
//! `src/runtime.ts` is a verbatim copy of `runtime/runtime.ts` and is left out.

use std::fs;
use std::path::Path;

/// A package under `tests/models`, lowered.
fn load(name: &str) -> crucible_ir::ir::Service {
    compile_with(name, |text| text)
        .unwrap_or_else(|rendered| panic!("{name} compiles:\n{rendered}"))
}

/// A package under `tests/models` with every source file rewritten first;
/// the rendered diagnostics when it does not compile.
fn compile_with(
    name: &str,
    edit: impl Fn(String) -> String,
) -> Result<crucible_ir::ir::Service, String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name);
    let mut package = crucible_lang::load_package(&dir).unwrap();
    for file in &mut package.files {
        file.text = edit(std::mem::take(&mut file.text).replace("\r\n", "\n"));
    }
    let compiled = crucible_lang::compile(&package, None);
    compiled.service.clone().ok_or_else(|| compiled.render())
}

/// The weather package with its sources rewritten first.
fn weather_with(edit: impl Fn(String) -> String) -> crucible_ir::ir::Service {
    compile_with("weather", edit).unwrap_or_else(|rendered| panic!("weather compiles:\n{rendered}"))
}

fn weather() -> crucible_ir::ir::Service {
    load("weather")
}

#[test]
fn stream_client_matches_golden() {
    let files =
        crucible_codegen_ts::emit(&load("server-stream"), &options("@example/streams")).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/server-stream");
    let mut stale = Vec::new();
    for file in files {
        if file.path == "src/runtime.ts" {
            continue;
        }
        let path = root.join(&file.path);
        if std::env::var_os("CRUCIBLE_BLESS").is_some() {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &file.contents).unwrap();
        }
        if fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n")
            != file.contents
        {
            stale.push(file.path);
        }
    }
    assert!(
        stale.is_empty(),
        "generated output differs from tests/golden/server-stream in {stale:?}; review the change and rerun with CRUCIBLE_BLESS=1"
    );
}

#[test]
fn late_document_schemas_follow_reused_http_payloads_through_recursive_containers() {
    let mut service = load("stream-error-document");
    let envelope = service
        .shapes
        .keys()
        .find(|id| id.name() == "Envelope")
        .unwrap()
        .clone();
    let payload = service
        .shapes
        .values()
        .find_map(|shape| match shape {
            crucible_ir::ir::Shape::Structure(shape) => shape.error.as_ref()?.payload.clone(),
            _ => None,
        })
        .unwrap();
    // The IR also accepts named HTTP structures from front ends other than .cru.
    let read = service
        .operations
        .iter_mut()
        .find(|op| op.name() == "Read")
        .unwrap();
    read.output = Some(envelope);
    read.response.payload = Some(payload);
    let files = emit(&service, &options("@example/stream-errors")).unwrap();
    let schemas = &files
        .iter()
        .find(|file| file.path == "src/schemas.ts")
        .unwrap()
        .contents;
    assert!(schemas.contains("S_Envelope: Schema = structure(() => ({ next: S_Envelope })"));
    assert!(schemas.contains(
        "S_Envelope_document: Schema = structure(() => ({ data: blob, next: S_Envelope_document })"
    ));
    assert!(schemas.contains("S_Envelopes_document: Schema = list(() => S_Envelope_document)"));
    assert!(schemas.contains("S_EnvelopeMap_document: Schema = map(() => S_Envelope_document)"));
    assert!(schemas.contains("documentSchema: S_NestedProblem_document"));
}

use crucible_codegen_ts::{Middleware, Options, emit};

/// The typed permissions fixture, a `.cru` package; `edit` rewrites its
/// source first.
fn leyline_with(edit: impl Fn(String) -> String) -> crucible_ir::ir::Service {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/permissions");
    let mut package = crucible_lang::load_package(&dir).unwrap();
    for file in &mut package.files {
        file.text = edit(std::mem::take(&mut file.text));
    }
    let compiled = crucible_lang::compile(&package, None);
    let rendered = compiled.render();
    compiled
        .service
        .unwrap_or_else(|| panic!("the fixture compiles:\n{rendered}"))
}

fn leyline() -> crucible_ir::ir::Service {
    leyline_with(|text| text)
}

/// Typed permissions (docs/permissions.md §6.3): open constants per
/// resource, deprecated aliases, the registry, and each method's
/// requirements in its documentation; nothing of the internal tier.
#[test]
fn permission_constants_match_golden() {
    let external = crucible_ir::project(&leyline(), crucible_ir::Audience::External).unwrap();
    let options = Options {
        package_name: "@example/leyline".into(),
        package_version: "0.0.0-dev".into(),
        client_name: None,
        endpoint: None,
        middleware: vec![],
        package_json: None,
        metadata: Default::default(),
    };
    let files = emit(&external, &options).unwrap();
    let file = |path: &str| {
        files
            .iter()
            .find(|file| file.path == path)
            .unwrap()
            .contents
            .clone()
    };
    let permissions = file("src/permissions.ts");
    assert!(!permissions.contains("organization:read"), "{permissions}");
    let index = file("src/index.ts");
    assert!(
        index.contains("export { PERMISSIONS, canonicalPermission, GraphPermission, HistoryPermission, OrganizationPermission } from \"./permissions.js\";"),
        "{index}"
    );
    assert!(!index.contains("export * as permissions"), "{index}");
    let client = file("src/client.ts");
    assert!(client.contains(
        "Requires `graph:read` on the graph; `graph:write` on the graph, checked by the service (only when the parsed query writes); `history:read` on the history, when `has target.history`."
    ));
    // Hostile documentation: a backtick in a `when` literal gets a longer
    // fence, a one-paragraph handler note keeps `HTTP`, and a longer one
    // follows the sentence whole. Its model text is escaped: a bare `[ops]`
    // is no link and a line-start `@see` no JSDoc tag swallowing the rest.
    assert!(
        client.contains("Requires `graph:read` on the graph, when ``mode == \"a`b\"``; `graph:write` on the graph, checked by the service (HTTP rewrites only, when `mode` is ``a`b``, and \\[ops] approve); `history:read` on the history, checked by the service (see below)."),
        "{client}"
    );
    assert!(
        client.contains("`history:read` on the history, checked by the service:\n   *\n   * GraphQL plans are checked twice:"),
        "{client}"
    );
    assert!(
        client.contains("\n   * \\@see the runbook; @param is no tag either.\n   *\n   * Input: RewriteGraphInput."),
        "{client}"
    );
    // Requirements on internal permissions are still requirements: a mixed
    // list says more are internal, and an all-internal one says only that.
    assert!(
        client.contains(
            "Requires `graph:write` on the graph; and further permissions internal to the service."
        ),
        "{client}"
    );
    assert!(
        client.contains("Requires permissions internal to the service."),
        "{client}"
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/permissions.ts");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::write(&path, &permissions).unwrap();
    }
    assert_eq!(
        fs::read_to_string(path).unwrap().replace("\r\n", "\n"),
        permissions
    );
    // A model without permissions gets no file.
    let weather = weather();
    assert!(
        emit(&weather, &options)
            .unwrap()
            .iter()
            .all(|file| file.path != "src/permissions.ts")
    );
}

/// An internal-audience client keeps internal members and operations, but
/// its registry has no internal permission (decision 9), so its doc
/// comments name none either: they say what is left out, as the client
/// audience projection does.
#[test]
fn an_internal_audience_client_names_no_internal_permission() {
    let service = leyline();
    let internal = crucible_ir::project(&service, crucible_ir::Audience::Internal).unwrap();
    let options = Options {
        package_name: "@example/leyline".into(),
        package_version: "0.0.0-dev".into(),
        client_name: None,
        endpoint: None,
        middleware: vec![],
        package_json: None,
        metadata: Default::default(),
    };
    let files = emit(&internal, &options).unwrap();
    for file in &files {
        assert!(
            !file.contents.contains("organization:read"),
            "{}: {}",
            file.path,
            file.contents
        );
    }
    let client = &files
        .iter()
        .find(|file| file.path == "src/client.ts")
        .unwrap()
        .contents;
    // The member is this audience's; the internal clause reading it is not.
    assert!(
        files
            .iter()
            .any(|file| file.contents.contains("auditTicket"))
    );
    assert!(
        files
            .iter()
            .all(|file| !file.contents.contains("has auditTicket"))
    );
    assert!(
        client.contains(
            "Requires `graph:write` on the graph; and further permissions internal to the service."
        ),
        "{client}"
    );
    assert!(
        client.contains("Requires permissions internal to the service."),
        "{client}"
    );
}

/// A permission export a model already names (`GraphPermission`) stays out
/// of the package root, where the model keeps the name, and every export
/// is reachable as `permissions.*`: no build failure.
#[test]
fn a_permission_export_named_like_a_model_is_namespaced() {
    let service = leyline_with(|text| text.replace("HistoryRef", "GraphPermission"));
    let external = crucible_ir::project(&service, crucible_ir::Audience::External).unwrap();
    let options = Options {
        package_name: "@example/leyline".into(),
        package_version: "0.0.0-dev".into(),
        client_name: None,
        endpoint: None,
        middleware: vec![],
        package_json: None,
        metadata: Default::default(),
    };
    let files = emit(&external, &options).unwrap();
    let index = &files
        .iter()
        .find(|file| file.path == "src/index.ts")
        .unwrap()
        .contents;
    assert!(
        index.contains("export { PERMISSIONS, canonicalPermission, HistoryPermission, OrganizationPermission } from \"./permissions.js\";"),
        "{index}"
    );
    assert!(
        index.contains("export type { PermissionDescription, ResourcePermissions, GraphPermissionValue, HistoryPermissionValue, OrganizationPermissionValue } from \"./permissions.js\";"),
        "{index}"
    );
    assert!(
        index.contains("export * as permissions from \"./permissions.js\";"),
        "{index}"
    );
    // A model named `permissions` as well leaves no name for the namespace.
    let service = leyline_with(|text| {
        text.replace("HistoryRef", "GraphPermission")
            .replace("list Names: string", "list permissions: string")
            .replace("Names", "permissions")
    });
    let external = crucible_ir::project(&service, crucible_ir::Audience::External).unwrap();
    let diagnostics = emit(&external, &options).unwrap_err();
    assert!(
        diagnostics
            .to_string()
            .contains("a model named `permissions`"),
        "{diagnostics}"
    );
}

#[test]
fn weather_client_matches_golden_files() {
    let service = weather();
    let options = Options {
        package_name: "@example/weather".into(),
        package_version: "0.0.0-dev".into(),
        client_name: None,
        endpoint: Some("https://weather.example".into()),
        middleware: vec![Middleware::ContentSha256],
        package_json: None,
        metadata: Default::default(),
    };
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/weather");
    let bless = std::env::var_os("CRUCIBLE_BLESS").is_some();
    let mut stale = Vec::new();
    for file in emit(&service, &options).unwrap() {
        if file.path == "src/runtime.ts" {
            continue;
        }
        let path = golden.join(&file.path);
        if bless {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &file.contents).unwrap();
        } else if fs::read_to_string(&path)
            .map(|s| s.replace("\r\n", "\n"))
            .ok()
            != Some(file.contents)
        {
            stale.push(file.path);
        }
    }
    assert!(
        stale.is_empty(),
        "generated output differs from tests/golden/weather in {stale:?}; \
         review the change and rerun with CRUCIBLE_BLESS=1"
    );
}

/// A model may name a shape `Date` (a calendar date, say). Timestamps are
/// the global `Date`, so they must not resolve to that shape.
#[test]
fn a_shape_named_date_does_not_shadow_timestamps() {
    let service = weather_with(|text| {
        let text = text.replace(
            "structure CitySummary {\n",
            "structure CitySummary {\n    foundedOn?: Date\n",
        );
        if text.contains("structure CitySummary {") {
            text + "\nstring Date\n"
        } else {
            text
        }
    });
    let options = Options {
        package_name: "@example/weather".into(),
        package_version: "0.0.0-dev".into(),
        client_name: None,
        endpoint: None,
        middleware: vec![],
        package_json: None,
        metadata: Default::default(),
    };
    let files = emit(&service, &options).unwrap();
    let models = &files
        .iter()
        .find(|file| file.path == "src/models.ts")
        .unwrap()
        .contents;
    assert!(models.contains("export type Date = string;"), "{models}");
    assert!(
        models.contains("export type DateTime = globalThis.Date;"),
        "{models}"
    );
}

#[test]
fn schemas_preserve_ir_nullability_and_exact_long_defaults() {
    let service = weather_with(|text| {
        text.replace(
            "population: long = 0",
            &format!("population: long = {}", i64::MAX),
        )
    });
    let files = emit(
        &service,
        &Options {
            package_name: "weather".into(),
            package_version: "0.0.0".into(),
            client_name: None,
            endpoint: None,
            middleware: vec![],
            package_json: None,
            metadata: Default::default(),
        },
    )
    .unwrap();
    let schemas = &files
        .iter()
        .find(|f| f.path == "src/schemas.ts")
        .unwrap()
        .contents;
    assert!(schemas.contains("population: parseJson(\"9223372036854775807\")"));
    let city = schemas
        .lines()
        .find(|line| line.starts_with("export const S_CitySummary:"))
        .unwrap();
    assert!(city.ends_with("[\"id\", \"name\", \"population\"]);"));
    let input = schemas
        .lines()
        .find(|line| line.starts_with("export const S_CreateCityInput:"))
        .unwrap();
    assert!(input.ends_with("[\"name\"]);"));
    let models = &files
        .iter()
        .find(|f| f.path == "src/models.ts")
        .unwrap()
        .contents;
    assert!(models.contains("population: bigint;"));
    assert!(models.contains("expectedRevision?: bigint;"));
}

#[test]
fn prototype_named_members_defaults_and_errors_are_own_metadata() {
    let service = weather_with(|text| {
        text.replace("ConflictError", "__proto__").replace(
            "structure CitySummary {\n",
            "structure CitySummary {\n    __proto__: long = 3\n",
        )
    });
    let files = emit(
        &service,
        &Options {
            package_name: "weather".into(),
            package_version: "0.0.0".into(),
            client_name: None,
            endpoint: None,
            middleware: vec![],
            package_json: None,
            metadata: Default::default(),
        },
    )
    .unwrap();
    let schemas = &files
        .iter()
        .find(|f| f.path == "src/schemas.ts")
        .unwrap()
        .contents;
    assert!(schemas.contains("[\"__proto__\"]: integer(64)"));
    assert!(schemas.contains("[\"__proto__\"]: parseJson(\"3\")"));
    assert!(schemas.contains("[\"__proto__\"]: { status: 409"));
    assert!(schemas.contains("new e.__proto__(fields, metadata)"));
}

#[test]
fn generated_hardening_fixture_preserves_names_auth_and_client_defaults() {
    let service = load("hardening");
    let files = emit(
        &service,
        &Options {
            package_name: "hardening".into(),
            package_version: "0.0.0".into(),
            client_name: None,
            endpoint: None,
            middleware: vec![],
            package_json: None,
            metadata: Default::default(),
        },
    )
    .unwrap();
    let source = |path| {
        &files
            .iter()
            .find(|file| file.path == path)
            .unwrap()
            .contents
    };
    let client = source("src/client.ts");
    for method in [
        "constructor_3",
        "constructor_2",
        "get",
        "get_2",
        "paginateListThings",
        "paginateListThings_2",
    ] {
        assert!(client.contains(&format!("  {method}(")), "{client}");
    }
    assert!(client.contains("return paginate(page => this.listThings(page, options)"));
    let schemas = source("src/schemas.ts");
    let imports = schemas
        .lines()
        .find(|line| line.starts_with("import {"))
        .unwrap();
    for helper in ["boolean", "document", "number", "string"] {
        assert!(imports.contains(helper), "{imports}");
    }
    assert!(schemas.contains("name: \"Public\",\n  method: \"GET\",\n  bearer: false"));
    assert!(schemas.contains("name: \"Protected\",\n  method: \"GET\",\n  bearer: true"));
    let defaults = schemas
        .lines()
        .find(|line| line.starts_with("export const S_DefaultsOperationOutput:"))
        .unwrap();
    assert!(defaults.contains("ordinary: parseJson(\"7\")"));
    let input = schemas
        .lines()
        .find(|line| line.starts_with("export const S_DefaultsOperationInput:"))
        .unwrap();
    // Input members are required in clients (decision 20), so an input
    // default is kept like an output's.
    assert!(input.contains("ordinary: parseJson(\"7\")"));

    // Generated method allocation must not depend on an input model's order.
    let mut reordered = service.clone();
    reordered.operations.reverse();
    let reordered_files = emit(
        &reordered,
        &Options {
            package_name: "hardening".into(),
            package_version: "0.0.0".into(),
            client_name: None,
            endpoint: None,
            middleware: vec![],
            package_json: None,
            metadata: Default::default(),
        },
    )
    .unwrap();
    let reversed_client = &reordered_files
        .iter()
        .find(|file| file.path == "src/client.ts")
        .unwrap()
        .contents;
    for method in [
        "constructor_3",
        "constructor_2",
        "get",
        "get_2",
        "paginateListThings",
        "paginateListThings_2",
    ] {
        assert!(reversed_client.contains(&format!("  {method}(")));
    }
}

/// Common package metadata maps onto `package.json` as
/// docs/sdk-packaging.md's table says, and the README and license are copied.
#[test]
fn package_metadata_matches_golden() {
    let service = weather();
    let mut options = options("@example/weather");
    let metadata = &mut options.metadata;
    metadata.description = Some("Client for the Weather API.".into());
    metadata.repository = Some("https://github.com/example/weather".into());
    metadata.homepage = Some("https://weather.example".into());
    metadata.documentation = Some("https://docs.weather.example".into());
    metadata.license_file = Some("Proprietary.\n".into());
    metadata.authors = vec![
        "Weather Team <sdk@weather.example>".into(),
        "Ops <ops@weather.example>".into(),
    ];
    metadata.readme = Some("# Weather\n".into());
    metadata.keywords = vec!["weather".into(), "forecast".into()];
    metadata.private = true;
    metadata.registry = Some(crucible_codegen_common::Registry::npm(
        "https://npm.pkg.github.com",
        Some("restricted"),
    ));
    let files = emit(&service, &options).unwrap();
    let package = file(&files, "package.json");
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/metadata.package.json");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::write(&path, package).unwrap();
    }
    assert_eq!(
        fs::read_to_string(path).unwrap().replace("\r\n", "\n"),
        package
    );
    assert_eq!(file(&files, "README.md"), "# Weather\n");
    assert_eq!(file(&files, "LICENSE"), "Proprietary.\n");
    // Publishable to the named registry, so not `"private": true`.
    let json: serde_json::Value = serde_json::from_str(package).unwrap();
    assert!(json.get("private").is_none());
    // Private with no npm registry: npm refuses to publish it.
    options.metadata.registry = None;
    let files = emit(&service, &options).unwrap();
    let json: serde_json::Value = serde_json::from_str(file(&files, "package.json")).unwrap();
    assert_eq!(json["private"], true);
    assert!(json.get("publishConfig").is_none());
    // Released through Git tags: never an npm publish, private or not.
    options.metadata.private = false;
    options.metadata.registry = Some(crucible_codegen_common::Registry::Git);
    let files = emit(&service, &options).unwrap();
    let json: serde_json::Value = serde_json::from_str(file(&files, "package.json")).unwrap();
    assert_eq!(json["private"], true);
    // A private package on the public registry, however it is spelled, is refused.
    options.metadata.private = true;
    options.metadata.registry = Some(crucible_codegen_common::Registry::npm(
        "https://%72egistry.npmjs.org.",
        None,
    ));
    assert!(emit(&service, &options).is_err());
}

/// Provenance is stamped by the shared layer: the package lists both files
/// without the generator writing either.
#[test]
fn the_package_lists_provenance_it_does_not_write() {
    let service = weather();
    let files = emit(&service, &options("weather")).unwrap();
    let package: serde_json::Value = serde_json::from_str(file(&files, "package.json")).unwrap();
    assert_eq!(
        package["files"],
        serde_json::json!([
            "dist",
            "crucible.json",
            "crucible-model.json",
            "README.md",
            "AGENTS.md"
        ])
    );
    assert!(
        !files
            .iter()
            .any(|f| f.path == "crucible.json" || f.path == "crucible-model.json")
    );
    // Without a configured README the package carries the generated one,
    // beside its AGENTS.md; a license only when one is configured.
    let readme = file(&files, "README.md");
    assert!(readme.starts_with("# weather\n"), "{readme}");
    assert!(file(&files, "AGENTS.md").contains("npm install weather@0.0.0"));
    assert!(!files.iter().any(|f| f.path == "LICENSE"));
}

#[test]
fn package_json_cannot_override_what_the_package_owns() {
    let service = weather();
    let mut options = options("weather");
    options.package_json = Some(serde_json::json!({
        "name": "other", "version": "9.9.9", "crucible": {}, "files": [],
        "private": false, "publishConfig": {}, "bugs": "https://x.example"
    }));
    let error = emit(&service, &options).unwrap_err().to_string();
    for key in [
        "name",
        "version",
        "crucible",
        "files",
        "private",
        "publishConfig",
    ] {
        assert!(error.contains(&format!("cannot set `{key}`")), "{error}");
    }
    assert!(!error.contains("bugs"));
    options.package_json = Some(serde_json::json!({ "bugs": "https://x.example" }));
    let files = emit(&service, &options).unwrap();
    assert!(file(&files, "package.json").contains("\"bugs\": \"https://x.example\""));
    options.package_json = None;
    options.metadata.repository = Some("not a url".into());
    assert!(emit(&service, &options).is_err());
}

fn options(name: &str) -> Options {
    Options {
        package_name: name.into(),
        package_version: "0.0.0".into(),
        client_name: None,
        endpoint: None,
        middleware: vec![],
        package_json: None,
        metadata: Default::default(),
    }
}

fn file<'a>(files: &'a [crucible_codegen_ts::File], path: &str) -> &'a str {
    &files.iter().find(|f| f.path == path).unwrap().contents
}

#[test]
fn shapes_named_after_globals_do_not_shadow_them() {
    let service = load("globals");
    let files = emit(&service, &options("globals")).unwrap();
    let models = file(&files, "src/models.ts");
    assert!(models.contains("export interface Record {"), "{models}");
    assert!(models.contains("export type Map = globalThis.Record<string, Record>;"));
    assert!(
        models.contains(
            "export type ColorMap = globalThis.Partial<globalThis.Record<Color, Partial>>;"
        )
    );
    assert!(models.contains("bytes?: globalThis.Uint8Array;"));
    assert!(models.contains("buffer?: Uint8Array;"));
    let errors = file(&files, "src/errors.ts");
    assert!(errors.contains("export class Object extends GlobalsException"));
    assert!(errors.contains("fields: globalThis.Record<string, unknown>"));
    assert!(errors.contains(
        "if (globalThis.Object.hasOwn(fields, \"data\")) globalThis.Object.defineProperty("
    ));
    assert!(!errors.contains(" Object.") && !errors.contains("(Object."));
}

/// `globalThis` itself, the generated namespaces and reserved words cannot
/// be declared; each is a diagnostic rather than a client that fails to compile.
#[test]
fn reserved_names_are_diagnostics() {
    for name in ["globalThis", "m", "class"] {
        let service = weather_with(|text| text.replace("CitySummary", name));
        let diagnostics = emit(&service, &options("weather")).unwrap_err().to_string();
        assert!(
            diagnostics.contains(&format!("`{name}` is reserved")),
            "{diagnostics}"
        );
    }
}

#[test]
fn configured_client_cannot_shadow_the_service_exception() {
    let service = weather();
    let mut options = options("weather");
    options.client_name = Some("WeatherException".into());
    assert!(
        emit(&service, &options)
            .unwrap_err()
            .to_string()
            .contains("client name collides")
    );
}

/// `schemas.ts` imports `Schema`, `OperationSpec` and helpers such as `list`
/// by those names, so an operation called the same must not be declared under
/// the import's name (TS2395) yet stay reachable as `spec.Schema`.
#[test]
fn operations_named_like_schemas_imports_are_exported_under_an_alias() {
    let service = load("globals");
    let options = Options {
        package_name: "globals".into(),
        package_version: "0.0.0".into(),
        client_name: None,
        endpoint: None,
        middleware: vec![],
        package_json: None,
        metadata: Default::default(),
    };
    let files = emit(&service, &options).unwrap();
    let file = |path: &str| {
        &files
            .iter()
            .find(|file| file.path == path)
            .unwrap_or_else(|| panic!("{path}"))
            .contents
    };
    let schemas = file("src/schemas.ts");
    for name in ["Schema", "OperationSpec", "list"] {
        assert!(
            schemas.contains(&format!("const __op_{name}: OperationSpec = {{")),
            "{schemas}"
        );
        assert!(
            schemas.contains(&format!("export {{ __op_{name} as {name} }};")),
            "{schemas}"
        );
        assert!(
            !schemas.contains(&format!("export const {name}:")),
            "{schemas}"
        );
    }
    // Other operations keep their plain exports.
    assert!(schemas.contains("export const JSON: OperationSpec = {"));
    assert!(file("src/client.ts").contains("spec.Schema"));
}

/// The private alias for `Schema` is `__op_Schema`; an operation that already
/// has that name must not be redeclared by it.
#[test]
fn the_import_alias_avoids_another_operations_name() {
    let service = compile_with("globals", |text| {
        if !text.contains("service Globals") {
            return text;
        }
        text.replace("operations [", "operations [__op_Schema, ")
            + "\nreadonly operation __op_Schema {\n    http GET \"/op-schema\"\n}\n"
    })
    .unwrap_or_else(|rendered| panic!("globals compiles:\n{rendered}"));
    let options = Options {
        package_name: "globals".into(),
        package_version: "0.0.0".into(),
        client_name: None,
        endpoint: None,
        middleware: vec![],
        package_json: None,
        metadata: Default::default(),
    };
    let files = emit(&service, &options).unwrap();
    let schemas = &files
        .iter()
        .find(|file| file.path == "src/schemas.ts")
        .unwrap()
        .contents;
    assert!(
        schemas.contains("export const __op_Schema: OperationSpec = {"),
        "{schemas}"
    );
    assert!(
        schemas.contains("const ___op_Schema: OperationSpec = {"),
        "{schemas}"
    );
    assert!(
        schemas.contains("export { ___op_Schema as Schema };"),
        "{schemas}"
    );
}

#[test]
fn documentation_and_formatting_use_the_lowered_contract() {
    let service = load("client-docs");
    let files = emit(
        &service,
        &Options {
            package_name: "@example/docs".into(),
            package_version: "0.0.0".into(),
            client_name: None,
            endpoint: None,
            middleware: vec![],
            package_json: None,
            metadata: Default::default(),
        },
    )
    .unwrap();
    let models = &files
        .iter()
        .find(|f| f.path.ends_with("models.ts"))
        .unwrap()
        .contents;
    let client = &files
        .iter()
        .find(|f| f.path.ends_with("client.ts"))
        .unwrap()
        .contents;
    let runtime = &files
        .iter()
        .find(|f| f.path.ends_with("runtime.ts"))
        .unwrap()
        .contents;
    assert!(runtime.contains("Maximum retry backoff delay in milliseconds"));
    assert!(models.contains("https://example.com"));
    assert!(models.contains("line two"));
    assert!(models.contains("Label marker."));
    assert!(client.contains("Modeled errors: Problem"));
    assert!(client.contains("Construct") || client.contains("Construct with"));
    assert!(models.contains("export interface Record"));
}

#[test]
fn representation_method_collisions_are_diagnosed() {
    let service = load("client-docs-ts-collisions");
    let diagnostics = emit(
        &service,
        &Options {
            package_name: "@example/docs".into(),
            package_version: "0.0.0".into(),
            client_name: None,
            endpoint: None,
            middleware: vec![],
            package_json: None,
            metadata: Default::default(),
        },
    )
    .err()
    .unwrap();
    assert!(
        diagnostics
            .to_string()
            .contains("an error member cannot be named `toString` in TypeScript")
    );
}

#[test]
fn doc_links_are_tsdoc_link_tags() {
    let service =
        crucible_ir::project(&load("doc-links"), crucible_ir::Audience::External).unwrap();
    let files = emit(
        &service,
        &Options {
            package_name: "@example/links".into(),
            package_version: "0.0.0".into(),
            client_name: None,
            endpoint: None,
            middleware: vec![],
            package_json: None,
            metadata: Default::default(),
        },
    )
    .unwrap();
    let file = |name: &str| {
        files
            .iter()
            .find(|f| f.path.ends_with(name))
            .unwrap()
            .contents
            .clone()
    };
    let (models, client, errors) = (file("models.ts"), file("client.ts"), file("errors.ts"));
    // A type, a client method, a member and an enum value, as each file
    // names them: `client.ts` imports the models as `m`.
    assert!(
        models.contains(" * A widget; {@link __client.LinksClient.createWidget | LinksClient.createWidget} makes one."),
        "{models}"
    );
    assert!(
        models.contains(" * The name {@link CreateWidgetInput.name} chose."),
        "{models}"
    );
    assert!(
        models.contains(" * One of {@link Color}; {@link Color.BLUE} by default."),
        "{models}"
    );
    assert!(client.contains("Reads the {@link m.Widget | Widget} that {@link LinksClient.createWidget} made, by {@link m.GetWidgetInput.widgetId | GetWidgetInput.widgetId},"), "{client}");
    assert!(
        client.contains(
            "and returns it as {@link m.GetWidgetOutput.widget | GetWidgetOutput.widget}. Fails with {@link __errors.WidgetError | WidgetError}."
        ),
        "{client}"
    );
    assert!(errors.contains("Raised by {@link __client.LinksClient.getWidget | LinksClient.getWidget} when {@link m.GetWidgetInput.widgetId | GetWidgetInput.widgetId} names nothing."), "{errors}");
    // The service is the client; a resource, a prelude type, `[text][Name]`
    // and internal API are plain text, and code stays code.
    let overview = "the read call; a {@link m.Widget | Widget} has a {@link m.Widget.name | Widget.name} and a";
    assert!(client.contains(overview), "{client}");
    assert!(client.contains("{@link m.Color | Color}, such as {@link m.Color.RED | Color.RED}. Gadget is a resource, {@link LinksClient} is this"), "{client}");
    assert!(
        client.contains("service and String a prelude type. Internal: Reveal and Secret."),
        "{client}"
    );
    assert!(client.contains("`[NotALink]` in code"), "{client}");
    // A link to another file's symbol goes through a type-only namespace
    // import of that file, so it resolves where it is written.
    assert!(
        models.contains("import type * as __client from \"./client.js\";"),
        "{models}"
    );
    assert!(
        models.contains("import type * as __errors from \"./errors.js\";"),
        "{models}"
    );
    assert!(
        client.contains("import type * as __errors from \"./errors.js\";"),
        "{client}"
    );
    assert!(
        errors.contains("import type * as __client from \"./client.js\";"),
        "{errors}"
    );
    assert!(!client.contains("import type * as __client"), "{client}");
    for file in [&models, &client, &errors] {
        assert!(
            !file.contains("Secret}") && !file.contains("Reveal}") && !file.contains("{@link the")
        );
    }
}

/// Release v1 of the e2e versioned shop, an SDK line (docs/api-versions-
/// clients.md, 5c): the current model (v4) as v1 sees it. Its operations
/// send v1 in `Crucible-Version`, and its types are v1's: `max` where v2
/// renamed it `limit`, `name` where v2 renamed it `title`, the operation v3
/// removed, and the operations added since, in v1's shapes.
#[test]
fn a_v1_line_matches_golden_files() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../e2e/weather-service/versioned");
    let current = crucible_lang::load_versioned_service(&dir).unwrap_or_else(|e| panic!("{e}"));
    let line = crucible_ir::versions::line(&current, "v1").unwrap();
    let service = crucible_ir::client_view(
        &crucible_ir::project(&line, crucible_ir::Audience::External).unwrap(),
    );
    let options = Options {
        package_name: "@example/shop-v1".into(),
        package_version: "0.0.0-dev".into(),
        client_name: None,
        endpoint: None,
        middleware: vec![],
        package_json: None,
        metadata: Default::default(),
    };
    let files = emit(&service, &options).unwrap();
    let file = |path: &str| {
        files
            .iter()
            .find(|file| file.path == path)
            .unwrap_or_else(|| panic!("no {path}"))
            .contents
            .as_str()
    };
    let schemas = file("src/schemas.ts");
    assert!(schemas.contains(r#"apiVersion: "v1""#), "{schemas}");
    assert!(!schemas.contains(r#"apiVersion: "v4""#), "{schemas}");
    let client = file("src/client.ts");
    for method in ["countItems(", "getLabel(", "putLabel(", "listLabels("] {
        assert!(client.contains(method), "{method}");
    }
    let models = file("src/models.ts");
    assert!(models.contains("  max?: number;"), "{models}");
    assert!(models.contains("  name: string;"), "{models}");
    assert!(!file("src/errors.ts").contains("ItemGoneException"));
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/shop-v1");
    let bless = std::env::var_os("CRUCIBLE_BLESS").is_some();
    let mut stale = Vec::new();
    for file in &files {
        if file.path == "src/runtime.ts" {
            continue;
        }
        let path = golden.join(&file.path);
        if bless {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &file.contents).unwrap();
        } else if fs::read_to_string(&path)
            .map(|s| s.replace("\r\n", "\n"))
            .ok()
            .as_ref()
            != Some(&file.contents)
        {
            stale.push(file.path.clone());
        }
    }
    assert!(
        stale.is_empty(),
        "generated output differs from tests/golden/shop-v1 in {stale:?}; \
         review the change and rerun with CRUCIBLE_BLESS=1"
    );
}
