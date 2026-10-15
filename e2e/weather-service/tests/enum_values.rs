//! A generated string enum lists its wire values, so a service can enumerate them.
use weather_service::weather::Sky;

#[test]
fn a_string_enum_lists_its_wire_values_in_declaration_order() {
    assert_eq!(Sky::VALUES, ["clear", "cloudy"]);
    assert!(Sky::VALUES.contains(&Sky::Clear.as_str()));
}
