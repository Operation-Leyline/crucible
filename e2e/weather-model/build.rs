//! Publishes the weather model: the fixture the other packages share.
fn main() {
    let model = "../../tests/models/weather";
    println!("cargo::rerun-if-changed={model}");
    crucible_build::model(model).publish();
}
