//! Generated body names must not replace another operation's contract.

use std::path::Path;

use crucible_ir::{ShapeId, ir::Shape};
use crucible_lang::{Package, compile, load_package};

fn collision_package() -> Package {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models/body-name-collision-refusal");
    // The distinct manifest templates are valid; only their expansion for
    // these particular operations collides.
    load_package(&dir).expect("valid naming templates")
}

#[test]
fn colliding_body_names_diagnose_both_bodies_without_replacing_the_first() {
    for operations in [
        "AConsume, Consume",
        "Consume, AConsume",
        "AConsume",
        "Consume",
        // The service's duplicate-operation error must not make one body's
        // repeated registration look like a second source body.
        "AConsume, AConsume, Consume",
        "Consume, Consume, AConsume",
    ] {
        for reverse_files in [false, true] {
            let mut package = collision_package();
            let service = package
                .files
                .iter_mut()
                .find(|file| file.path.ends_with("service.cru"))
                .unwrap();
            service.text = service.text.replace(
                "operations [AConsume, Consume]",
                &format!("operations [{operations}]"),
            );
            if reverse_files {
                package.files.reverse();
            }
            let compiled = compile(&package, None);
            assert!(compiled.has_errors(), "{}", compiled.render());
            assert!(compiled.service.is_none());
            let collisions: Vec<_> = compiled
                .diagnostics
                .iter()
                .filter(|item| {
                    item.diagnostic.message.contains(
                        "`AConsumeBody` is the generated name of more than one operation body",
                    )
                })
                .collect();
            assert_eq!(collisions.len(), 2, "{}", compiled.render());
            let mut bodies: Vec<_> = collisions
                .iter()
                .map(|item| {
                    assert!(item.diagnostic.is_error());
                    let file = &compiled.files[item.file.expect("body file")];
                    item.diagnostic.span.text(&file.text)
                })
                .collect();
            bodies.sort_unstable();
            assert_eq!(
                bodies,
                [
                    "input { inputOnly: string }",
                    "output { outputOnly: integer }"
                ]
            );

            // An editor may consume best-effort IR after errors. Keep the
            // first registered body, including when the other is unreached.
            let service = compiled.best_effort.expect("best-effort service");
            let Shape::Structure(body) =
                &service.shapes[&ShapeId::new("example.collision", "AConsumeBody")]
            else {
                panic!("operation body structure");
            };
            let members: Vec<_> = body
                .members
                .iter()
                .map(|member| member.name.as_str())
                .collect();
            let first = if operations.starts_with("AConsume") {
                "inputOnly"
            } else {
                "outputOnly"
            };
            assert_eq!(
                members,
                [first],
                "operations [{operations}], reverse_files={reverse_files}"
            );
        }
    }
}

#[test]
fn distinct_body_names_preserve_both_contracts() {
    let mut package = collision_package();
    package.naming.output = "{Operation}Reply".into();
    let compiled = compile(&package, None);
    assert!(!compiled.has_errors(), "{}", compiled.render());
    let service = compiled.service.expect("service");
    for (name, member) in [
        ("AConsumeBody", "inputOnly"),
        ("ConsumeReply", "outputOnly"),
    ] {
        let Shape::Structure(body) = &service.shapes[&ShapeId::new("example.collision", name)]
        else {
            panic!("operation body structure");
        };
        assert_eq!(body.members.len(), 1);
        assert_eq!(body.members[0].name, member);
    }
}
