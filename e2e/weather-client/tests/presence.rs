use crucible_client_runtime_rust::{Field, UnknownUnion};
use serde_json::{Value, json};

#[allow(dead_code)]
mod fixture {
    include!(concat!(env!("OUT_DIR"), "/presence_client.rs"));
}
use fixture::model::{Choice, Root, State};

#[test]
fn native_builders_preserve_omission_defaults_and_collision_names() {
    let error = Root::builder().build().unwrap_err();
    assert_eq!((error.shape, error.member), ("Root", "requiredDocument"));
    let mut value = Root::builder()
        .required_document(Value::Null)
        .builder("ordinary")
        .member_build2("collision")
        .items(vec![json!({"a": 1})])
        .build()
        .unwrap();
    assert_eq!(
        value.member_builder2().map(String::as_str),
        Some("ordinary")
    );
    assert_eq!(value.build().map(String::as_str), Some("collision"));
    assert_eq!(value.items().unwrap().len(), 1);
    assert!(
        value.fallback.is_absent(),
        "optional defaults remain omitted in requests"
    );
    assert_eq!(value.document(), None);
    value.document = Field::Null;
    assert_eq!(value.document(), None);
    assert!(value.document.is_null(), "advanced presence is retained");
    let decoded: Root = serde_json::from_value(serde_json::to_value(value).unwrap()).unwrap();
    assert_eq!(
        decoded.fallback(),
        None,
        "clientOptional suppresses the modeled default"
    );
    let city = weather_client::model::CitySummary::builder()
        .id("city")
        .name("Oslo")
        .build()
        .unwrap();
    assert_eq!(
        *city.population(),
        0,
        "required default has its modeled value"
    );
}

#[test]
fn required_document_null_is_present_and_missing_is_rejected() {
    assert!(serde_json::from_str::<Root>("{}").is_err());
    let value: Root = serde_json::from_str(r#"{"requiredDocument":null}"#).unwrap();
    assert_eq!(value.required_document, Value::Null);
    assert_eq!(value.document, Field::Absent);
    assert_eq!(value.fallback, Field::Absent);
    assert_eq!(
        serde_json::to_value(value).unwrap(),
        json!({"requiredDocument":null})
    );
}

#[test]
fn optional_documents_lists_recursive_members_and_defaults_keep_presence() {
    for wire in [
        json!({"requiredDocument":null,"fallback":3}),
        json!({"requiredDocument":null,"document":null,"items":null,"next":null,"fallback":null,"httpDate":null}),
        json!({"requiredDocument":{},"document":[],"items":[{"nil":null},{},[]],"next":{"requiredDocument":null,"fallback":3},"fallback":0,"httpDate":"Thu, 01 Jan 1970 00:00:00 GMT"}),
    ] {
        let value: Root = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(value).unwrap(), wire);
    }
    let value: Root =
        serde_json::from_str(r#"{"requiredDocument":null,"document":null,"items":null}"#).unwrap();
    assert_eq!(value.document, Field::Null);
    assert_eq!(value.items, Field::Null);
}

#[test]
fn unknown_variant_names_do_not_collide_with_declared_unknown_names() {
    assert_eq!(
        serde_json::from_str::<State>(r#""known""#).unwrap(),
        State::UnknownValue
    );
    let state = serde_json::from_str::<State>(r#""future""#).unwrap();
    assert_eq!(state, State::UnknownValue2("future".into()));
    assert_eq!(serde_json::to_string(&state).unwrap(), r#""future""#);
    assert_eq!(state.as_str(), "future");
    assert!(matches!(
        serde_json::from_str::<Choice>(r#"{"unknown":{}}"#).unwrap(),
        Choice::Unknown(_)
    ));
    let wire = json!({"future":{"n":9007199254740993_u64,"object":{},"array":[]}});
    let value: Choice = serde_json::from_value(wire.clone()).unwrap();
    assert!(matches!(value, Choice::UnknownValue3(_)));
    assert_eq!(serde_json::to_value(value).unwrap(), wire);
    let known: Choice = serde_json::from_str(r#"{"document":null}"#).unwrap();
    assert!(matches!(known, Choice::Document(Value::Null)));
}

#[test]
fn malformed_known_and_multi_arm_unions_do_not_fall_back_to_unknown() {
    for wire in [
        "{}",
        "[]",
        r#"{"text":42}"#,
        r#"{"text":"ok","future":1}"#,
        r#"{"future":1,"other":2}"#,
        r#"{"future":1,"future":2}"#,
    ] {
        assert!(serde_json::from_str::<Choice>(wire).is_err(), "{wire}");
    }
    let forged = Choice::UnknownValue3(UnknownUnion {
        name: "text".into(),
        value: json!(42),
    });
    assert!(serde_json::to_value(forged).is_err());
}
