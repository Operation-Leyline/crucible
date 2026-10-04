fn main() {
    let model = "../../crates/crucible-smithy/tests/fixtures/observability.json";
    println!("cargo::rerun-if-changed={model}");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(
        out.join("observability.rs"),
        crucible_build::Codegen::from_model(model).render().unwrap(),
    )
    .unwrap();
    std::fs::write(
        out.join("observability_client.rs"),
        crucible_build::Codegen::client_from_model(model)
            .render()
            .unwrap(),
    )
    .unwrap();
    crucible_build::service("weather_model").generate();
}
