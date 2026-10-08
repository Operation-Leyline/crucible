use crucible_smithy::ir::{Binding, Fault, Paginated, Payload, Shape, SimpleKind, TimestampFormat};
use crucible_smithy::{Capabilities, Error, Feature, Model, ShapeId};
use serde_json::{Value, json};

const WEATHER: &str = include_str!("fixtures/weather.json");

#[test]
fn standard_service_exceptions_lower_without_operations() {
    let source = include_str!("fixtures/standard-service-errors.json");
    let service = Model::from_json(source).unwrap().lower(None).unwrap();
    assert!(service.operations.is_empty());
    for (name, status, fault, retryable) in [
        ("UnknownOperationException", 404, Fault::Client, false),
        ("ValidationException", 400, Fault::Client, false),
        ("InternalServerException", 500, Fault::Server, true),
    ] {
        let Shape::Structure(shape) = service
            .shape(&ShapeId::new("crucible.errors", name))
            .unwrap()
        else {
            panic!("error structure")
        };
        let error = shape.error.as_ref().unwrap();
        assert_eq!(error.status, status);
        assert_eq!(error.fault, fault);
        assert_eq!(error.retryable.is_some(), retryable);
    }
}

#[test]
fn service_errors_remain_reachable_without_operations() {
    let source = include_str!("fixtures/empty-service-errors.json");
    let service = Model::from_json(source).unwrap().lower(None).unwrap();
    assert!(service.operations.is_empty());
    for name in ["ValidationError", "InternalError"] {
        let Shape::Structure(error) = service.shape(&ShapeId::new("example.empty", name)).unwrap()
        else {
            panic!("error structure")
        };
        assert!(error.error.is_some());
        assert!(error.member("message").is_some());
    }
    assert!(
        service
            .shape(&ShapeId::new("smithy.api", "String"))
            .is_some()
    );

    let mut model: Value = serde_json::from_str(source).unwrap();
    model["shapes"]["example.empty#ValidationError"]["traits"] = json!({});
    let diagnostics = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap_err()
        .to_string();
    assert!(diagnostics.contains("listed as an error but has no @error trait"));
}

#[test]
fn tiny_timestamp_defaults_lower_even_when_clients_ignore_them() {
    for in_input in [false, true] {
        let mut model: Value =
            serde_json::from_str(include_str!("fixtures/timestamp-defaults.json")).unwrap();
        let shape_id = if in_input {
            model["shapes"]["example.timestamps#Read"]["input"] =
                json!({"target": "example.timestamps#Input"});
            model["shapes"]["example.timestamps#Input"] =
                json!({"type": "structure", "traits": {"smithy.api#input": {}}, "members": {}});
            "example.timestamps#Input"
        } else {
            "example.timestamps#Output"
        };
        model["shapes"][shape_id]["members"]["tiny"] = json!({
            "target": "smithy.api#Timestamp",
            "traits": {"smithy.api#default": -1e-100}
        });
        if !in_input {
            model["shapes"][shape_id]["members"]["tiny"]["traits"]["smithy.api#clientOptional"] =
                json!({});
        }
        let service = Model::from_json(&model.to_string())
            .unwrap()
            .lower(None)
            .unwrap();
        let Shape::Structure(structure) = service.shape(&id(shape_id)).unwrap() else {
            panic!("structure")
        };
        let member = structure.member("tiny").unwrap();
        assert_eq!(member.wire_default, Some(json!("1970-01-01T00:00:00Z")));
        assert_eq!(member.client_wire_default(), None);
        assert_eq!(member.default, Some(json!(-1e-100)));
    }
}

#[test]
fn invalid_timestamp_defaults_are_diagnosed_at_the_member() {
    let messages = refused(|model| {
        shape(model, "StatusInput")["members"]["time"] =
            json!({"target":"smithy.api#Timestamp","traits":{"smithy.api#default":1e300}});
    });
    assert!(
        messages
            .iter()
            .any(|m| m.contains("StatusInput$time") && m.contains("timestamp default")),
        "{messages:?}"
    );
}

