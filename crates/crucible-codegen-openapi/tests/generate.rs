use std::fs;
use std::path::Path;

use crucible_codegen_openapi::{Options, capabilities, generate};
use crucible_smithy::{Capabilities, Feature, Model, Protocol};
use serde_json::{Value, json};

const WEATHER: &str = include_str!("../../crucible-smithy/tests/fixtures/weather.json");

fn service(edit: impl FnOnce(&mut Value)) -> crucible_smithy::ir::Service {
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    edit(&mut model);
    Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &capabilities())
        .unwrap()
}

fn document(service: &crucible_smithy::ir::Service) -> Value {
    serde_json::from_str(&generate(service, &Options::default()).unwrap()).unwrap()
}

#[test]
fn weather_matches_reviewed_golden() {
    let service = service(|_| {});
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

#[test]
fn http_members_are_projected_into_their_wire_locations() {
    let doc = document(&service(|_| {}));
    assert_eq!(doc["openapi"], "3.1.1");
    assert!(doc.get("servers").is_none());
    let create = &doc["paths"]["/cities"]["post"];
    assert_eq!(create["operationId"], "CreateCity");
    assert_eq!(create["parameters"][0]["name"], "X-Expected-Revision");
    assert_eq!(
        create["parameters"][0]["schema"]["$ref"],
        "#/components/schemas/smithy.api.Long"
    );
    let body = &create["requestBody"]["content"]["application/json"]["schema"];
    assert_eq!(body["required"], json!(["name"]));
    assert_eq!(body["properties"]["name"]["maxLength"], 40);
    // Lowering replaces the entire target length trait with the member's.
    assert!(body["properties"]["name"].get("minLength").is_none());
    assert!(body["properties"].get("expectedRevision").is_none());
    assert_eq!(create["requestBody"]["required"], true);
    assert_eq!(create["x-smithy-idempotency-token"], "clientToken");
    assert_eq!(create["x-smithy-traits"]["example.weather#tier"], "write");
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
    assert_eq!(list["x-smithy-pagination"]["items"], "cities");
    assert!(
        doc["paths"]["/ping"]["get"]["responses"]["200"]
            .get("content")
            .is_none()
    );
}

#[test]
fn binary_payloads_and_json_blobs_have_different_representations() {
    let doc = document(&service(|_| {}));
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
    assert_eq!(blob["x-smithy-length"]["max"], 8);
    assert!(blob.get("maxLength").is_none());
}

#[test]
fn constraint_overrides_can_relax_the_target_without_intersection() {
    let doc = document(&service(|model| {
        model["shapes"]["example.weather#CreateCityInput"]["members"]["name"]["traits"]["smithy.api#length"] =
            json!({"min": 0, "max": 200});
    }));
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
    let svc = service(|model| {
        model["shapes"]["example.weather#GetCityOutput"]["members"]["modified"] = json!({"target": "smithy.api#Timestamp", "traits": {"smithy.api#httpHeader": "Last-Modified", "smithy.api#timestampFormat": "http-date"}});
        model["shapes"]["example.weather#ListCitiesInput"]["members"]["since"] = json!({"target": "smithy.api#Timestamp", "traits": {"smithy.api#httpQuery": "since", "smithy.api#timestampFormat": "epoch-seconds"}});
    });
    let doc = document(&svc);
    let city = &doc["components"]["schemas"]["example.weather.CitySummary"]["properties"];
    assert_eq!(city["updatedAt"]["anyOf"][0]["format"], "date-time");
    assert_eq!(city["createdAt"]["anyOf"][0]["type"], "number");
    assert_eq!(
        doc["paths"]["/cities/{cityId}"]["get"]["responses"]["200"]["headers"]["Last-Modified"]["schema"]
            ["x-smithy-timestamp-format"],
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
    let doc = document(&service(|model| {
        model["shapes"]["example.weather#ConflictError"]["traits"]["smithy.api#httpError"] =
            json!(429);
    }));
    let response = &doc["paths"]["/cities"]["post"]["responses"]["429"];
    assert_eq!(response["x-smithy-errors"].as_array().unwrap().len(), 2);
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
fn errors_on_the_success_status_do_not_overwrite_success() {
    let doc = document(&service(|model| {
        model["shapes"]["example.weather#ConflictError"]["traits"]["smithy.api#httpError"] =
            json!(201);
    }));
    let variants = doc["paths"]["/cities"]["post"]["responses"]["201"]["content"]["application/json"]["schema"]["anyOf"].as_array().unwrap();
    assert_eq!(variants.len(), 2);
    assert!(
        variants
            .iter()
            .any(|s| s["properties"].get("city").is_some())
    );
    assert!(
        variants
            .iter()
            .any(|s| s["properties"]["__type"]["const"] == "ConflictError")
    );
}

#[test]
fn auth_overrides_and_optional_auth_are_resolved_per_operation() {
    let doc = document(&service(|model| {
        model["shapes"]["example.weather#Ping"]["traits"]["smithy.api#auth"] = json!([]);
        model["shapes"]["example.weather#Status"]["traits"]["smithy.api#optionalAuth"] = json!({});
    }));
    assert_eq!(
        doc["components"]["securitySchemes"]["smithy.api.httpBearerAuth"]["scheme"],
        "bearer"
    );
    assert_eq!(doc["paths"]["/ping"]["get"]["security"], json!([]));
    assert_eq!(
        doc["paths"]["/status"]["get"]["security"],
        json!([{"smithy.api.httpBearerAuth": []}, {}])
    );
}

#[test]
fn additional_auth_schemes_have_native_openapi_representations() {
    let doc = document(&service(|model| {
        let traits = &mut model["shapes"]["example.weather#Weather"]["traits"];
        traits["smithy.api#httpBasicAuth"] = json!({});
        traits["smithy.api#httpDigestAuth"] = json!({});
        traits["smithy.api#httpApiKeyAuth"] =
            json!({"name": "x-api-key", "in": "header", "scheme": "Token"});
    }));
    let schemes = &doc["components"]["securitySchemes"];
    assert_eq!(schemes["smithy.api.httpBasicAuth"]["scheme"], "basic");
    assert_eq!(schemes["smithy.api.httpDigestAuth"]["scheme"], "digest");
    assert_eq!(schemes["smithy.api.httpApiKeyAuth"]["type"], "apiKey");
    assert_eq!(schemes["smithy.api.httpApiKeyAuth"]["in"], "header");
    assert_eq!(
        schemes["smithy.api.httpApiKeyAuth"]["x-smithy-api-key-scheme"],
        "Token"
    );
}

#[test]
fn json_names_sparse_collections_sets_and_large_numbers_are_preserved() {
    let source = include_str!("../../crucible-smithy/tests/fixtures/extended-types.json");
    let service = Model::from_json(source)
        .unwrap()
        .lower_with_capabilities(None, &capabilities())
        .unwrap();
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
    assert_eq!(shapes["smithy.api.BigInteger"]["type"], "integer");
    assert_eq!(shapes["smithy.api.BigDecimal"]["type"], "number");
    let renamed = document(&self::service(|m| {
        m["shapes"]["example.weather#CreateCityInput"]["members"]["name"]["traits"]["smithy.api#jsonName"] =
            json!("city_name");
    }));
    assert_eq!(
        renamed["paths"]["/cities"]["post"]["requestBody"]["content"]["application/json"]["schema"]
            ["required"],
        json!(["city_name"])
    );
}

#[test]
fn recursive_models_keep_references_and_union_arms_are_exclusive() {
    let doc = document(&service(|model| {
        model["shapes"]["example.weather#CitySummary"]["members"]["parent"] =
            json!({"target": "example.weather#CitySummary"});
    }));
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
    let doc = document(&service(|model| {
        model["shapes"]["example.weather#RecursiveNodes"] = json!({
            "type": "list", "traits": {"smithy.api#length": {"max": 4}},
            "member": {"target": "example.weather#RecursiveNode"}
        });
        model["shapes"]["example.weather#RecursiveNode"] = json!({"type": "structure", "members": {
            "children": {"target": "example.weather#RecursiveNodes", "traits": {"smithy.api#length": {"max": 2}}}
        }});
        model["shapes"]["example.weather#CitySummary"]["members"]["nested"] =
            json!({"target": "example.weather#RecursiveNode"});
    }));
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
fn media_type_enums_in_headers_describe_the_encoded_value() {
    let doc = document(&service(|model| {
        model["shapes"]["example.weather#Sky"]["traits"] =
            json!({"smithy.api#mediaType": "text/plain"});
        model["shapes"]["example.weather#GetCityOutput"]["members"]["sky"] =
            json!({"target": "example.weather#Sky", "traits": {"smithy.api#httpHeader": "X-Sky"}});
    }));
    let sky =
        &doc["paths"]["/cities/{cityId}"]["get"]["responses"]["200"]["headers"]["X-Sky"]["schema"];
    assert_eq!(sky["contentEncoding"], "base64");
    assert_eq!(
        sky["x-smithy-decoded-schema"]["$ref"],
        "#/components/schemas/example.weather.Sky"
    );
}

#[test]
fn literal_queries_are_parameters_but_colliding_routes_are_diagnostics() {
    let svc = service(|model| {
        model["shapes"]["example.weather#Ping"]["traits"]["smithy.api#http"]["uri"] =
            json!("/ping?mode=ping&flag");
    });
    let doc = document(&svc);
    assert_eq!(
        doc["paths"]["/ping"]["get"]["parameters"][0]["schema"]["const"],
        "ping"
    );
    assert_eq!(
        doc["paths"]["/ping"]["get"]["parameters"][1]["allowEmptyValue"],
        true
    );
    let svc = service(|model| {
        model["shapes"]["example.weather#Status"]["traits"]["smithy.api#http"]["uri"] =
            json!("/ping?mode=status");
    });
    let errors = generate(&svc, &Options::default()).unwrap_err().to_string();
    assert!(errors.contains("example.weather#Status"));
    assert!(errors.contains("multiple operations on GET /ping"));
}

#[test]
fn incompatible_path_templates_and_methods_are_collected() {
    let mut svc = service(|_| {});
    let ping = svc
        .operations
        .iter_mut()
        .find(|op| op.name() == "Ping")
        .unwrap();
    ping.uri.segments = vec![
        crucible_smithy::ir::Segment::Literal("cities".into()),
        crucible_smithy::ir::Segment::Label("id".into()),
    ];
    ping.method = "CONNECT".into();
    let errors = generate(&svc, &Options::default()).unwrap_err().to_string();
    assert!(errors.contains("cannot distinguish path templates"));
    assert!(errors.contains("CONNECT"));
}

#[test]
fn aws_rest_json_uses_its_timestamp_and_error_rules() {
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    let traits = model["shapes"]["example.weather#Weather"]["traits"]
        .as_object_mut()
        .unwrap();
    traits.remove("crucible.protocols#json");
    traits.insert("aws.protocols#restJson1".into(), json!({}));
    model["shapes"]["example.weather#GetCityOutput"]["members"]["modified"] = json!({"target": "smithy.api#Timestamp", "traits": {"smithy.api#httpHeader": "Last-Modified"}});
    let svc = Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &capabilities())
        .unwrap();
    assert_eq!(svc.protocol, Protocol::AwsRestJson1);
    let doc = document(&svc);
    assert_eq!(
        doc["components"]["schemas"]["smithy.api.Timestamp"]["type"],
        "number"
    );
    assert_eq!(
        doc["paths"]["/cities/{cityId}"]["get"]["responses"]["200"]["headers"]["Last-Modified"]["schema"]
            ["x-smithy-timestamp-format"],
        "http-date"
    );
    assert!(
        doc["paths"]["/cities"]["post"]["responses"]["409"]["headers"]
            .get("X-Amzn-Errortype")
            .is_some()
    );
}

#[test]
fn externally_lowered_unsupported_features_are_rechecked() {
    let model = include_str!("../../crucible-smithy/tests/fixtures/error-payloads.json");
    let svc = Model::from_json(model)
        .unwrap()
        .lower_with_capabilities(None, &Capabilities::new(Feature::ALL.iter().copied()))
        .unwrap();
    assert!(
        generate(&svc, &Options::default())
            .unwrap_err()
            .to_string()
            .contains("prefix-headers")
    );
    let mut svc = service(|_| {});
    svc.protocol = Protocol::SmithyRpcV2Cbor;
    assert!(
        generate(&svc, &Options::default())
            .unwrap_err()
            .to_string()
            .contains("REST JSON only")
    );
}

#[test]
fn text_document_and_error_payloads_keep_their_media_types() {
    let source = include_str!("../../crucible-smithy/tests/fixtures/error-payloads.json");
    let mut model: Value = serde_json::from_str(source).unwrap();
    for shape in model["shapes"].as_object_mut().unwrap().values_mut() {
        if let Some(members) = shape.get_mut("members").and_then(Value::as_object_mut) {
            members.retain(|_, member| {
                member["traits"]
                    .get("smithy.api#httpPrefixHeaders")
                    .is_none()
            });
        }
    }
    let svc = Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &capabilities())
        .unwrap();
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
    let svc = service(|_| {});
    let first = generate(&svc, &Options::default()).unwrap();
    let mut reversed = svc.clone();
    reversed.operations.reverse();
    assert_eq!(first, generate(&reversed, &Options::default()).unwrap());
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    let shapes = model["shapes"].as_object_mut().unwrap();
    let reordered: serde_json::Map<_, _> = shapes
        .iter()
        .rev()
        .map(|(id, shape)| (id.clone(), shape.clone()))
        .collect();
    *shapes = reordered;
    let reordered = Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &capabilities())
        .unwrap();
    assert_eq!(first, generate(&reordered, &Options::default()).unwrap());
    assert!(first.ends_with('\n'));
}

#[test]
fn ignored_and_duplicate_headers_are_diagnostics_instead_of_lost_contracts() {
    let mut svc = service(|_| {});
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
