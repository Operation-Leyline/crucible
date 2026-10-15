//! The weather fixture's generated code, checked in, so every change to the
//! output shows in review. `CRUCIBLE_BLESS=1 cargo test` rewrites it.

use std::fs;
use std::path::Path;

use crucible_codegen_rust::{Options, generate, generate_client};
use crucible_smithy::Model;
use serde_json::{Value, json};

const WEATHER: &str = include_str!("../../crucible-smithy/tests/fixtures/weather.json");

#[test]
fn scoped_server_metadata_matches_golden() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/scopes.json"
    ))
    .unwrap()
    .lower_with_capabilities(None, &crucible_codegen_rust::capabilities())
    .unwrap();
    let code = generate(&service, &Options::default()).unwrap();
    // The client marker has no use for scopes: they gate the server only.
    let client = generate_client(&service).unwrap();
    assert!(!client.contains("const SCOPES"), "{client}");
    matches_golden(&code, "scopes.rs");
}

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

/// An alias whose constant is one the enum already defines (`ALL`,
/// `RESOURCE`) is a diagnostic, not code that does not compile.
#[test]
fn an_alias_named_like_a_generated_constant_is_a_diagnostic() {
    for (symbol, constant) in [("all", "ALL"), ("resource", "RESOURCE")] {
        let service = leyline_with(|text| {
            text.replace(
                "@aliasOf(read)\n    permission view",
                &format!("@aliasOf(read)\n    permission {symbol}"),
            )
        });
        let diagnostics = generate(&service, &Options::default()).unwrap_err();
        assert!(
            diagnostics.iter().any(|d| d.message
                == format!(
                    "permission `{symbol}` generates `GraphPermission::{constant}`, as the generated `{constant}` does"
                )),
            "{diagnostics:?}"
        );
    }
}

/// Typed permissions (docs/permissions.md §6.2): the descriptions, the
/// closed enums and typed references, and per operation its requirements,
/// the bindings' `CHECKS` and its handler obligations.
#[test]
fn permissions_server_bindings_match_golden() {
    let service = leyline();
    matches_golden(
        &generate(&service, &Options::default()).unwrap(),
        "permissions.rs",
    );
    // The client sees the external audience: open constants, no internal
    // permission, and the requirements in each method's documentation.
    let external = crucible_smithy::project(&service, crucible_smithy::Audience::External).unwrap();
    let client = generate_client(&external).unwrap();
    assert!(!client.contains("organization:read"), "{client}");
    assert!(!client.contains("const REQUIREMENTS"), "{client}");
    matches_golden(&client, "permissions_client.rs");
    // MCP tools carry the sentence too, from the External projection.
    let mcp = generate(
        &service,
        &Options {
            mcp: true,
            ..Options::default()
        },
    )
    .unwrap();
    let tools = mcp.rsplit_once("pub static MCP_TOOLS").unwrap().1;
    assert!(tools.contains("Requires no permission."), "{tools}");
    assert!(
        tools.contains("Requires `graph:write` on every graph in the organization."),
        "{tools}"
    );
    assert!(!tools.contains("organization:read"), "{tools}");
}

/// A service with errors of its own that designates its 404
/// (`notFound ResourceNotFoundException`): the service description names
/// it, and a typed reference's `not_found()` asks the service's factory,
/// the one that answers a hidden resource.
#[test]
fn a_designated_not_found_error_matches_golden() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/permissions-not-found");
    let compiled = crucible_lang::compile(&crucible_lang::load_package(&dir).unwrap(), None);
    let rendered = compiled.render();
    // Only the pending operation's reminder.
    assert_eq!(compiled.diagnostics.len(), 1, "\n{rendered}");
    assert!(rendered.contains("permissions.pending"), "\n{rendered}");
    let service = compiled.service.expect("the fixture compiles");
    // The designated 404 is declared under `requires pending` too, where
    // the handler still checks; not under `requires none`.
    let declares = |name: &str| {
        let op = service
            .operations
            .iter()
            .find(|op| op.id.name() == name)
            .unwrap();
        op.errors
            .iter()
            .any(|e| e.name() == "ResourceNotFoundException")
    };
    assert!(declares("RenameRoom") && declares("GetRoom") && !declares("Whoami"));
    let code = generate(&service, &Options::default()).unwrap();
    assert!(
        code.contains(".with_not_found(\"example.desk#ResourceNotFoundException\")"),
        "{code}"
    );
    assert!(!code.contains("crucible.errors"), "{code}");
    matches_golden(&code, "permissions_not_found.rs");
    // Without the designation nothing changes: the references answer the
    // framework's 404 themselves.
    let leyline = generate(&leyline(), &Options::default()).unwrap();
    assert!(!leyline.contains("with_not_found"), "{leyline}");
    assert!(!leyline.contains("permission_not_found"), "{leyline}");
}

