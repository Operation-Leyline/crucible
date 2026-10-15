//! Publishes the weather model: the fixture the other packages share.
fn main() {
    let model = "../../crates/crucible-smithy/tests/fixtures/weather.json";
    println!("cargo::rerun-if-changed={model}");
    crucible_build::model(model).publish();
}
