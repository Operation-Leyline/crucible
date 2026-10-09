//! The weather model exists twice: as the Smithy fixture every generator is
//! tested with, and as `docs/idl/examples/weather`. Both front ends must
//! produce the same IR (decision 63), apart from deliberate language changes,
//! which are applied to the Smithy side before comparing.

use std::path::Path;

use crucible_lang::{Package, SourceFile, compile};
use crucible_smithy::ir::{Service, Shape};

fn smithy_weather() -> Service {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../crucible-smithy/tests/fixtures/weather.json");
    let json = std::fs::read_to_string(path).expect("weather fixture");
    crucible_smithy::Model::from_json(&json)
        .expect("model")
        .lower(None)
        .expect("lowered")
}

fn cru_weather() -> (Package, crucible_lang::Compiled) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/idl/examples/weather/model/weather.cru");
    let text = std::fs::read_to_string(&path).expect("weather.cru");
    let package = Package {
        namespace: "example.weather".to_owned(),
        files: vec![SourceFile {
            path: path.display().to_string(),
            text,
        }],
        naming: Default::default(),
        imports: Vec::new(),
    };
    let compiled = compile(&package, None);
    (package, compiled)
}

/// The deliberate differences between the two front ends:
/// - decision 20: members are never client-optional, where Smithy makes every
///   member of an `@input` structure optional for clients;
/// - decision 43: raw `traits` carry project metadata only, because every
///   standard trait is a typed IR field.
fn as_cru_lowers_it(mut service: Service) -> Service {
    let standard = |id: &crucible_smithy::ShapeId| {
        id.is_prelude()
            || id.namespace().starts_with("aws.")
            || id.namespace() == "crucible.protocols"
            || id.namespace() == "smithy.test"
    };
    let strip = |traits: &mut crucible_smithy::ir::Traits| traits.retain(|id, _| !standard(id));
    strip(&mut service.traits);
    for operation in &mut service.operations {
        strip(&mut operation.traits);
    }
    for shape in service.shapes.values_mut() {
        match shape {
            Shape::Unit { .. } => {}
            Shape::Simple(s) => strip(&mut s.traits),
            Shape::Enum(s) => strip(&mut s.traits),
            Shape::IntEnum(s) => strip(&mut s.traits),
            Shape::List(s) => {
                strip(&mut s.traits);
                strip(&mut s.member.traits);
            }
            Shape::Map(s) => {
                strip(&mut s.traits);
                strip(&mut s.key.traits);
                strip(&mut s.value.traits);
            }
            Shape::Structure(s) | Shape::Union(s) => {
                strip(&mut s.traits);
                for member in &mut s.members {
                    member.client_optional = false;
                    strip(&mut member.traits);
                }
            }
        }
    }
    service
}

/// The first differing lines of two pretty-printed values, with context.
fn first_difference(expected: &str, actual: &str) -> Option<String> {
    let expected: Vec<&str> = expected.lines().collect();
    let actual: Vec<&str> = actual.lines().collect();
    let index = expected
        .iter()
        .zip(&actual)
        .position(|(a, b)| a != b)
        .or_else(|| (expected.len() != actual.len()).then(|| expected.len().min(actual.len())))?;
    let start = index.saturating_sub(25);
    let mut out = String::new();
    for line in &expected[start..index] {
        out.push_str(&format!("  {line}\n"));
    }
    for line in expected.iter().skip(index).take(8) {
        out.push_str(&format!("- {line}\n"));
    }
    for line in actual.iter().skip(index).take(8) {
        out.push_str(&format!("+ {line}\n"));
    }
    Some(out)
}

#[test]
fn weather_cru_lowers_to_the_same_ir_as_the_smithy_fixture() {
    let (_, compiled) = cru_weather();
    assert!(compiled.diagnostics.is_empty(), "\n{}", compiled.render());
    let actual = compiled.service.expect("service");
    let expected = as_cru_lowers_it(smithy_weather());
    assert_eq!(actual.operations.len(), 7);
    assert_eq!(actual.shapes.len(), expected.shapes.len());
    let expected = format!("{expected:#?}");
    let actual = format!("{actual:#?}");
    if let Some(difference) = first_difference(&expected, &actual) {
        panic!("IR differs (- Smithy, + .cru):\n{difference}");
    }
}

/// The comparison above is only worth something if it can fail.
#[test]
fn the_comparison_notices_a_one_field_change() {
    let (_, compiled) = cru_weather();
    let mut changed = compiled.service.expect("service");
    let expected = format!("{changed:#?}");
    let Some(Shape::Structure(summary)) = changed.shapes.values_mut().find(|shape| {
        matches!(shape, Shape::Structure(s) if s.members.iter().any(|m| m.name == "population"))
    }) else {
        panic!("CitySummary");
    };
    summary.members[2].required = true;
    let difference = first_difference(&expected, &format!("{changed:#?}")).expect("a difference");
    assert!(difference.contains("required: true"), "{difference}");
}