#[test]
fn generated_patterns_use_ecma_classes_and_line_terminators() {
    for (pattern, accepted, rejected) in [
        (r"^\d+$", "12", vec!["١٢", "１"]),
        (r"^\w+$", "a_2", vec!["é"]),
        (
            r"^a.b$",
            "aéb",
            vec!["a\rb", "a\u{2028}b", "a\u{2029}b", "a\nb"],
        ),
        (r"^\s$", "\u{feff}", vec!["\u{85}"]),
        (r"\ba\b", "éaé", vec!["bab"]),
        (r"^\Bé\B$", "é", vec!["aé"]),
        (r"^[\d\w]+$", "a_12", vec!["é", "١"]),
        (r"^\D+$", "é١", vec!["12"]),
        (r"^\W+$", "é", vec!["a"]),
        (r"^\S+$", "\u{85}", vec!["\u{feff}"]),
        (r"^\\d\.$", r"\d.", vec!["12x"]),
        (r"^[\b.]$", "\u{8}", vec!["a"]),
        (r"^\u00e9$", "é", vec!["e"]),
        (r"^[a&&b]$", "&", vec!["c"]),
        (r"\d+", "!12!", vec!["!é!"]),
    ] {
        let service = edited(|model| {
            model["shapes"]["example.weather#CityId"]["traits"]["smithy.api#pattern"] =
                json!(pattern);
        });
        let code = generate(&service, &Options::default()).unwrap();
        let literal = code
            .split("::regex::Regex::new(")
            .nth(1)
            .unwrap()
            .split(").expect(")
            .next()
            .unwrap();
        let translated: String = serde_json::from_str(literal).unwrap();
        let regex = regex::Regex::new(&translated).unwrap();
        assert!(
            regex.is_match(accepted),
            "{pattern:?} rejected {accepted:?}"
        );
        for text in rejected {
            assert!(!regex.is_match(text), "{pattern:?} accepted {text:?}");
        }
    }
}

#[test]
fn generated_empty_ecma_classes_preserve_matching_and_following_tokens() {
    for (pattern, accepted, rejected) in [
        ("^[]$", vec![], vec!["", "a", "]", "\n", "🦀"]),
        ("^[]x]$", vec![], vec!["x]", "ax]", "]x]"]),
        ("^[]*$", vec![""], vec!["a", "]"]),
        ("^(?:[]|x)$", vec!["x"], vec!["", "]", "a"]),
        (
            "^[^]$",
            vec!["a", "]", "\n", "\r", "\u{2028}", "\u{2029}", "é"],
            vec!["", "ab"],
        ),
        ("^[^]x]$", vec!["ax]", "\nx]", "éx]"], vec!["a", "x]", "ax"]),
        (r"^[\]]$", vec!["]"], vec!["", "a"]),
    ] {
        let service = edited(|model| {
            model["shapes"]["example.weather#CityId"]["traits"]["smithy.api#pattern"] =
                json!(pattern);
        });
        for code in [
            generate(&service, &Options::default()).unwrap(),
            generate_client(&service).unwrap(),
        ] {
            let literal = code
                .split("::regex::Regex::new(")
                .nth(1)
                .unwrap()
                .split(").expect(")
                .next()
                .unwrap();
            let translated: String = serde_json::from_str(literal).unwrap();
            let regex = regex::Regex::new(&translated).unwrap();
            for text in &accepted {
                assert!(regex.is_match(text), "{pattern:?} rejected {text:?}");
            }
            for text in &rejected {
                assert!(!regex.is_match(text), "{pattern:?} accepted {text:?}");
            }
        }
    }
}

