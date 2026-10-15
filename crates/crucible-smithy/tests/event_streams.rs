//! Event streams: which member carries the stream, where the other members
//! travel, event bindings, and the constructs refused.
use crucible_smithy::ir::{EventBinding, Operation, PayloadKind, Service, Shape};
use crucible_smithy::{Capabilities, Feature, Model, Protocol};
use serde_json::{Value, json};

const MODEL: &str = include_str!("fixtures/event-streams.json");

fn all() -> Capabilities {
    Capabilities::new(Feature::ALL.iter().copied())
}

fn lower(model: &Value) -> Result<Service, String> {
    Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &all())
        .map_err(|error| error.to_string())
}

fn op<'s>(service: &'s Service, name: &str) -> &'s Operation {
    service
        .operations
        .iter()
        .find(|op| op.name() == name)
        .unwrap()
}

#[test]
fn event_streams_require_the_target_capability() {
    let model = Model::from_json(MODEL).unwrap();
    let error = model
        .lower_with_capabilities(None, &Capabilities::default())
        .unwrap_err()
        .to_string();
    assert!(error.contains("event-streams"), "{error}");
    assert!(error.contains("smithy.api#eventHeader"), "{error}");
}

#[test]
fn other_members_travel_in_http_bindings() {
    let service = lower(&serde_json::from_str(MODEL).unwrap()).unwrap();
    assert_eq!(service.protocol, Protocol::CrucibleJson);
    let publish = op(&service, "Publish");
    let stream = publish.request.event_stream.as_ref().unwrap();
    assert_eq!(stream.member, "events");
    assert_eq!(stream.target.to_string(), "example.events#ClientEvents");
    let payload = publish.request.payload.as_ref().unwrap();
    assert_eq!(payload.kind, PayloadKind::EventStream);
    assert_eq!(payload.media_type, "application/vnd.amazon.eventstream");
    assert_eq!(publish.request.labels, ["channel"]);
    assert!(publish.request.body.is_empty());
    assert!(publish.response.event_stream.is_none());
    assert_eq!(publish.response.body, ["accepted"]);

    let subscribe = op(&service, "Subscribe");
    assert!(subscribe.request.event_stream.is_none());
    let stream = subscribe.response.event_stream.as_ref().unwrap();
    assert_eq!(stream.target.to_string(), "example.events#ServerEvents");
    assert_eq!(subscribe.response.headers[0].member, "channel");

    let chat = op(&service, "Chat");
    assert!(chat.request.event_stream.is_some() && chat.response.event_stream.is_some());
}

#[test]
fn event_members_bind_headers_and_payloads() {
    let service = lower(&serde_json::from_str(MODEL).unwrap()).unwrap();
    let structure =
        |name: &str| match service.shape(&format!("example.events#{name}").parse().unwrap()) {
            Some(Shape::Structure(structure)) => structure,
            _ => panic!("{name}"),
        };
    let message = structure("MessageEvent");
    for header in ["id", "sequence", "urgent", "sentAt"] {
        assert_eq!(
            message.member(header).unwrap().event_binding,
            Some(EventBinding::Header)
        );
    }
    assert_eq!(
        message.member("text").unwrap().event_binding,
        Some(EventBinding::Payload)
    );
    assert!(
        structure("StructuredEvent")
            .members
            .iter()
            .all(|member| member.event_binding.is_none())
    );
    let Some(Shape::Union(events)) = service.shape(&"example.events#ServerEvents".parse().unwrap())
    else {
        panic!()
    };
    assert!(events.streaming);
    assert!(structure("ThrottledError").error.is_some());
}

/// An edit that makes the fixture invalid.
type Change = Box<dyn Fn(&mut Value)>;

#[test]
fn malformed_event_streams_are_diagnostics_against_their_shapes() {
    let cases: Vec<(&str, Change)> = vec![
        (
            "event binding targets an unsupported shape type",
            Box::new(|model| {
                model["shapes"]["example.events#MessageEvent"]["members"]["list"] = json!({
                    "target": "example.events#Values", "traits": {"smithy.api#eventHeader": {}}
                });
            }),
        ),
        (
            "only one member may bind @eventPayload",
            Box::new(|model| {
                model["shapes"]["example.events#DataEvent"]["members"]["second"] = json!({
                    "target": "smithy.api#Blob", "traits": {"smithy.api#eventPayload": {}}
                });
            }),
        ),
        (
            "every other member beside @eventPayload must bind @eventHeader",
            Box::new(|model| {
                model["shapes"]["example.events#MessageEvent"]["members"]["loose"] =
                    json!({"target": "smithy.api#String"});
            }),
        ),
        (
            "event stream variants must target structures",
            Box::new(|model| {
                model["shapes"]["example.events#ClientEvents"]["members"]["raw"] =
                    json!({"target": "smithy.api#String"});
            }),
        ),
        (
            "@eventHeader and @eventPayload apply only to members of event stream events",
            Box::new(|model| {
                model["shapes"]["example.events#Nested"]["members"]["header"] = json!({
                    "target": "smithy.api#String", "traits": {"smithy.api#eventHeader": {}}
                });
            }),
        ),
        (
            "streaming shapes must be top-level operation input or output members",
            Box::new(|model| {
                model["shapes"]["example.events#Nested"]["members"]["events"] =
                    json!({"target": "example.events#ServerEvents"});
            }),
        ),
        (
            "only one member may target a streaming shape",
            Box::new(|model| {
                model["shapes"]["example.events#ChatInput"]["members"]["more"] = json!({
                    "target": "example.events#ClientEvents"
                });
            }),
        ),
        (
            "to a label, query parameter or header",
            Box::new(|model| {
                model["shapes"]["example.events#ChatInput"]["members"]["topic"] =
                    json!({"target": "smithy.api#String"});
            }),
        ),
        (
            "streaming members must bind @httpPayload",
            Box::new(|model| {
                model["shapes"]["example.events#ChatInput"]["members"]["events"]["traits"] =
                    json!({});
            }),
        ),
    ];
    for (expected, change) in cases {
        let mut model: Value = serde_json::from_str(MODEL).unwrap();
        change(&mut model);
        let error = lower(&model).unwrap_err();
        assert!(error.contains(expected), "{expected}: {error}");
    }
}
