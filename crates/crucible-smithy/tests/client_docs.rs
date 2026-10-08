//! The documentation and sensitivity facts that native client generators read
//! to write API docs and redacted string representations.
use crucible_smithy::{Capabilities, Feature, Model, ShapeId, ir::Shape};

fn service() -> crucible_smithy::ir::Service {
    Model::from_json(include_str!("fixtures/client-docs.json"))
        .unwrap()
        .lower_with_capabilities(None, &Capabilities::new([Feature::IntEnum]))
        .unwrap()
}

fn id(name: &str) -> ShapeId {
    format!("example.vault#{name}").parse().unwrap()
}

#[test]
fn documentation_reaches_every_kind_of_shape() {
    let service = service();
    assert!(
        service
            .docs
            .as_deref()
            .unwrap()
            .starts_with("Stores secrets.")
    );
    let operation = service
        .operations
        .iter()
        .find(|operation| operation.name() == "GetSecret")
        .unwrap();
    assert_eq!(operation.docs.as_deref(), Some("Reads one secret."));
    let Shape::Structure(secret) = service.shape(&id("Secret")).unwrap() else {
        panic!("structure")
    };
    assert_eq!(secret.docs.as_deref(), Some("A stored secret."));
    assert_eq!(
        secret.member("name").unwrap().docs.as_deref(),
        Some("The <b>name</b> of the secret.")
    );
    let Shape::Enum(kind) = service.shape(&id("Kind")).unwrap() else {
        panic!("enum")
    };
    assert_eq!(kind.docs.as_deref(), Some("What a secret unlocks."));
    assert_eq!(kind.values[0].docs.as_deref(), Some("A website login."));
    let Shape::IntEnum(level) = service.shape(&id("Level")).unwrap() else {
        panic!("int enum")
    };
    assert_eq!(level.values[0].docs.as_deref(), Some("Rarely rotated."));
    let Shape::Union(credential) = service.shape(&id("Credential")).unwrap() else {
        panic!("union")
    };
    assert_eq!(
        credential.member("password").unwrap().docs.as_deref(),
        Some("A password.")
    );
}

#[test]
fn sensitivity_covers_members_and_the_shapes_they_target() {
    let service = service();
    let Shape::Structure(secret) = service.shape(&id("Secret")).unwrap() else {
        panic!("structure")
    };
    let sensitive = |name: &str| service.sensitive(secret.member(name).unwrap());
    // On the member itself, and through a @sensitive target shape.
    assert!(sensitive("pin"));
    assert!(sensitive("password"));
    // A collection of sensitive elements is not itself @sensitive; generators
    // decide how to treat it.
    assert!(!sensitive("history"));
    assert!(!sensitive("name"));
}