#[test]
fn ecma_pattern_statics_match_the_golden_file() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/patterns.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let code = generate(&service, &Options::default()).unwrap();
    let statics = code
        .split("static PATTERN_")
        .skip(1)
        .map(|part| format!("static PATTERN_{}));\n", part.split("));").next().unwrap()))
        .collect::<String>();
    matches_golden(&statics, "patterns.rs");
}

#[test]
fn unsupported_ecma_patterns_collect_shape_diagnostics() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/patterns-unsupported.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let diagnostics = generate(&service, &Options::default()).unwrap_err();
    for member in [
        "flags",
        "anchor",
        "property",
        "surrogate",
        "lookahead",
        "backreference",
        "wordstart",
        "wordend",
    ] {
        assert!(
            diagnostics.iter().any(|d| d.shape.to_string()
                == format!("example.patterns#Input${member}")
                && d.message.contains("does not compile faithfully")),
            "{diagnostics}"
        );
    }
}

#[test]
fn service_exception_uses_rust_type_spelling() {
    let mut service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    service.id = crucible_smithy::ShapeId::new("example.weather", "weather_api");
    let source = generate_client(&service).unwrap();
    assert!(source.contains("pub enum WeatherApiException"));
}

#[test]
fn standard_exception_models_and_answer_functions_match_the_golden_file() {
    let source = include_str!("../../crucible-smithy/tests/fixtures/standard-service-errors.json");
    let service = Model::from_json(source).unwrap().lower(None).unwrap();
    let code = generate(&service, &Options::default()).unwrap();
    // The existing registration tests cover empty services; this snapshot
    // records the shared exception types, wire metadata and response factories.
    let errors = code
        .split("/// The service, as the framework sees it.")
        .next()
        .unwrap();
    matches_golden(&format!("{}\n", errors.trim_end()), "standard_errors.rs");
}