#[test]
fn client_auth_and_defaults_resolve_before_generation() {
    let mut model: Value =
        serde_json::from_str(include_str!("fixtures/review-hardening.json")).unwrap();
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    assert!(
        service
            .operations
            .iter()
            .find(|o| o.name() == "PublicPing")
            .unwrap()
            .auth
            .is_empty()
    );
    assert_eq!(
        service
            .operations
            .iter()
            .find(|o| o.name() == "ReadValues")
            .unwrap()
            .auth,
        [id("smithy.api#httpBearerAuth")]
    );
    let Shape::Structure(values) = service.shape(&id("example.hardening#Values")).unwrap() else {
        panic!("structure")
    };
    let member = values.member("optionalDefault").unwrap();
    assert_eq!(member.default.as_ref(), Some(&json!(7)));
    assert_eq!(member.client_default(), None);
    model["shapes"]["example.hardening#Hardening"]["traits"]["smithy.api#auth"] = json!([]);
    model["shapes"]["example.hardening#ReadValues"]["traits"]["smithy.api#auth"] =
        json!(["smithy.api#httpBearerAuth"]);
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    assert!(
        service
            .operations
            .iter()
            .find(|o| o.name() == "EmptyWrite")
            .unwrap()
            .auth
            .is_empty()
    );
    assert_eq!(
        service
            .operations
            .iter()
            .find(|o| o.name() == "ReadValues")
            .unwrap()
            .auth,
        [id("smithy.api#httpBearerAuth")]
    );
}

#[test]
fn media_type_header_strings_are_diagnosed_including_list_items() {
    for list in [false, true] {
        let messages = refused(|model| {
            model["shapes"]["example.weather#MediaString"] =
                json!({"type":"string","traits":{"smithy.api#mediaType":"application/json"}});
            model["shapes"]["example.weather#MediaList"] =
                json!({"type":"list","member":{"target":"example.weather#MediaString"}});
            shape(model, "StatusInput")["members"]["media"] = json!({"target":if list {"example.weather#MediaList"} else {"example.weather#MediaString"},"traits":{"smithy.api#httpHeader":"x-media"}});
        });
        assert!(
            messages
                .iter()
                .any(|m| m.contains("StatusInput$media") && m.contains("media-type-headers")),
            "{messages:?}"
        );
    }
}

#[test]
fn media_type_strings_are_still_supported_in_json_bodies() {
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    model["shapes"]["example.weather#MediaString"] =
        json!({"type":"string","traits":{"smithy.api#mediaType":"application/json"}});
    shape(&mut model, "StatusInput")["members"]["media"] =
        json!({"target":"example.weather#MediaString"});
    assert!(
        Model::from_json(&model.to_string())
            .unwrap()
            .lower(None)
            .is_ok()
    );
}

#[test]
fn malformed_and_undeclared_auth_overrides_are_diagnosed() {
    for value in [json!({}), json!(["smithy.api#httpBasicAuth"])] {
        let messages = refused(|model| {
            shape(model, "Ping")["traits"]["smithy.api#auth"] = value;
        });
        assert!(
            messages
                .iter()
                .any(|m| m.contains("Ping") && m.contains("@auth")),
            "{messages:?}"
        );
    }
}

#[test]
fn sensitivity_and_literal_queries_survive_lowering() {
    let service = Model::from_json(include_str!("fixtures/observability.json"))
        .unwrap()
        .lower(None)
        .unwrap();
    assert_eq!(
        service.operations[0].uri.query,
        vec![("mode".into(), Some("echo".into())), ("flag".into(), None)]
    );
    let Shape::Structure(profile) = service.shape(&id("example.observability#Profile")).unwrap()
    else {
        panic!("profile")
    };
    assert!(service.sensitive(profile.member("password").unwrap()));
    assert!(!service.sensitive(profile.member("name").unwrap()));
    let Shape::Structure(payload) = service.shape(&id("example.observability#Payload")).unwrap()
    else {
        panic!("payload")
    };
    assert!(service.sensitive(payload.member("whole").unwrap()));
    assert!(service.sensitive(payload.member("secretTarget").unwrap()));
}

fn id(text: &str) -> ShapeId {
    text.parse().unwrap()
}

