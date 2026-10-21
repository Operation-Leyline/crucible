use std::fs;
use std::path::Path;

use crucible_codegen_openapi::{Options, generate};
use serde_json::{Value, json};

fn package_dir(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
}

fn load(name: &str) -> crucible_ir::ir::Service {
    crucible_lang::load_service(&package_dir(name))
        .unwrap_or_else(|rendered| panic!("{name}:\n{rendered}"))
}

fn weather() -> crucible_ir::ir::Service {
    load("weather")
}

#[test]
fn response_stream_documents_framing_and_item_schema() {
    let output = generate(&load("server-stream"), &Options::default()).unwrap();
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/server-stream.openapi.json");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::write(&path, &output).unwrap();
    }
    assert_eq!(
        fs::read_to_string(path).unwrap().replace("\r\n", "\n"),
        output
    );
    let document: Value = serde_json::from_str(&output).unwrap();
    let success = &document["paths"]["/events"]["get"]["responses"]["200"];
    assert!(success["content"]["text/event-stream"].is_object());
    let stream = &success["content"]["text/event-stream"]["x-crucible-stream"];
    assert_eq!(stream["profile"], 1);
    assert!(stream["itemSchema"]["$ref"].is_string());
    assert_eq!(stream["itemExamples"]["Ready"]["value"]["ready"], "ready");
    assert_eq!(
        stream["errorSchema"]["required"],
        json!(["type", "status", "error"])
    );
    assert!(
        stream["modeledErrors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error["type"] == "WorkFailed" && error["status"] == 503)
    );
}

/// Root and nested event values use the shared document encoding.
#[test]
fn stream_item_examples_use_the_same_document_encoding_as_items() {
    let mut package = crucible_lang::load_package(&package_dir("server-stream")).unwrap();
    for file in &mut package.files {
        file.text = file
            .text
            .replace("ready: string", "ready: timestamp")
            .replace("ready: \"ready\"", "ready: 0");
    }
    let compiled = crucible_lang::compile(&package, None);
    let service = compiled
        .service
        .as_ref()
        .unwrap_or_else(|| panic!("{}", compiled.render()));
    let output: Value =
        serde_json::from_str(&generate(service, &Options::default()).unwrap()).unwrap();
    let example = &output["paths"]["/events"]["get"]["responses"]["200"]["content"]["text/event-stream"]
        ["x-crucible-stream"]["itemExamples"]["Ready"]["value"];
    assert_eq!(example["ready"], "1970-01-01T00:00:00Z");
}

/// Weather with each `(old, new)` pair replaced in its sources.
fn edited(edits: &[(&str, &str)]) -> crucible_ir::ir::Service {
    try_edited(edits).unwrap_or_else(|rendered| panic!("the edited weather compiles:\n{rendered}"))
}

/// [`edited`], or the diagnostics when the edited package does not compile.
fn try_edited(edits: &[(&str, &str)]) -> Result<crucible_ir::ir::Service, String> {
    let mut package = crucible_lang::load_package(&package_dir("weather")).unwrap();
    for (old, new) in edits {
        let mut found = false;
        for file in &mut package.files {
            file.text = file.text.replace("\r\n", "\n");
            if file.text.contains(old) {
                file.text = file.text.replace(old, new);
                found = true;
            }
        }
        assert!(found, "weather has no `{old}`");
    }
    let compiled = crucible_lang::compile(&package, None);
    match compiled.service.clone() {
        Some(service) => Ok(service),
        None => Err(compiled.render()),
    }
}

fn document(service: &crucible_ir::ir::Service) -> Value {
    serde_json::from_str(&generate(service, &Options::default()).unwrap()).unwrap()
}

#[test]
fn weather_matches_reviewed_golden() {
    let service = weather();
    let output = generate(
        &service,
        &Options {
            endpoint: Some("http://localhost:8080".into()),
        },
    )
    .unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/weather.openapi.json");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &output).unwrap();
    }
    assert_eq!(
        fs::read_to_string(path).unwrap().replace("\r\n", "\n"),
        output
    );
}