#[test]
fn empty_services_can_generate_their_declared_rejection_errors() {
    let source = include_str!("../../crucible-smithy/tests/fixtures/empty-service-errors.json");
    let service = Model::from_json(source).unwrap().lower(None).unwrap();
    let code = generate(
        &service,
        &Options {
            require_rejection: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert!(code.contains("pub struct ValidationError"));
    assert!(code.contains("pub struct InternalError"));
    assert!(code.contains("Some(reject),\n        Some(internal),"));
}

#[test]
fn high_level_services_diagnose_missing_rejection_errors_before_startup() {
    let model = include_str!("../../../tests/conformance/empty-service.json");
    let service = Model::from_json(model).unwrap().lower(None).unwrap();
    let diagnostics = generate(
        &service,
        &Options {
            require_rejection: true,
            ..Options::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert!(diagnostics.contains(&service.id.to_string()));
    assert!(diagnostics.contains("errors: [ValidationException]"));
    assert!(diagnostics.contains("custom_rejections()"));
    // The existing JSON-AST path can still supply its own runtime handler.
    assert!(generate(&service, &Options::default()).is_ok());
}

#[test]
fn requiring_a_rejection_error_preserves_a_valid_services_generated_code() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let options = Options {
        require_rejection: true,
        ..Options::default()
    };
    assert_eq!(
        generate(&service, &options).unwrap(),
        generate(&service, &Options::default()).unwrap()
    );
}

#[test]
fn empty_services_do_not_import_unused_redaction_types() {
    let model = include_str!("../../../tests/conformance/empty-service.json");
    let service = Model::from_json(model).unwrap().lower(None).unwrap();
    for code in [
        generate_client(&service).unwrap(),
        generate(&service, &Options::default()).unwrap(),
    ] {
        assert!(
            code.lines()
                .map(str::trim)
                .collect::<Vec<_>>()
                .join("\n")
                .contains("mod __redaction {\n}")
        );
        assert!(!code.contains("operation::Redaction;"));
    }
}

#[test]
fn client_optional_defaults_are_removed_only_from_client_code() {
    let model = include_str!("../../crucible-smithy/tests/fixtures/review-hardening.json");
    let service = Model::from_json(model).unwrap().lower(None).unwrap();
    let client = generate_client(&service).unwrap();
    let server = generate(&service, &Options::default()).unwrap();
    assert!(!client.contains("defaults::values__optional_default"));
    assert!(server.contains("defaults::values__optional_default"));
    assert!(client.contains("fn classify(status:"));
    assert!(client.contains("const AUTH_BEARER: bool = false;"));
    assert!(client.contains("const AUTH_BEARER: bool = true;"));
}

#[test]
fn service_client_name_cannot_collide_with_a_model() {
    let service = edited(|model| {
        model["shapes"]["example.weather#Weather"]["rename"] =
            json!({"example.weather#CitySummary":"WeatherClient"});
    });
    let diagnostics = generate_client(&service).unwrap_err();
    assert!(
        diagnostics
            .to_string()
            .contains("generated client `WeatherClient`")
    );
}

fn edited(edit: impl FnOnce(&mut Value)) -> crucible_smithy::ir::Service {
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    edit(&mut model);
    // These tests reshape weather's shapes; its examples describe the original ones.
    for shape in model["shapes"].as_object_mut().unwrap().values_mut() {
        if let Some(traits) = shape.get_mut("traits").and_then(|t| t.as_object_mut()) {
            traits.remove("smithy.api#examples");
        }
    }
    Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap()
}

#[test]
fn float_ranges_use_the_members_precision() {
    let service = edited(|model| {
        let members = &mut model["shapes"]["example.weather#Coordinates"]["members"];
        members["latitude"]["target"] = json!("smithy.api#Float");
        members["latitude"]["traits"]["smithy.api#range"] = json!({"min": 0.7});
        members["longitude"]["traits"]["smithy.api#range"] = json!({"max": 0.1});
    });
    let code = generate(&service, &Options::default()).unwrap();
    assert!(code.contains("validator.range_float32(*value, Some(0.7f64), None);"));
    assert!(code.contains("validator.range_float(*value as f64, None, Some(0.1f64));"));
    let checks = code
        .lines()
        .filter(|line| line.contains("validator.range_float"))
        .map(|line| format!("{}\n", line.trim()))
        .collect::<String>();
    matches_golden(&checks, "float_ranges.rs");
}

#[test]
fn weather_matches_the_golden_file() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let code = generate(&service, &Options::default()).unwrap();
    matches_golden(&code, "weather.rs");
}

#[test]
fn weather_client_matches_the_golden_file() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let code = generate_client(&service).unwrap();
    matches_golden(&code, "weather_client.rs");
}

#[test]
fn non_throttling_retryability_reaches_client_metadata() {
    let service = edited(|model| {
        model["shapes"]["example.weather#ConflictError"]["traits"]["smithy.api#retryable"] =
            json!({});
    });
    let code = generate_client(&service).unwrap();
    assert!(
        code.contains("Fault::Client,\n        true,\n        false,"),
        "{code}"
    );
}

#[test]
fn a_sensitive_blob_payload_is_redacted_from_logs() {
    let service = edited(|model| {
        model["shapes"]["example.weather#Photo"]["traits"]["smithy.api#sensitive"] = json!({});
    });
    let code = generate(&service, &Options::default()).unwrap();
    assert!(
        code.contains(
            "member_info(\"photo\", ::crucible_server_runtime_rust::__generated::operation::Location::Payload(\"image/jpeg\"), ::crucible_server_runtime_rust::__generated::operation::Scalar::Json, &::crucible_server_runtime_rust::__generated::operation::Redaction::Sensitive"
        ),
        "{code}"
    );
}

fn matches_golden(code: &str, file: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(file);
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, code).unwrap();
    }
    let golden = fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        golden == code,
        "generated code differs from tests/golden/{file}; review it and rerun with CRUCIBLE_BLESS=1"
    );
}

#[test]
fn a_validation_error_crucible_cannot_fill_leaves_rejection_to_the_service() {
    let service = edited(|model| {
        model["shapes"]["example.weather#ValidationError"]["members"]["reason"] =
            json!({ "target": "smithy.api#String", "traits": { "smithy.api#required": {} } });
    });
    let code = generate(&service, &Options::default()).unwrap();
    assert!(code.contains("classify,\n        None,"), "{code}");
    assert!(!code.contains("fn reject("));
}