fn weather() -> crucible_smithy::ir::Service {
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
    assert_eq!(
        names,
        [
            "CreateCity",
            "GetCity",
            "GetCityPhoto",
            "ListCities",
            "Ping",
            "PutCityPhoto",
            "Status"
        ]
    );
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

    assert_eq!(create.resource, Some(id("example.weather#City")));
    let ping = &service.operations[4];
    assert_eq!(ping.resource, None);
    assert_eq!((ping.input.as_ref(), ping.output.as_ref()), (None, None));
}

#[test]
fn operation_errors_come_before_service_errors() {
    let service = weather();
    assert_eq!(
        service.operations[0].errors,
        [
            id("example.weather#ConflictError"),
            id("example.weather#ThrottlingError"),
            id("example.weather#ValidationError")
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
    let list = service
        .operations
        .iter()
        .find(|op| op.name() == "ListCities")
        .unwrap();
    assert_eq!(
        list.paginated,
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
fn refuses_streaming_blobs() {
    let messages = refused(|model| {
        shape(model, "Blobby")
            .clone_from(&json!({ "type": "blob", "traits": { "smithy.api#streaming": {} } }));
        shape(model, "Coordinates")["members"]["raw"] =
            json!({ "target": "example.weather#Blobby" });
    });
    assert!(
        messages
            .iter()
            .any(|message| message.contains("requires streaming")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("top-level operation")),
        "{messages:?}"
    );
}

fn operation<'s>(
    service: &'s crucible_smithy::ir::Service,
    name: &str,
) -> &'s crucible_smithy::ir::Operation {
    service
        .operations
        .iter()
        .find(|op| op.name() == name)
        .unwrap()
}

#[test]
fn a_blob_payload_is_the_whole_body() {
    let service = weather();
    let put = operation(&service, "PutCityPhoto");
    assert_eq!(
        put.request.payload,
        Some(Payload {
            kind: crucible_smithy::ir::PayloadKind::Blob,
            streaming: false,
            requires_length: false,
            member: "photo".into(),
            media_type: "image/jpeg".into()
        })
    );
    assert!(put.request.body.is_empty());
    assert_eq!(put.request.headers[0].name, "X-Photo-Caption");
    assert_eq!(put.response.payload, None);
    assert_eq!(put.response.body, ["photo"]);

    let get = operation(&service, "GetCityPhoto");
    assert_eq!(get.response.payload.as_ref().unwrap().member, "photo");
    assert!(get.response.body.is_empty());

    let Some(Shape::Simple(photo)) = service.shape(&id("example.weather#Photo")) else {
        panic!("photo not lowered");
    };
    assert_eq!(photo.kind, SimpleKind::Blob);
    let length = photo.constraints.length.clone().unwrap();
    assert_eq!((length.min, length.max), (Some(1), Some(4096)));
}

#[test]
fn a_payload_without_a_media_type_is_octet_stream() {
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    shape(&mut model, "Photo")["traits"]
        .as_object_mut()
        .unwrap()
        .remove("smithy.api#mediaType");
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let put = operation(&service, "PutCityPhoto");
    assert_eq!(
        put.request.payload.as_ref().unwrap().media_type,
        "application/octet-stream"
    );
}

#[test]
fn refuses_payloads_it_does_not_carry() {
    let messages = refused(|model| {
        shape(model, "PutCityPhotoInput")["members"]["photo"]["target"] =
            json!("example.weather#Coordinates");
        shape(model, "GetCityPhotoOutput")["members"]["photo"]["target"] =
            json!("smithy.api#String");
    });
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(
        messages[0].contains("GetCityPhotoOutput$photo")
            && messages[0].contains("non-blob-payload"),
        "{messages:?}"
    );
    assert!(
        messages[1].contains("PutCityPhotoInput$photo") && messages[1].contains("non-blob-payload"),
        "{messages:?}"
    );
}

#[test]
fn refuses_a_payload_beside_body_members() {
    let messages = refused(|model| {
        shape(model, "PutCityPhotoInput")["members"]["note"] =
            json!({ "target": "smithy.api#String" });
    });
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(
        messages[0].contains("PutCityPhotoInput$note")
            && messages[0].contains("the body is the @httpPayload member `photo`"),
        "{messages:?}"
    );
}

#[test]
fn error_payloads_require_capability_and_nested_payload_traits_are_ignored() {
    let messages = refused(|model| {
        shape(model, "NotFoundError")["members"]["raw"] = json!({
            "target": "example.weather#Thumbnail",
            "traits": { "smithy.api#httpPayload": {} }
        });
        shape(model, "PhotoSummary")["members"]["thumbnail"]["traits"]["smithy.api#httpPayload"] =
            json!({});
    });
    assert!(
        messages
            .iter()
            .any(|m| m.contains("NotFoundError$raw") && m.contains("error-payload")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .all(|m| !m.contains("PhotoSummary$thumbnail")),
        "{messages:?}"
    );
}

#[test]
fn refuses_blobs_in_headers() {
    let messages = refused(|model| {
        shape(model, "PutCityPhotoInput")["members"]["caption"]["target"] =
            json!("example.weather#Thumbnail");
    });
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(
        messages[0].contains("@httpHeader cannot carry example.weather#Thumbnail"),
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
fn refuses_multiple_greedy_labels_even_with_capability() {
    let model = Model::from_json(include_str!("fixtures/multiple-greedy-labels.json")).unwrap();
    for capabilities in [
        Capabilities::new([Feature::GreedyLabels]),
        Capabilities::default(),
    ] {
        let Err(Error::Unsupported(diagnostics)) =
            model.lower_with_capabilities(None, &capabilities)
        else {
            panic!("expected diagnostics for multiple greedy labels");
        };
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.shape == id("example.greedy#Get")
                    && diagnostic.message == "a URI pattern may contain at most one greedy label"
            }),
            "{diagnostics}"
        );
    }
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
fn requires_explicit_capabilities_for_other_protocols_and_auth() {
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
    assert!(messages[0].contains("aws-rest-json-1"), "{messages:?}");
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

#[test]
fn refuses_a_media_type_that_is_not_a_header_value() {
    let messages = refused(|model| {
        shape(model, "Photo")["traits"]["smithy.api#mediaType"] = json!("image/jpeg\n");
    });
    assert!(
        messages
            .iter()
            .all(|m| m.contains("example.weather#Photo") && m.contains("not a valid header value")),
        "{messages:?}"
    );
}

#[test]
fn renamed_errors_cannot_share_a_wire_discriminator_even_on_distinct_operations() {
    let model = Model::from_json(include_str!("fixtures/error-wire-collision.json")).unwrap();
    let diagnostics = model.lower(None).unwrap_err();
    assert!(
        diagnostics
            .to_string()
            .contains("error wire code \"Problem\" collides with example.collisions#Problem"),
        "{diagnostics}"
    );
}

/// Weather with `extra` shapes reachable from `CitySummary` as members `m0..`.
fn weather_with_shapes(extra: &[&str]) -> Value {
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    for (n, id) in extra.iter().enumerate() {
        model["shapes"][*id] = json!({ "type": "string" });
        model["shapes"]["example.weather#CitySummary"]["members"][format!("m{n}")] =
            json!({ "target": id });
    }
    model
}

#[test]
fn shape_names_clashing_across_namespaces_or_by_case_are_diagnosed() {
    let model = weather_with_shapes(&[
        "example.weather#Thing",
        "other.ns#Thing",
        "example.weather#thing",
    ]);
    let diagnostics = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap_err()
        .to_string();
    assert!(
        diagnostics
            .contains("other.ns#Thing: shape name `Thing` is also used by example.weather#Thing"),
        "{diagnostics}"
    );
    assert!(
        diagnostics.contains("example.weather#thing: shape name `thing` differs from `Thing` of example.weather#Thing only by case"),
        "{diagnostics}"
    );
    assert!(diagnostics.contains("rename"), "{diagnostics}");
}

#[test]
fn renaming_clashing_shapes_on_the_service_resolves_the_diagnostic() {
    let mut model = weather_with_shapes(&[
        "example.weather#Thing",
        "other.ns#Thing",
        "example.weather#thing",
    ]);
    model["shapes"]["example.weather#Weather"]["rename"] = json!({
        "other.ns#Thing": "OtherThing",
        "example.weather#thing": "LowerThing",
    });
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    assert_eq!(
        service.name_of(&ShapeId::new("other.ns", "Thing")),
        "OtherThing"
    );
}
