//! `@deprecated`, `@since`, `@unstable` and `@externalDocumentation` are
//! lowered once into typed IR fields, so no generator re-reads the raw traits.

use crucible_smithy::ir::{Deprecation, DocNotes, Service, Shape};
use crucible_smithy::{Capabilities, Error, Feature, Model, ShapeId};

fn service() -> Service {
    Model::from_json(include_str!("fixtures/doc-notes.json"))
        .unwrap()
        .lower_with_capabilities(None, &Capabilities::new([Feature::IntEnum]))
        .unwrap()
}

fn id(name: &str) -> ShapeId {
    format!("example.notes#{name}").parse().unwrap()
}

fn deprecation(message: Option<&str>, since: Option<&str>) -> Option<Deprecation> {
    Some(Deprecation {
        message: message.map(str::to_owned),
        since: since.map(str::to_owned),
    })
}

#[test]
fn service_carries_since_and_external_documentation() {
    let service = service();
    assert_eq!(service.notes.since.as_deref(), Some("1.0"));
    assert_eq!(
        service.notes.external_docs.get("Home").map(String::as_str),
        Some("https://notes.example/docs")
    );
    assert!(service.notes.deprecated.is_none());
    assert!(!service.notes.unstable);
}

#[test]
fn operation_deprecation_keeps_message_and_since() {
    let service = service();
    let old = service
        .operations
        .iter()
        .find(|op| op.name() == "Old")
        .unwrap();
    assert_eq!(
        old.notes.deprecated,
        deprecation(Some("Use Plain."), Some("2.0"))
    );
    let plain = service
        .operations
        .iter()
        .find(|op| op.name() == "Plain")
        .unwrap();
    assert_eq!(plain.notes, DocNotes::default());
}

#[test]
fn external_documentation_is_sorted_by_name() {
    let service = service();
    let old = service
        .operations
        .iter()
        .find(|op| op.name() == "Old")
        .unwrap();
    let names: Vec<_> = old.notes.external_docs.keys().map(String::as_str).collect();
    assert_eq!(names, ["Alpha", "Zeta"]);
    assert_eq!(old.notes.lines(), Vec::<String>::new());
}

#[test]
fn resource_carries_unstable() {
    // Plain lowering does not populate resources; infrastructure lowering does,
    // and it takes no capabilities, so the intEnum is left out of the model.
    assert!(service().resources.is_empty());
    let mut model: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/doc-notes.json")).unwrap();
    model["shapes"]["example.notes#OldInput"]["members"]
        .as_object_mut()
        .unwrap()
        .remove("level");
    model["shapes"]
        .as_object_mut()
        .unwrap()
        .remove("example.notes#Level");
    let infra = Model::from_json(&model.to_string())
        .unwrap()
        .lower_infrastructure(None)
        .unwrap();
    assert!(infra.resources[&id("Thing")].notes.unstable);
}

#[test]
fn members_carry_each_deprecation_shape() {
    let service = service();
    let Some(Shape::Structure(input)) = service.shape(&id("OldInput")) else {
        panic!("structure")
    };
    let notes = |name: &str| &input.member(name).unwrap().notes;
    assert_eq!(notes("kept"), &DocNotes::default());
    assert_eq!(notes("bare").deprecated, deprecation(None, None));
    assert_eq!(
        notes("messageOnly").deprecated,
        deprecation(Some("Gone soon."), None)
    );
    assert_eq!(
        notes("sinceOnly").deprecated,
        deprecation(None, Some("3.1"))
    );
    assert_eq!(notes("sinceOnly").since.as_deref(), Some("1.1"));
    assert!(input.notes.unstable);
    assert_eq!(input.notes.since.as_deref(), Some("1.2"));
    assert_eq!(input.notes.lines(), ["Unstable.", "Since 1.2."]);
}

