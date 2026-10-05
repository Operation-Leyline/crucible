fn main() {
    crucible_build::client("weather_model").generate();
    println!("cargo::rerun-if-changed=tests/fixtures/presence.json");
    let code = crucible_build::Codegen::client_from_model("tests/fixtures/presence.json")
        .render()
        .expect("client presence fixture");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::write(out.join("presence_client.rs"), code).unwrap();
    let fixture = "../../crates/crucible-smithy/tests/fixtures/review-hardening.json";
    println!("cargo::rerun-if-changed={fixture}");
    let code = crucible_build::Codegen::client_from_model(fixture)
        .render()
        .expect("hardening fixture");
    std::fs::write(out.join("hardening_client.rs"), code).unwrap();
}
