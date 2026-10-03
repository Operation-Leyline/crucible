use crucible_core::ir::{Binding, Fault, Paginated, Shape, SimpleKind, TimestampFormat};
use crucible_core::{Error, Model, ShapeId};
use serde_json::{Value, json};

const WEATHER: &str = include_str!("fixtures/weather.json");

fn id(text: &str) -> ShapeId {
    text.parse().unwrap()
}

fn weather() -> crucible_core::ir::Service {
    Model::from_json(WEATHER).unwrap().lower(None).unwrap()
}

/// The fixture with one edit applied, for a model that should be refused.
fn refused(edit: impl FnOnce(&mut Value)) -> Vec<String> {
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    edit(&mut model);
    match Model::from_json(&model.to_string()).unwrap().lower(None) {
        Err(Error::Unsupported(diagnostics)) => {
            diagnostics.iter().map(ToString::to_string).collect()
        }
        other => panic!("expected diagnostics, got {other:?}"),
    }
}

fn shape<'a>(model: &'a mut Value, name: &str) -> &'a mut Value {
    &mut model["shapes"][format!("example.weather#{name}")]
}

#[test]
fn finds_operations_through_resources_sorted_by_name() {
    let service = weather();
    let names: Vec<&str> = service.operations.iter().map(|op| op.name()).collect();
    assert_eq!(names, ["CreateCity", "GetCity", "ListCities", "Ping"]);
    assert_eq!(service.auth, [id("smithy.api#httpBearerAuth")]);
    assert_eq!(service.title.as_deref(), Some("Weather"));
}

#[test]
fn binds_labels_queries_headers_and_body() {
    let service = weather();
    let get = &service.operations[1];
    assert_eq!(get.uri.to_string(), "/cities/{cityId}");
    assert_eq!(get.request.labels, ["cityId"]);
    assert_eq!(
        get.request.query,
        [Binding {
            member: "at".into(),
            name: "at".into()
        }]
    );
    assert_eq!(
        get.response.headers,
        [Binding {
            member: "requestId".into(),
            name: "X-Request-Id".into()
        }]
    );
    assert_eq!(get.response.body, ["city", "forecast"]);

    let create = &service.operations[0];
    assert_eq!((create.method.as_str(), create.status), ("POST", 201));
    assert_eq!(create.request.body, ["name", "location", "clientToken"]);
    assert_eq!(create.request.headers[0].name, "X-Expected-Revision");
    assert_eq!(create.idempotency_token.as_deref(), Some("clientToken"));
    assert_eq!(create.traits[&id("example.weather#tier")], json!("write"));

    let ping = &service.operations[3];
    assert_eq!((ping.input.as_ref(), ping.output.as_ref()), (None, None));
}

#[test]
fn operation_errors_come_before_service_errors() {
    let service = weather();
    assert_eq!(
        service.operations[0].errors,
        [
            id("example.weather#ConflictError"),
            id("example.weather#ThrottlingError")
        ]
    );
    let Some(Shape::Structure(throttling)) = service.shape(&id("example.weather#ThrottlingError"))
    else {
        panic!("throttling error not lowered");
    };
    let error = throttling.error.as_ref().unwrap();
    assert_eq!((error.fault, error.status), (Fault::Client, 429));
    assert!(error.retryable.unwrap().throttling);
    assert_eq!(error.headers[0].name, "Retry-After");
}

#[test]
fn pagination_merges_service_defaults() {
    let service = weather();
    assert_eq!(
        service.operations[2].paginated,
        Some(Paginated {
            input_token: "nextToken".into(),
            output_token: "nextToken".into(),
            items: Some("cities".into()),
            page_size: Some("maxResults".into()),
        })
    );
}

#[test]
fn member_constraints_override_their_targets() {
    let service = weather();
    let Some(Shape::Structure(input)) = service.shape(&id("example.weather#CreateCityInput"))
    else {
        panic!();
    };
    let length = input
        .member("name")
        .unwrap()
        .constraints
        .length
        .clone()
        .unwrap();
    assert_eq!((length.min, length.max), (None, Some(40)));
}

#[test]
fn nullability_follows_smithy_two() {
    let service = weather();
    let Some(Shape::Structure(summary)) = service.shape(&id("example.weather#CitySummary")) else {
        panic!();
    };
    let [id_member, _, population, updated, _] = summary.members.as_slice() else {
        panic!()
    };
    assert!(!id_member.optional_on_server() && !id_member.optional_on_client());
    assert!(
        !population.optional_on_server(),
        "a default makes it non-null"
    );
    assert!(updated.optional_on_server());

    let Some(Shape::Structure(input)) = service.shape(&id("example.weather#CreateCityInput"))
    else {
        panic!();
    };
    let name = input.member("name").unwrap();
    assert!(
        !name.optional_on_server() && name.optional_on_client(),
        "@input members are clientOptional"
    );
}