#[test]
fn a_pattern_the_rust_regex_crate_cannot_run_is_a_diagnostic() {
    let service = edited(|model| {
        model["shapes"]["example.weather#CityId"]["traits"]["smithy.api#pattern"] =
            json!("^(?!x)c-.*$");
    });
    let diagnostics = generate(&service, &Options::default()).unwrap_err();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("does not compile")),
        "{diagnostics}"
    );
}

#[test]
fn a_configured_error_that_is_not_an_error_fails_generation() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    for require_rejection in [false, true] {
        let options = Options {
            require_rejection,
            validation_error: Some("example.weather#CitySummary".parse().unwrap()),
            internal_error: Some("example.weather#Missing".parse().unwrap()),
            ..Options::default()
        };
        let diagnostics = generate(&service, &options).unwrap_err();
        let messages: Vec<String> = diagnostics.iter().map(ToString::to_string).collect();
        assert_eq!(messages.len(), 2, "{messages:?}");
        assert!(
            messages
                .iter()
                .all(|m| m.contains("is not an @error structure"))
        );
    }
}

#[test]
fn a_configured_validation_error_crucible_cannot_fill_fails_generation() {
    let service = edited(|model| {
        model["shapes"]["example.weather#ConflictError"]["members"]["reason"] =
            json!({ "target": "smithy.api#String", "traits": { "smithy.api#required": {} } });
    });
    for require_rejection in [false, true] {
        let options = Options {
            require_rejection,
            validation_error: Some("example.weather#ConflictError".parse().unwrap()),
            ..Options::default()
        };
        let diagnostics = generate(&service, &options).unwrap_err();
        assert_eq!(diagnostics.len(), 1, "{diagnostics}");
        assert_eq!(
            diagnostics.to_string().contains("custom_rejections()"),
            require_rejection
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("on_rejection")),
            "{diagnostics}"
        );
    }
}

#[test]
fn a_paginated_operation_fills_a_fresh_token_per_page_unless_the_caller_set_one() {
    let service = edited(|model| {
        model["shapes"]["example.weather#ListCitiesInput"]["members"]["clientToken"] = json!({ "target": "smithy.api#String", "traits": { "smithy.api#idempotencyToken": {} } });
    });
    let code = generate_client(&service).unwrap();
    assert!(
        code.contains("let fresh = input.client_token.value().is_none();"),
        "{code}"
    );
    assert!(code.contains(
        "pages.fresh_token(|input| input.client_token = ::crucible_client_runtime_rust::__generated::Field::Value(::crucible_client_runtime_rust::__generated::idempotency_token()))"
    ));
}

#[test]
fn strict_structures_refuse_unknown_members_on_the_server_only() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let strict = "#[serde(deny_unknown_fields)]\n        pub struct ";
    let marked = generate(&service, &Options::default()).unwrap();
    assert!(marked.contains(&format!("{strict}CreateCityInput")));
    assert!(
        !marked.contains(&format!("{strict}GetCityInput")),
        "unmarked"
    );

    let all = Options {
        strict_inputs: true,
        ..Options::default()
    };
    let everything = generate(&service, &all).unwrap();
    for input in ["GetCityInput", "ListCitiesInput", "Coordinates"] {
        assert!(everything.contains(&format!("{strict}{input}")), "{input}");
    }
    assert!(
        !everything.contains(&format!("{strict}CitySummary")),
        "output only"
    );

    let client = generate_client(&service).unwrap();
    assert!(
        !client.contains("deny_unknown_fields"),
        "clients stay lenient"
    );
}

#[test]
fn without_the_standard_reason_an_unknown_operation_keeps_the_protocol_answer() {
    let service = edited(|model| {
        let members = &mut model["shapes"]["example.weather#ValidationError"]["members"];
        members.as_object_mut().unwrap().remove("reason");
    });
    let code = generate(&service, &Options::default()).unwrap();
    assert!(
        code.contains("Some(reject),\n        None,\n        None,\n    );"),
        "{code}"
    );
    assert!(!code.contains("fn unknown("));
}