/// The release header (docs/api-versions-clients.md, 5a and 5c) is a
/// required parameter fixed to the release the document describes: the
/// current one, or a line's own. Checked in for the e2e versioned shop,
/// its current release (v4) and its v1 line.
#[test]
fn every_request_names_the_documents_release() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../e2e/weather-service/versioned");
    let current = crucible_lang::load_versioned_service(&dir).unwrap_or_else(|e| panic!("{e}"));
    for (release, file) in [
        ("v4", "versioned.openapi.json"),
        ("v1", "versioned-v1.openapi.json"),
    ] {
        let line = crucible_ir::versions::line(&current, release).unwrap();
        let service = crucible_ir::project(&line, crucible_ir::Audience::External).unwrap();
        let output = generate(&service, &Options::default()).unwrap();
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(file);
        if std::env::var_os("CRUCIBLE_BLESS").is_some() {
            fs::write(&path, &output).unwrap();
        }
        assert_eq!(
            fs::read_to_string(&path)
                .expect("golden exists; bless with CRUCIBLE_BLESS=1")
                .replace("\r\n", "\n"),
            output,
            "{file}"
        );
        let document: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(document["info"]["version"], release);
        for (path, methods) in document["paths"].as_object().unwrap() {
            for (method, operation) in methods.as_object().unwrap() {
                let header = operation["parameters"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|parameter| parameter["name"] == "Crucible-Version");
                if operation.get("x-crucible-sender").is_some() {
                    // A third party's request names no release.
                    assert!(header.is_none(), "{method} {path}");
                    continue;
                }
                let header = header.unwrap_or_else(|| panic!("{method} {path}"));
                assert_eq!(header["in"], "header");
                assert_eq!(header["required"], true);
                assert_eq!(
                    header["schema"],
                    json!({"type": "string", "const": release})
                );
            }
        }
    }
}

/// Sender-signed operations (decision 81) carry `x-crucible-sender` and no
/// security requirement; the operation clients call has neither.
#[test]
fn sender_signed_operations_match_reviewed_golden() {
    let output = generate(&load("sender-signed"), &Options::default()).unwrap();
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/sender-signed.openapi.json");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::write(&path, &output).unwrap();
    }
    assert_eq!(
        fs::read_to_string(&path).unwrap().replace("\r\n", "\n"),
        output
    );
    let document: Value = serde_json::from_str(&output).unwrap();
    let payment = &document["paths"]["/payment-events"]["post"];
    assert_eq!(
        payment["x-crucible-sender"],
        serde_json::json!({"name": "Payments Inc.", "member": "signature", "header": "Payment-Signature"})
    );
    assert_eq!(payment["security"], serde_json::json!([]));
    assert!(
        document["paths"]["/accounts/{accountId}"]["get"]
            .get("x-crucible-sender")
            .is_none()
    );
}

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

