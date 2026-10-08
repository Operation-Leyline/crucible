fn main() {
    let auth = "../../crates/crucible-smithy/tests/fixtures/auth-chain.json";
    println!("cargo::rerun-if-changed={auth}");
    let auth_out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(
        auth_out.join("auth_chain.rs"),
        crucible_build::RustCodegen::from_model(auth)
            .render()
            .unwrap(),
    )
    .unwrap();
    let model = "../../crates/crucible-smithy/tests/fixtures/observability.json";
    println!("cargo::rerun-if-changed={model}");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(
        out.join("observability.rs"),
        crucible_build::RustCodegen::from_model(model)
            .mcp()
            .render()
            .unwrap(),
    )
    .unwrap();
    std::fs::write(
        out.join("observability_client.rs"),
        crucible_build::RustCodegen::client_from_model(model)
            .render()
            .unwrap(),
    )
    .unwrap();
    let cors = "../../crates/crucible-smithy/tests/fixtures/cors.json";
    println!("cargo::rerun-if-changed={cors}");
    std::fs::write(
        out.join("cors.rs"),
        crucible_build::RustCodegen::from_model(cors)
            .render()
            .unwrap(),
    )
    .unwrap();
    let model = "../../crates/crucible-smithy/tests/fixtures/internal-cors.json";
    println!("cargo::rerun-if-changed={model}");
    std::fs::write(
        out.join("audience_server.rs"),
        crucible_build::RustCodegen::from_model(model)
            .mcp()
            .render()
            .unwrap(),
    )
    .unwrap();
    crucible_build::rust_service("weather_model")
        .mcp()
        .conformance()
        .generate();
}