#[test]
fn the_standard_reason_matches_whatever_casing_the_model_uses() {
    let service = edited(|model| {
        let members = &mut model["shapes"]["example.weather#ValidationReason"]["members"];
        for (_, member) in members.as_object_mut().unwrap() {
            let value = member["traits"]["smithy.api#enumValue"].as_str().unwrap();
            let snake: String = value
                .chars()
                .flat_map(|c| {
                    if c.is_ascii_uppercase() {
                        vec!['_', c.to_ascii_lowercase()]
                    } else {
                        vec![c]
                    }
                })
                .collect();
            member["traits"]["smithy.api#enumValue"] = json!(snake);
        }
    });
    let code = generate(&service, &Options::default()).unwrap();
    assert!(code.contains("model::ValidationReason::CannotParse } else { model::ValidationReason::FieldValidationFailed }"));
    assert!(code.contains("reason: model::ValidationReason::UnknownOperation"));
}

#[test]
fn scaffold_plans_a_file_per_operation_in_a_folder_per_resource() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let stubs = crucible_codegen_rust::scaffold(&service, &Default::default());
    let mut paths: Vec<String> = stubs
        .iter()
        .map(|stub| stub.path().to_string_lossy().replace('\\', "/"))
        .collect();
    paths.sort();
    assert_eq!(
        paths,
        [
            "city/create_city.rs",
            "city/get_city.rs",
            "city/get_city_photo.rs",
            "city/list_cities.rs",
            "city/put_city_photo.rs",
            "service/ping.rs",
            "service/status.rs",
        ]
    );
    let get_city = stubs.iter().find(|stub| stub.module == "get_city").unwrap();
    assert!(
        get_city
            .contents
            .contains("use crate::weather::{GetCityInput, GetCityOperation, GetCityOutput};")
    );
    assert!(
        get_city
            .contents
            .contains("impl GetCityOperation for Service {")
    );
    assert!(
        get_city
            .contents
            .contains("async fn get_city(&self, input: GetCityInput) -> Result<GetCityOutput>")
    );
    let ping = stubs.iter().find(|stub| stub.module == "ping").unwrap();
    assert!(
        ping.contents
            .contains("use crate::weather::{PingOperation};"),
        "{}",
        ping.contents
    );
}

#[test]
fn scaffold_escapes_keyword_identifiers_in_modules_and_preserves_file_stems() {
    let model = r#"{
        "smithy": "2.0",
        "shapes": {
            "example#Service": {
                "type": "service",
                "version": "2026-01-01",
                "operations": [
                    { "target": "example#Type" },
                    { "target": "example#Self" }
                ],
                "resources": [
                    { "target": "example#Match" },
                    { "target": "example#Super" }
                ],
                "traits": {
                    "crucible.protocols#json": {}
                }
            },
            "example#Match": {
                "type": "resource",
                "operations": [
                    { "target": "example#Return" },
                    { "target": "example#Move" }
                ]
            },
            "example#Super": {
                "type": "resource",
                "operations": [
                    { "target": "example#Crate" }
                ]
            },
            "example#Type": {
                "type": "operation",
                "traits": { "smithy.api#http": { "method": "GET", "uri": "/type" } }
            },
            "example#Self": {
                "type": "operation",
                "traits": { "smithy.api#http": { "method": "GET", "uri": "/self" } }
            },
            "example#Return": {
                "type": "operation",
                "traits": { "smithy.api#http": { "method": "GET", "uri": "/return" } }
            },
            "example#Move": {
                "type": "operation",
                "traits": { "smithy.api#http": { "method": "GET", "uri": "/move" } }
            },
            "example#Crate": {
                "type": "operation",
                "traits": { "smithy.api#http": { "method": "GET", "uri": "/crate" } }
            }
        }
    }"#;
    let service = Model::from_json(model).unwrap().lower(None).unwrap();
    let stubs = crucible_codegen_rust::scaffold(&service, &Default::default());

    let mut paths: Vec<String> = stubs
        .iter()
        .map(|stub| stub.path().to_string_lossy().replace('\\', "/"))
        .collect();
    paths.sort();
    assert_eq!(
        paths,
        [
            "match/move.rs",
            "match/return.rs",
            "service/self_.rs",
            "service/type.rs",
            "super_/crate_.rs",
        ]
    );

    let type_stub = stubs.iter().find(|stub| stub.file_stem == "type").unwrap();
    assert_eq!(type_stub.module, "r#type");
    assert_eq!(type_stub.file_stem, "type");
    assert_eq!(type_stub.folder, "service");
    assert!(
        type_stub
            .contents
            .contains("impl TypeOperation for Service {")
    );
    assert!(type_stub.contents.contains("async fn r#type(&self)"));

    let self_stub = stubs.iter().find(|stub| stub.file_stem == "self_").unwrap();
    assert_eq!(self_stub.module, "self_");
    assert_eq!(self_stub.file_stem, "self_");
    assert!(
        self_stub
            .contents
            .contains("impl SelfOperation for Service {")
    );
    assert!(self_stub.contents.contains("async fn self_(&self)"));

    let crate_stub = stubs
        .iter()
        .find(|stub| stub.file_stem == "crate_")
        .unwrap();
    assert_eq!(crate_stub.module, "crate_");
    assert_eq!(crate_stub.folder, "super_");
}

