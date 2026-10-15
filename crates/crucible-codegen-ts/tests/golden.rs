//! The weather fixture's generated client, checked in, so every change to the
//! output shows in review. `CRUCIBLE_BLESS=1 cargo test` rewrites it.
//! `src/runtime.ts` is a verbatim copy of `runtime/runtime.ts` and is left out.

use std::fs;
use std::path::Path;

use crucible_codegen_ts::{Middleware, Options, emit};
use crucible_smithy::Model;

/// The typed permissions fixture, a `.cru` package; `edit` rewrites its
/// source first.
fn leyline_with(edit: impl Fn(String) -> String) -> crucible_smithy::ir::Service {
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

fn leyline() -> crucible_smithy::ir::Service {
    leyline_with(|text| text)
}

/// Typed permissions (docs/permissions.md §6.3): open constants per
/// resource, deprecated aliases, the registry, and each method's
/// requirements in its documentation; nothing of the internal tier.
#[test]
fn permission_constants_match_golden() {
    let external =
        crucible_smithy::project(&leyline(), crucible_smithy::Audience::External).unwrap();
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
    let weather = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
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
    let internal = crucible_smithy::project(&service, crucible_smithy::Audience::Internal).unwrap();
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
    let external = crucible_smithy::project(&service, crucible_smithy::Audience::External).unwrap();
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
    let external = crucible_smithy::project(&service, crucible_smithy::Audience::External).unwrap();
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
    let model = include_str!("../../crucible-smithy/tests/fixtures/weather.json");
    let service = Model::from_json(model).unwrap().lower(None).unwrap();
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
    let mut model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap();
    let shapes = model["shapes"].as_object_mut().unwrap();
    shapes.insert(
        "example.weather#Date".into(),
        serde_json::json!({ "type": "string" }),
    );
    shapes["example.weather#CitySummary"]["members"]["foundedOn"] =
        serde_json::json!({ "target": "example.weather#Date" });
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
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
    let mut model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap();
    model["shapes"]["example.weather#CitySummary"]["members"]["population"]["traits"]["smithy.api#default"] =
        serde_json::json!(i64::MAX);
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
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
    assert!(input.ends_with("[]);"));
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
    let mut model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap();
    model["shapes"]["example.weather#CitySummary"]["members"]["__proto__"] = serde_json::json!({
        "target": "smithy.api#Long", "traits": { "smithy.api#default": 3 }
    });
    model["shapes"]["example.weather#Weather"]["rename"] =
        serde_json::json!({ "example.weather#ConflictError": "__proto__" });
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
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
    assert!(schemas.contains("ConflictError: { status: 409"));
    assert!(schemas.contains("new e.__proto__(fields, metadata)"));
}

#[test]
fn generated_hardening_fixture_preserves_names_auth_and_client_defaults() {
    let service = Model::from_json(include_str!("fixtures/hardening.json"))
        .unwrap()
        .lower(None)
        .unwrap();
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
        .find(|line| line.starts_with("export const S_Defaults:"))
        .unwrap();
    assert!(defaults.contains("ordinary: parseJson(\"7\")"));
    assert!(!defaults.contains("optional: parseJson"));
    let input = schemas
        .lines()
        .find(|line| line.starts_with("export const S_DefaultInput:"))
        .unwrap();
    assert!(!input.contains("parseJson"));

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
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
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
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let files = emit(&service, &options("weather")).unwrap();
    let package: serde_json::Value = serde_json::from_str(file(&files, "package.json")).unwrap();
    assert_eq!(
        package["files"],
        serde_json::json!(["dist", "crucible.json", "crucible-model.json"])
    );
    assert!(
        !files
            .iter()
            .any(|f| f.path == "crucible.json" || f.path == "crucible-model.json")
    );
    assert!(
        !files
            .iter()
            .any(|f| f.path == "README.md" || f.path == "LICENSE")
    );
}

#[test]
fn package_json_cannot_override_what_the_package_owns() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
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
    let service = Model::from_json(include_str!("fixtures/globals.json"))
        .unwrap()
        .lower(None)
        .unwrap();
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
        let mut model: serde_json::Value = serde_json::from_str(include_str!(
            "../../crucible-smithy/tests/fixtures/weather.json"
        ))
        .unwrap();
        model["shapes"]["example.weather#Weather"]["rename"] =
            serde_json::json!({ "example.weather#CitySummary": name });
        let service = Model::from_json(&model.to_string())
            .unwrap()
            .lower(None)
            .unwrap();
        let diagnostics = emit(&service, &options("weather")).unwrap_err().to_string();
        assert!(
            diagnostics.contains(&format!("`{name}` is reserved")),
            "{diagnostics}"
        );
    }
}

#[test]
fn configured_client_cannot_shadow_the_service_exception() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
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
    let service = Model::from_json(include_str!("fixtures/globals.json"))
        .unwrap()
        .lower(None)
        .unwrap();
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
    let mut model: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/globals.json")).unwrap();
    let shapes = model["shapes"].as_object_mut().unwrap();
    let service_id = shapes
        .iter()
        .find(|(_, shape)| shape["type"] == "service")
        .map(|(id, _)| id.clone())
        .unwrap();
    shapes.insert(
        "example.globals#__op_Schema".into(),
        serde_json::json!({
            "type": "operation",
            "traits": { "smithy.api#http": { "method": "GET", "uri": "/op-schema" } }
        }),
    );
    shapes.get_mut(&service_id).unwrap()["operations"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({ "target": "example.globals#__op_Schema" }));
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
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
    let service = Model::from_json(include_str!("../../../tests/fixtures/client-docs.json"))
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_ts::capabilities())
        .unwrap();
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
    let service = Model::from_json(include_str!(
        "../../../tests/fixtures/client-docs-ts-collisions.json"
    ))
    .unwrap()
    .lower_with_capabilities(None, &crucible_codegen_ts::capabilities())
    .unwrap();
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