#[test]
fn deprecation_sentences_cover_every_field_combination() {
    assert_eq!(Deprecation::default().sentence(), "Deprecated.");
    assert_eq!(
        deprecation(Some("Use Plain."), Some("2.0"))
            .unwrap()
            .sentence(),
        "Deprecated since 2.0: Use Plain."
    );
    assert_eq!(
        deprecation(None, Some("2.0")).unwrap().note(),
        "Deprecated since 2.0."
    );
    assert_eq!(
        deprecation(Some("Use Plain."), Some("2.0")).unwrap().note(),
        "Use Plain. (since 2.0)"
    );
    assert_eq!(Deprecation::default().note(), "Deprecated.");
}

#[test]
fn enum_and_int_enum_values_carry_their_notes() {
    let service = service();
    let Some(Shape::Enum(color)) = service.shape(&id("Color")) else {
        panic!("enum")
    };
    assert_eq!(
        color.values[0].notes.deprecated,
        deprecation(Some("Use CRIMSON."), None)
    );
    assert_eq!(color.values[1].notes.since.as_deref(), Some("1.5"));
    let Some(Shape::IntEnum(level)) = service.shape(&id("Level")) else {
        panic!("intEnum")
    };
    assert_eq!(
        level.values[0].notes.deprecated,
        deprecation(None, Some("2.0"))
    );
    assert!(level.values[1].notes.deprecated.is_none());
}

#[test]
fn legacy_enum_definition_deprecated_flag_becomes_a_deprecation() {
    let service = service();
    let Some(Shape::Enum(legacy)) = service.shape(&id("Legacy")) else {
        panic!("enum")
    };
    assert_eq!(legacy.values[0].notes.deprecated, deprecation(None, None));
    assert!(legacy.values[1].notes.deprecated.is_none());
    assert!(legacy.values[2].notes.deprecated.is_none());
}

#[test]
fn collection_shapes_carry_deprecation() {
    let service = service();
    let Some(Shape::List(tags)) = service.shape(&id("Tags")) else {
        panic!("list")
    };
    assert_eq!(
        tags.notes.deprecated,
        deprecation(Some("Send a map instead."), None)
    );
}

#[test]
fn prose_joins_docs_notes_and_the_deprecation_sentence() {
    let notes = DocNotes {
        deprecated: deprecation(Some("Use Plain."), None),
        since: Some("1.0".into()),
        unstable: true,
        ..Default::default()
    };
    assert_eq!(
        notes.prose(Some("Does a thing.")).unwrap(),
        "Does a thing.\n\nUnstable.\n\nSince 1.0.\n\nDeprecated: Use Plain."
    );
    assert_eq!(DocNotes::default().prose(None), None);
    assert_eq!(DocNotes::default().prose(Some("  ")), None);
}

#[test]
fn malformed_documentation_traits_are_collected_diagnostics() {
    let model = Model::from_json(include_str!("fixtures/doc-notes-malformed.json")).unwrap();
    for result in [model.lower(None), model.lower_infrastructure(None)] {
        let Error::Unsupported(diagnostics) = result.unwrap_err() else {
            panic!("unsupported")
        };
        for (shape, message) in [
            (
                "example.notes#Bad",
                "@externalDocumentation must be a map of names to URLs",
            ),
            ("example.notes#Call", "@deprecated must be a structure"),
            (
                "example.notes#Input$messageNotString",
                "@deprecated message must be a string",
            ),
            (
                "example.notes#Input$sinceNotString",
                "@deprecated since must be a string",
            ),
            (
                "example.notes#Input$sinceNotString",
                "@since must be a string",
            ),
            (
                "example.notes#Input$linkNotString",
                "@externalDocumentation entry \"Guide\" must be a string URL",
            ),
            (
                "example.notes#Legacy",
                "@enum definition deprecated must be boolean",
            ),
        ] {
            assert!(
                diagnostics
                    .iter()
                    .any(|d| d.shape.to_string() == shape && d.message.contains(message)),
                "missing {shape}: {message} in {diagnostics}"
            );
        }
        assert_eq!(diagnostics.len(), 7, "{diagnostics}");
    }
}
