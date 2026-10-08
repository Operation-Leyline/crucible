use crucible_smithy::ir::{Shape, TimestampFormat};
use crucible_smithy::{Capabilities, Feature, Model};
use serde_json::{Value, json};

const ACCEPTED: &str = include_str!("fixtures/examples.json");
/// The shapes the fixtures use are optional targets, so these cases exercise
/// examples rather than capability checks.
fn capabilities() -> Capabilities {
    Capabilities::new([Feature::Sparse, Feature::IntEnum, Feature::BigInteger])
}

const REFUSED: &str = include_str!("fixtures/examples-refused.json");

/// Diagnostics for one operation of the refused fixture, so each case reads alone.
fn diagnostics(operation: &str) -> Vec<String> {
    let mut model: Value = serde_json::from_str(REFUSED).unwrap();
    model["shapes"]["test.examples#Service"]["operations"] =
        json!([{ "target": format!("test.examples#{operation}") }]);
    Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &capabilities())
        .unwrap_err()
        .to_string()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn expect(operation: &str, expected: &[&str]) {
    let found = diagnostics(operation);
    for expected in expected {
        assert!(
            found.iter().any(|d| d.contains(expected)),
            "{operation}: missing {expected:?} in {found:#?}"
        );
    }
}

#[test]
fn accepted_examples_lower_in_model_order_with_their_values() {
    let service = Model::from_json(ACCEPTED)
        .unwrap()
        .lower_with_capabilities(None, &capabilities())
        .unwrap();
    let good = service
        .operations
        .iter()
        .find(|op| op.name() == "Good")
        .unwrap();
    let titles: Vec<_> = good.examples.iter().map(|e| e.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "Everything",
            "Constraint violations permitted",
            "Epoch timestamp",
            "Modelled error"
        ]
    );
    let first = &good.examples[0];
    assert_eq!(first.docs.as_deref(), Some("All kinds of values."));
    assert_eq!(first.input.as_ref().unwrap()["choice"], json!({"b": 2}));
    assert_eq!(first.output, Some(json!({"code": "ABC"})));
    assert!(good.examples[1].allow_constraint_errors);
    let error = good.examples[3].error.as_ref().unwrap();
    assert_eq!(error.shape.to_string(), "test.examples#Problem");
    assert_eq!(error.content, json!({"message": "no"}));
    let nothing = service
        .operations
        .iter()
        .find(|op| op.name() == "Nothing")
        .unwrap();
    assert_eq!(nothing.examples.len(), 2);
    assert!(nothing.examples[1].input.is_none());
}

#[test]
fn malformed_examples_are_diagnosed_against_the_operation() {
    expect("NotAList", &["@examples must be a list"]);
    expect("Unknown", &["@examples[0] is malformed", "inpt"]);
    expect("NoTitle", &["@examples[0] is malformed", "title"]);
    expect("Both", &["an example cannot have both output and error"]);
    expect(
        "WrongError",
        &["error.shapeId \"test.examples#Other\" is not an error of this operation"],
    );
}

#[test]
fn examples_are_checked_against_the_shapes_they_exercise() {
    expect("NotObject", &["input must be an object, not an array"]);
    expect("UnknownMember", &["input.nope is not a member"]);
    expect("MissingRequired", &["input.code is required"]);
    expect(
        "NoShape",
        &[
            "input must be empty: the operation declares none",
            "output must be empty: the operation declares none",
        ],
    );
    expect(
        "Mistyped",
        &[
            "input.count must be a integer, not a string",
            "input.ratio must be a number, not a boolean",
            "input.tags must be a array, not a string",
            "input.scores must be a object, not an array",
            "input.inner.flag must be a boolean, not a string",
            "input.level must be a integer, not a number",
            "input.when must be an RFC 3339 string or epoch seconds",
            "input.data must be valid base64",
            "input.big must be a integer, not a number",
            "input.byte: 128 is outside the range of the type (-128 to 127)",
        ],
    );
    expect(
        "BadMembers",
        &[
            "input.color: \"blue\" is not a value of test.examples#Color",
            "input.level: 2 is not a value of test.examples#Level",
            "input.choice must set exactly one union member",
            "input.tags[0] must not be null",
            "input.scores key \"abc\" does not match the pattern",
            "input.inner must not be null",
        ],
    );
}

#[test]
fn constraint_violations_are_refused_unless_the_example_allows_them() {
    expect(
        "Constraints",
        &[
            "input.code has 4 characters, outside its length bounds (3..3)",
            "input.code does not match the pattern \"^[A-Z]{3}$\"",
            "input.count: 0 is outside its range",
            "input.tags must not repeat items",
            "input.data has 5 bytes, outside its length bounds (..4)",
        ],
    );
}

#[test]
fn wire_examples_use_json_names_and_the_members_timestamp_format() {
    let service = Model::from_json(include_str!("fixtures/weather.json"))
        .unwrap()
        .lower(None)
        .unwrap();
    let ping = service.operations.iter().find(|op| op.name() == "Ping");
    assert!(ping.unwrap().examples.is_empty());
    let create = service
        .operations
        .iter()
        .find(|op| op.name() == "CreateCity")
        .unwrap();
    let Some(Shape::Structure(output)) = service.shape(create.output.as_ref().unwrap()) else {
        panic!("output structure")
    };
    let city = output.member("city").unwrap();
    let value = &create.examples[0].output.as_ref().unwrap()["city"];
    let wire = service.wire_example(city, value, None);
    // `createdAt` is epoch seconds by its own trait; `updatedAt` follows the protocol.
    assert_eq!(wire["createdAt"], json!(1767323045));
    assert_eq!(wire["updatedAt"], json!("2026-01-02T03:04:05Z"));
    let http = service.wire_example(
        city,
        &json!({"updatedAt": "2026-01-02T03:04:05Z"}),
        Some(TimestampFormat::EpochSeconds),
    );
    // A binding's format applies to the bound member, never inside a document.
    assert_eq!(http["updatedAt"], json!("2026-01-02T03:04:05Z"));
}