#[test]
fn timestamps_take_member_then_target_then_date_time() {
    let service = weather();
    let Some(Shape::Structure(summary)) = service.shape(&id("example.weather#CitySummary")) else {
        panic!();
    };
    let format = |name: &str| service.timestamp_format(summary.member(name).unwrap());
    assert_eq!(format("updatedAt"), TimestampFormat::DateTime);
    assert_eq!(format("createdAt"), TimestampFormat::EpochSeconds);
}

#[test]
fn lowers_unions_enums_maps_and_prelude_targets() {
    let service = weather();
    assert!(matches!(
        service.shape(&id("smithy.api#Unit")),
        Some(Shape::Unit)
    ));
    assert!(matches!(
        service.shape(&id("example.weather#Location")),
        Some(Shape::Union(_))
    ));
    let Some(Shape::Enum(sky)) = service.shape(&id("example.weather#Sky")) else {
        panic!()
    };
    assert_eq!(sky.values[0].name, "CLEAR");
    assert_eq!(sky.values[0].value, "clear");
    let Some(Shape::Simple(document)) = service.shape(&id("smithy.api#Document")) else {
        panic!()
    };
    assert_eq!(document.kind, SimpleKind::Document);
}

#[test]
fn refuses_unflattened_mixins() {
    let messages = refused(|model| {
        shape(model, "ConflictError")["mixins"] =
            json!([{ "target": "example.weather#Coordinates" }]);
    });
    assert!(
        messages[0].contains("flattenAndRemoveMixins"),
        "{messages:?}"
    );
}

#[test]
fn refuses_blobs_and_streaming() {
    let messages = refused(|model| {
        shape(model, "Blobby")
            .clone_from(&json!({ "type": "blob", "traits": { "smithy.api#streaming": {} } }));
        shape(model, "Coordinates")["members"]["raw"] =
            json!({ "target": "example.weather#Blobby" });
    });
    assert!(
        messages.iter().any(|m| m.contains("@streaming")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("blob shapes")),
        "{messages:?}"
    );
}

#[test]
fn refuses_labels_without_members_and_members_without_labels() {
    let messages = refused(|model| {
        shape(model, "GetCity")["traits"]["smithy.api#http"]["uri"] = json!("/cities/{city}");
    });
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages[0].contains("{city}"));
    assert!(messages[1].contains("does not appear"));
}

#[test]
fn refuses_greedy_labels_and_structured_headers() {
    let messages = refused(|model| {
        shape(model, "GetCity")["traits"]["smithy.api#http"]["uri"] = json!("/cities/{cityId+}");
        shape(model, "CreateCityInput")["members"]["expectedRevision"]["target"] =
            json!("example.weather#Coordinates");
    });
    assert!(
        messages.iter().any(|m| m.contains("greedy")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("@httpHeader cannot carry")),
        "{messages:?}"
    );
}

#[test]
fn requires_its_own_protocol_and_bearer_auth() {
    let messages = refused(|model| {
        let traits = &mut shape(model, "Weather")["traits"];
        traits["smithy.api#httpBasicAuth"] = json!({});
        traits["aws.protocols#restJson1"] = json!({});
        traits
            .as_object_mut()
            .unwrap()
            .remove("crucible.protocols#json");
    });
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(
        messages[0].contains("crucible.protocols#json"),
        "{messages:?}"
    );
    assert!(messages[1].contains("httpBasicAuth"), "{messages:?}");
}

#[test]
fn tolerates_another_protocol_beside_its_own() {
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    model["shapes"]["example.weather#Weather"]["traits"]["aws.protocols#restJson1"] = json!({});
    assert!(
        Model::from_json(&model.to_string())
            .unwrap()
            .lower(None)
            .is_ok()
    );
}

#[test]
fn chooses_among_services() {
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    model["shapes"]["example.weather#Other"] = json!({ "type": "service", "version": "1" });
    let model = Model::from_json(&model.to_string()).unwrap();
    assert!(matches!(model.lower(None), Err(Error::AmbiguousService(_))));
    assert!(model.lower(Some(&id("example.weather#Weather"))).is_ok());
    assert!(matches!(
        model.lower(Some(&id("a#B"))),
        Err(Error::UnknownService(_))
    ));
}