/// Typed permissions (docs/permissions.md §6.3): `x-crucible-requires` per
/// operation, `x-crucible-permissions` for the registry, a requirement
/// sentence in each description, and the framework's 404 among the
/// responses. Internal permissions only in the server-audience document.
#[test]
fn permissions_match_reviewed_golden_per_audience() {
    let service = leyline();
    for (audience, file) in [
        (crucible_ir::Audience::Internal, "permissions.openapi.json"),
        (
            crucible_ir::Audience::External,
            "permissions-external.openapi.json",
        ),
    ] {
        let projected = crucible_ir::project(&service, audience).unwrap();
        let generated = generate(&projected, &Options::default()).unwrap();
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(file);
        if std::env::var_os("CRUCIBLE_BLESS").is_some() {
            std::fs::write(&path, &generated).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(&path)
                .unwrap()
                .replace("\r\n", "\n"),
            generated,
            "{file}"
        );
        // Every internal value (and an alias of one) appears only in the
        // server-audience document, and nothing there carries the tier.
        let document: serde_json::Value = serde_json::from_str(&generated).unwrap();
        let mut strings = Vec::new();
        strings_of(&document, &mut strings);
        for value in internal_values(&service) {
            let named = strings.iter().any(|s| s.contains(&value));
            assert_eq!(
                named,
                audience == crucible_ir::Audience::Internal,
                "{file}: {value}"
            );
        }
        // ArchiveGraph's internal clause reads an internal member; neither
        // reaches a client, and its public clause is marked partial there.
        assert_eq!(
            strings.iter().any(|s| s.contains("auditTicket")),
            audience == crucible_ir::Audience::Internal,
            "{file}"
        );
        let archive = document["paths"]["/orgs/{organizationId}/graphs/{graphId}/archive"]["post"]
            ["x-crucible-requires"]
            .clone();
        assert_eq!(
            archive.get("partial").cloned(),
            (audience == crucible_ir::Audience::External).then_some(serde_json::json!(true)),
            "{file}: {archive}"
        );
        let internal_tier = generated.contains("\"tier\": \"internal\"");
        assert_eq!(
            internal_tier,
            audience == crucible_ir::Audience::Internal,
            "{file}"
        );
        // Requirements reach the document as `x-crucible-requires` alone:
        // exactly the operations whose projected requirements say something
        // carry it (the pending one only for the internal audience, whose
        // projection keeps its reason), and no requirement leaks into
        // `x-crucible-traits`, which carries the model's own traits.
        let document: Value = serde_json::from_str(&generated).unwrap();
        let mut documented = std::collections::BTreeMap::new();
        for item in document["paths"].as_object().unwrap().values() {
            for operation in item.as_object().unwrap().values() {
                let traits = operation["x-crucible-traits"].as_object().unwrap();
                assert!(
                    traits.keys().all(|key| !key.starts_with("crucible.auth#")),
                    "{file}: {traits:?}"
                );
                documented.insert(
                    operation["operationId"].as_str().unwrap().to_owned(),
                    operation
                        .get("x-crucible-requires")
                        .map(|requires| requires["kind"].as_str().unwrap().to_owned()),
                );
            }
        }
        let expected: std::collections::BTreeMap<String, Option<String>> = projected
            .operations
            .iter()
            .map(|op| {
                use crucible_ir::ir::Requirements;
                let kind = match &op.requirements {
                    Requirements::Undeclared => None,
                    Requirements::None => Some("none"),
                    Requirements::Pending { .. } => Some("pending"),
                    Requirements::Internal => Some("internal"),
                    Requirements::List(_) | Requirements::Partial(_) => Some("list"),
                };
                (op.id.name().to_owned(), kind.map(str::to_owned))
            })
            .collect();
        assert_eq!(documented, expected, "{file}");
        let pending = expected.values().flatten().any(|kind| kind == "pending");
        assert_eq!(
            pending,
            audience == crucible_ir::Audience::Internal,
            "{file}"
        );
    }
}

/// The values of the model's internal permissions and of their aliases.
fn internal_values(service: &crucible_ir::ir::Service) -> Vec<String> {
    let values: Vec<String> = service
        .permissions
        .values()
        .flat_map(|resource| {
            resource.permissions.iter().filter_map(move |permission| {
                let primary = permission
                    .alias_of
                    .as_deref()
                    .and_then(|symbol| resource.permission(symbol))
                    .unwrap_or(permission);
                (primary.tier == crucible_ir::ir::Tier::Internal).then(|| permission.value.clone())
            })
        })
        .collect();
    assert!(
        !values.is_empty(),
        "the fixture declares internal permissions"
    );
    values
}

/// Every string in `value`, keys included.
fn strings_of(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Array(items) => items.iter().for_each(|v| strings_of(v, out)),
        serde_json::Value::Object(map) => {
            for (key, v) in map {
                out.push(key.clone());
                strings_of(v, out);
            }
        }
        _ => {}
    }
}

