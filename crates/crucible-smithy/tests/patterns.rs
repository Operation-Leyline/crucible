use crucible_smithy::ir::Shape;
use crucible_smithy::{Model, ShapeId};

#[test]
fn ecma_patterns_translate_from_effective_ir_constraints() {
    let service = Model::from_json(include_str!("fixtures/patterns.json"))
        .unwrap()
        .lower(None)
        .unwrap();
    let Shape::Structure(input) = service
        .shape(&ShapeId::new("example.patterns", "Input"))
        .unwrap()
    else {
        panic!("input structure")
    };
    for member in &input.members {
        assert!(
            member.constraints.rust_pattern().unwrap().is_some(),
            "{}",
            member.name
        );
    }
    assert_eq!(
        input
            .member("digits")
            .unwrap()
            .constraints
            .rust_pattern()
            .unwrap()
            .unwrap(),
        "^[0-9]+$"
    );
    assert_eq!(
        input
            .member("literal")
            .unwrap()
            .constraints
            .rust_pattern()
            .unwrap()
            .unwrap(),
        r"^\\d\.\x{00e9}\x{41}\x{01}$"
    );
    for (member, translated) in [
        ("empty", r"^[^\x{0}-\x{10FFFF}]$"),
        ("any", r"^[\x{0}-\x{10FFFF}]$"),
        ("anySuffix", r"^[\x{0}-\x{10FFFF}]x\]$"),
    ] {
        assert_eq!(
            input
                .member(member)
                .unwrap()
                .constraints
                .rust_pattern()
                .unwrap()
                .unwrap(),
            translated
        );
    }
}

#[test]
fn unrepresentable_ecma_patterns_are_refused_by_the_shared_translation() {
    let service = Model::from_json(include_str!("fixtures/patterns-unsupported.json"))
        .unwrap()
        .lower(None)
        .unwrap();
    let Shape::Structure(input) = service
        .shape(&ShapeId::new("example.patterns", "Input"))
        .unwrap()
    else {
        panic!("input structure")
    };
    for member in &input.members {
        assert!(
            member.constraints.rust_pattern().is_err(),
            "{}",
            member.name
        );
    }
}