#[test]
fn payload_kinds_and_media_headers_reach_client_markers() {
    let mut ast: Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/error-payloads.json"
    ))
    .unwrap();
    // Flat SDK types need distinct names for the exception and operation enum.
    ast["shapes"]["example.wire#Wire"]["rename"] = json!({"example.wire#TextError": "TextFault"});
    let service = Model::from_json(&ast.to_string())
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_rust::client_capabilities())
        .unwrap();
    let client = generate_client(&service).unwrap();
    assert!(client.contains("Location::TextPayload(\"text/plain\")"));
    assert!(client.contains("Location::DocumentPayload(\"application/json\")"));
    assert!(client.contains(
        "Scalar::List(&::crucible_client_runtime_rust::__generated::operation::Scalar::Base64)"
    ));
    // The error's payload is described too, so its body decodes as text.
    assert!(client.contains(
        "member_info(\"details\", ::crucible_client_runtime_rust::__generated::operation::Location::TextPayload"
    ));
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/error-payloads.json"
    ))
    .unwrap()
    .lower_with_capabilities(None, &crucible_codegen_rust::capabilities())
    .unwrap();
    assert!(
        generate(&service, &Options::default())
            .unwrap()
            .contains("Scalar::Base64")
    );
}

#[test]
fn finite_only_float_members_use_checked_codec() {
    let service = edited(|model| {
        model["shapes"]["example.weather#Coordinates"]["members"]["latitude"]["target"] =
            json!("smithy.api#Float");
    });
    let server = generate(&service, &Options::default()).unwrap();
    let client = generate_client(&service).unwrap();
    assert!(
        server
            .contains("Adapter::<::crucible_server_runtime_rust::__generated::codec::FiniteFloat>")
    );
    assert!(
        client
            .contains("Adapter::<::crucible_client_runtime_rust::__generated::codec::FiniteFloat>")
    );
    let adapters = [&server, &client]
        .into_iter()
        .flat_map(|code| code.lines())
        .filter(|line| line.contains("codec::FiniteFloat"))
        .map(|line| format!("{}\n", line.trim()))
        .collect::<String>();
    matches_golden(&adapters, "finite_float.rs");
}

#[test]
fn internal_audience_server_and_projected_client_match_goldens() {
    let full = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/internal.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let external = crucible_smithy::project(&full, crucible_smithy::Audience::External).unwrap();
    matches_golden(
        &generate(&full, &Options::default()).unwrap(),
        "internal_server.rs",
    );
    matches_golden(
        &generate_client(&external).unwrap(),
        "internal_external_client.rs",
    );
    matches_golden(&generate_client(&full).unwrap(), "internal_client.rs");
}

#[test]
fn only_tokened_operations_with_buffered_outputs_can_replay() {
    let code = generate(&edited(|_| {}), &Options::default()).unwrap();
    // CreateCity has a token and a buffered output; nothing else replays.
    assert_eq!(code.matches("const IDEMPOTENCY_TOKEN").count(), 1, "{code}");
    assert_eq!(code.matches("const OUTPUT_FROM_JSON").count(), 1, "{code}");
    assert!(code.contains("PaginationMember { name: \"clientToken\" }"));
    assert!(
        !generate_client(&edited(|_| {}))
            .unwrap()
            .contains("IDEMPOTENCY_TOKEN")
    );
}