#[test]
fn http_members_are_projected_into_their_wire_locations() {
    let doc = document(&weather());
    assert_eq!(doc["openapi"], "3.1.1");
    assert!(doc.get("servers").is_none());
    let create = &doc["paths"]["/cities"]["post"];
    assert_eq!(create["operationId"], "CreateCity");
    assert_eq!(create["parameters"][0]["name"], "X-Expected-Revision");
    assert_eq!(
        create["parameters"][0]["schema"]["$ref"],
        "#/components/schemas/crucible.api.Long"
    );
    let body = &create["requestBody"]["content"]["application/json"]["schema"];
    assert_eq!(body["required"], json!(["name"]));
    assert_eq!(body["properties"]["name"]["maxLength"], 40);
    // Lowering replaces the entire target length trait with the member's.
    assert!(body["properties"]["name"].get("minLength").is_none());
    assert!(body["properties"].get("expectedRevision").is_none());
    assert_eq!(create["requestBody"]["required"], true);
    assert_eq!(create["x-crucible-idempotency-token"], "clientToken");
    assert_eq!(create["x-crucible-traits"]["example.weather#tier"], "write");
    let get = &doc["paths"]["/cities/{cityId}"]["get"];
    assert_eq!(get["parameters"][0]["in"], "path");
    assert_eq!(get["parameters"][0]["required"], true);
    assert!(get.get("requestBody").is_none()); // label-only input has no JSON body
    assert!(
        get["responses"]["200"]["headers"]
            .get("X-Request-Id")
            .is_some()
    );
    let list = &doc["paths"]["/cities"]["get"];
    let tags = list["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "tag")
        .unwrap();
    assert_eq!(tags["style"], "form");
    assert_eq!(tags["explode"], true);
    assert_eq!(list["x-crucible-pagination"]["items"], "cities");
    assert!(
        doc["paths"]["/ping"]["get"]["responses"]["200"]
            .get("content")
            .is_none()
    );
}

#[test]
fn binary_payloads_and_json_blobs_have_different_representations() {
    let doc = document(&weather());
    let payload = &doc["paths"]["/cities/{cityId}/photo"]["put"]["requestBody"];
    assert_eq!(payload["required"], true);
    assert_eq!(
        payload["content"]["image/jpeg"]["schema"]["format"],
        "binary"
    );
    assert!(
        payload["content"]["image/jpeg"]["schema"]
            .get("contentEncoding")
            .is_none()
    );
    let blob = &doc["components"]["schemas"]["example.weather.Thumbnail"];
    assert_eq!(blob["contentEncoding"], "base64");
    assert_eq!(blob["x-crucible-length"]["max"], 8);
    assert!(blob.get("maxLength").is_none());
}

#[test]
fn constraint_overrides_can_relax_the_target_without_intersection() {
    let doc = document(&edited(&[(
        "name: CityName { length ..40 }",
        "name: CityName { length 0..200 }",
    )]));
    let name = &doc["paths"]["/cities"]["post"]["requestBody"]["content"]["application/json"]["schema"]
        ["properties"]["name"];
    assert_eq!(name["minLength"], 0);
    assert_eq!(name["maxLength"], 200);
    assert!(name.get("$ref").is_none());
    assert_eq!(
        doc["components"]["schemas"]["example.weather.CityName"]["maxLength"],
        100
    );
}

#[test]
fn timestamp_formats_follow_document_and_http_binding_ir() {
    let svc = edited(&[
        (
            "        @header(\"X-Request-Id\")\n        requestId?: string\n",
            "        @header(\"X-Request-Id\")\n        requestId?: string\n        @header(\"Last-Modified\")\n        modified?: timestamp { format http-date }\n",
        ),
        (
            "        @query(\"tag\")\n        tags?: Tags\n",
            "        @query(\"tag\")\n        tags?: Tags\n        @query(\"since\")\n        since?: timestamp { format epoch-seconds }\n",
        ),
    ]);
    let doc = document(&svc);
    let city = &doc["components"]["schemas"]["example.weather.CitySummary"]["properties"];
    assert_eq!(city["updatedAt"]["anyOf"][0]["format"], "date-time");
    assert_eq!(city["createdAt"]["anyOf"][0]["type"], "number");
    assert_eq!(
        doc["paths"]["/cities/{cityId}"]["get"]["responses"]["200"]["headers"]["Last-Modified"]["schema"]
            ["x-crucible-timestamp-format"],
        "http-date"
    );
    let parameters = doc["paths"]["/cities"]["get"]["parameters"]
        .as_array()
        .unwrap();
    assert_eq!(
        parameters.iter().find(|p| p["name"] == "since").unwrap()["schema"]["type"],
        "number"
    );
}

#[test]
fn errors_sharing_a_status_keep_bodies_headers_and_discriminators() {
    let doc = document(&edited(&[(
        "error ConflictError {\n    status 409",
        "error ConflictError {\n    status 429",
    )]));
    let response = &doc["paths"]["/cities"]["post"]["responses"]["429"];
    assert_eq!(response["x-crucible-errors"].as_array().unwrap().len(), 2);
    let schemas = response["content"]["application/json"]["schema"]["anyOf"]
        .as_array()
        .unwrap();
    assert_eq!(schemas.len(), 2);
    assert!(
        schemas
            .iter()
            .any(|s| s["properties"]["__type"]["const"] == "ConflictError")
    );
    assert!(
        schemas
            .iter()
            .any(|s| s["properties"]["__type"]["const"] == "ThrottlingError")
    );
    assert!(
        schemas
            .iter()
            .all(|s| s["properties"].get("retryAfter").is_none())
    );
    assert!(response["headers"].get("Retry-After").is_some());
    assert_eq!(
        response["headers"]["X-Crucible-Error"]["schema"]["anyOf"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn examples_land_on_the_request_response_and_error_that_carry_them() {
    let doc = document(&weather());
    let create = &doc["paths"]["/cities"]["post"];
    // Body members only: the header-bound revision is a parameter example.
    let body = &create["requestBody"]["content"]["application/json"]["examples"];
    assert_eq!(body["Create a city"]["summary"], "Create a city");
    assert_eq!(
        body["Create a city"]["description"],
        "Creates Lisbon with coordinates."
    );
    assert!(
        body["Create a city"]["value"]
            .get("expectedRevision")
            .is_none()
    );
    assert_eq!(
        create["parameters"][0]["examples"]["Create a city"]["value"],
        7
    );
    // Timestamps follow each member's own format, not the model's spelling.
    let city = &create["responses"]["201"]["content"]["application/json"]["examples"]["Create a city"]
        ["value"]["city"];
    assert_eq!(city["createdAt"], 1767323045);
    assert_eq!(city["updatedAt"], "2026-01-02T03:04:05Z");
    // An error example appears only on its own error, with the wire discriminator
    // the schema requires.
    let conflict = &create["responses"]["409"]["content"]["application/json"]["examples"];
    assert_eq!(
        conflict["Create a duplicate city"]["value"],
        json!({"message": "Lisbon already exists", "__type": "ConflictError"})
    );
    assert!(
        doc["paths"]["/cities/{cityId}"]["get"]["responses"]["404"]["content"]["application/json"]
            ["examples"]
            .get("Create a duplicate city")
            .is_none()
    );
    let get = &doc["paths"]["/cities/{cityId}"]["get"];
    assert_eq!(
        get["responses"]["200"]["headers"]["X-Request-Id"]["examples"]["Read a city"]["value"],
        "req-1"
    );
    // The raw trait is not repeated beside the native examples.
    assert!(
        get["x-crucible-traits"]
            .get("crucible.api#examples")
            .is_none()
    );
}

#[test]
fn operations_without_examples_emit_no_example_keys() {
    let doc = document(&weather());
    let ping = serde_json::to_string(&doc["paths"]["/ping"]).unwrap();
    assert!(!ping.contains("examples"), "{ping}");
}

#[test]
fn repeated_titles_get_distinct_keys_in_model_order() {
    let first = "    example \"Read a city\" {\n        input { cityId: \"c-0a1b2c3d\", at: \"2026-01-02T03:04:05Z\" }\n        output { city: { id: \"c-0a1b2c3d\", name: \"Lisbon\" }, forecast: { clear: { highC: 21 } }, requestId: \"req-1\" }\n    }\n";
    let doc = document(&edited(&[(first, &format!("{first}{first}"))]));
    let keys: Vec<_> = doc["paths"]["/cities/{cityId}"]["get"]["parameters"][0]["examples"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        keys,
        ["Read a city", "Read a city (2)", "Read a missing city"]
    );
}

#[test]
fn examples_survive_responses_that_share_a_status() {
    let doc = document(&edited(&[(
        "error ConflictError {
    status 409",
        "error ConflictError {
    status 429",
    )]));
    let examples = &doc["paths"]["/cities"]["post"]["responses"]["429"]["content"]["application/json"]
        ["examples"];
    assert!(examples.get("Create a duplicate city").is_some());
}

#[test]
fn an_invalid_example_stops_the_model_compiling() {
    let rendered = try_edited(&[(
        "input { cityId: \"c-0a1b2c3d\", at:",
        "input { cityId: \"not-a-city\", at:",
    )])
    .unwrap_err();
    assert!(
        rendered.contains("input.cityId does not match the pattern"),
        "{rendered}"
    );
}

#[test]
fn auth_overrides_are_resolved_per_operation() {
    let doc = document(&edited(&[(
        "readonly operation Ping",
        "public readonly operation Ping",
    )]));
    assert_eq!(
        doc["components"]["securitySchemes"]["crucible.api.httpBearerAuth"]["scheme"],
        "bearer"
    );
    assert_eq!(doc["paths"]["/ping"]["get"]["security"], json!([]));
    assert_eq!(
        doc["paths"]["/status"]["get"]["security"],
        json!([{"crucible.api.httpBearerAuth": []}])
    );
}

#[test]
fn sparse_collections_unique_lists_and_large_numbers_are_preserved() {
    let service = load("extended-types");
    let doc = document(&service);
    let shapes = &doc["components"]["schemas"];
    assert_eq!(
        shapes["example.types.Mode"]["enum"],
        json!([-1, 2147483647])
    );
    assert_eq!(shapes["example.types.Names"]["uniqueItems"], true);
    assert_eq!(
        shapes["example.types.SparseNames"]["items"]["anyOf"][1]["type"],
        "null"
    );
    assert_eq!(
        shapes["example.types.SparseMap"]["additionalProperties"]["anyOf"][1]["type"],
        "null"
    );
    assert_eq!(shapes["crucible.api.BigInteger"]["type"], "integer");
    assert_eq!(shapes["crucible.api.BigDecimal"]["type"], "number");
}

#[test]
fn recursive_models_keep_references_and_union_arms_are_exclusive() {
    let doc = document(&edited(&[(
        "structure CitySummary {\n",
        "structure CitySummary {\n    parent?: CitySummary\n",
    )]));
    assert_eq!(
        doc["components"]["schemas"]["example.weather.CitySummary"]["properties"]["parent"]["anyOf"]
            [0]["$ref"],
        "#/components/schemas/example.weather.CitySummary"
    );
    let union = &doc["components"]["schemas"]["example.weather.Location"];
    assert_eq!(union["oneOf"].as_array().unwrap().len(), 2);
    for arm in union["oneOf"].as_array().unwrap() {
        assert_eq!(arm["additionalProperties"], false);
        assert_eq!(arm["required"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn constrained_recursive_collections_do_not_expand_forever_or_intersect_overrides() {
    let doc = document(&edited(&[(
        "structure CitySummary {\n",
        "list RecursiveNodes: RecursiveNode { length ..4 }\n\nstructure RecursiveNode {\n    children?: RecursiveNodes { length ..2 }\n}\n\nstructure CitySummary {\n    nested?: RecursiveNode\n",
    )]));
    let shapes = &doc["components"]["schemas"];
    assert_eq!(shapes["example.weather.RecursiveNodes"]["maxItems"], 4);
    let children = &shapes["example.weather.RecursiveNode"]["properties"]["children"]["anyOf"][0];
    assert_eq!(children["maxItems"], 2);
    assert_eq!(
        children["items"]["$ref"],
        "#/components/schemas/example.weather.RecursiveNode"
    );
    assert!(children.get("$ref").is_none());
}

#[test]
fn literal_queries_are_parameters_but_colliding_routes_are_diagnostics() {
    let svc = edited(&[("http GET \"/ping\"", "http GET \"/ping?mode=ping&flag\"")]);
    let doc = document(&svc);
    assert_eq!(
        doc["paths"]["/ping"]["get"]["parameters"][0]["schema"]["const"],
        "ping"
    );
    assert_eq!(
        doc["paths"]["/ping"]["get"]["parameters"][1]["allowEmptyValue"],
        true
    );
    let svc = edited(&[
        ("http GET \"/ping\"", "http GET \"/ping?mode=ping&flag\""),
        ("http GET \"/status\"", "http GET \"/ping?mode=status\""),
    ]);
    let errors = generate(&svc, &Options::default()).unwrap_err().to_string();
    assert!(errors.contains("example.weather#Status"));
    assert!(errors.contains("multiple operations on GET /ping"));
}

#[test]
fn incompatible_path_templates_and_methods_are_collected() {
    let mut svc = weather();
    let ping = svc
        .operations
        .iter_mut()
        .find(|op| op.name() == "Ping")
        .unwrap();
    ping.uri.segments = vec![
        crucible_ir::ir::Segment::Literal("cities".into()),
        crucible_ir::ir::Segment::Label("id".into()),
    ];
    ping.method = "CONNECT".into();
    let errors = generate(&svc, &Options::default()).unwrap_err().to_string();
    assert!(errors.contains("cannot distinguish path templates"));
    assert!(errors.contains("CONNECT"));
}

#[test]
fn text_document_and_error_payloads_keep_their_media_types() {
    let svc = load("error-payloads");
    let doc = document(&svc);
    assert!(
        doc["paths"]["/text"]["post"]["requestBody"]["content"]
            .get("text/plain")
            .is_some()
    );
    assert!(
        doc["paths"]["/document"]["post"]["requestBody"]["content"]
            .get("application/json")
            .is_some()
    );
    let error = &doc["paths"]["/text"]["post"]["responses"]["422"];
    assert!(error["content"].get("text/plain").is_some());
    assert!(error["headers"].get("X-Crucible-Error").is_some());
    let headers = &doc["paths"]["/text"]["post"]["parameters"];
    assert_eq!(headers[0]["schema"]["items"]["contentEncoding"], "base64");
}

#[test]
fn output_does_not_depend_on_model_order_or_operation_vector_order() {
    let svc = weather();
    let first = generate(&svc, &Options::default()).unwrap();
    let mut reversed = svc.clone();
    reversed.operations.reverse();
    assert_eq!(first, generate(&reversed, &Options::default()).unwrap());
    let mut package = crucible_lang::load_package(&package_dir("weather")).unwrap();
    package.files.reverse();
    let reordered = crucible_lang::compile(&package, None).service.unwrap();
    assert_eq!(first, generate(&reordered, &Options::default()).unwrap());
    assert!(first.ends_with('\n'));
}

#[test]
fn ignored_and_duplicate_headers_are_diagnostics_instead_of_lost_contracts() {
    let mut svc = weather();
    let create = svc
        .operations
        .iter_mut()
        .find(|op| op.name() == "CreateCity")
        .unwrap();
    create.request.headers[0].name = "Authorization".into();
    let get = svc
        .operations
        .iter_mut()
        .find(|op| op.name() == "GetCity")
        .unwrap();
    get.response.headers[0].name = "Content-Type".into();
    let mut duplicate = get.response.headers[0].clone();
    duplicate.name = "content-type".into();
    get.response.headers.push(duplicate);
    let errors = generate(&svc, &Options::default()).unwrap_err().to_string();
    assert!(errors.contains("modeled authorization request header"));
    assert!(errors.contains("modeled content-type response header"));
    assert!(errors.contains("duplicate OpenAPI response header content-type"));
    assert!(errors.contains("example.weather#CreateCity"));
    assert!(errors.contains("example.weather#GetCity"));
}

#[test]
fn doc_links_are_their_text_in_descriptions() {
    let service =
        crucible_ir::project(&load("doc-links"), crucible_ir::Audience::External).unwrap();
    let document = document(&service);
    let get = &document["paths"]["/widgets/{widgetId}"]["get"];
    assert_eq!(
        get["description"],
        "Reads the Widget that CreateWidget made, by GetWidget.widgetId,\nand returns it as GetWidget.widget. Fails with WidgetError."
    );
    let info = document["info"]["description"].as_str().unwrap();
    assert!(
        info.contains("read it back with\nthe read call; a Widget has a Widget.name"),
        "{info}"
    );
    assert!(info.contains("Internal: Reveal and Secret."), "{info}");
    assert!(
        info.contains("`[NotALink]` in code, and [an ordinary link](https://example.com)."),
        "{info}"
    );
    let schemas = document["components"]["schemas"].as_object().unwrap();
    assert!(
        schemas
            .values()
            .any(|schema| schema["description"] == "A colour. Color.RED is the first."),
        "{schemas:#?}"
    );
}
