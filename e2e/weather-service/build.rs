fn main() {
    let auth = "../../crates/crucible-smithy/tests/fixtures/auth-chain.json";
    println!("cargo::rerun-if-changed={auth}");
    let auth_out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(
        auth_out.join("auth_chain.rs"),
        crucible_build::model(auth)
            .server()
            .mcp()
            .explorer()
            .render()
            .unwrap(),
    )
    .unwrap();
    let scopes = "../../crates/crucible-smithy/tests/fixtures/scopes.json";
    println!("cargo::rerun-if-changed={scopes}");
    std::fs::write(
        auth_out.join("scopes.rs"),
        crucible_build::model(scopes)
            .server()
            .mcp()
            .render()
            .unwrap(),
    )
    .unwrap();
    let model = "../../crates/crucible-smithy/tests/fixtures/observability.json";
    println!("cargo::rerun-if-changed={model}");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(
        out.join("observability.rs"),
        crucible_build::model(model)
            .server()
            .mcp()
            .render()
            .unwrap(),
    )
    .unwrap();
    std::fs::write(
        out.join("observability_client.rs"),
        crucible_build::model(model).client().render().unwrap(),
    )
    .unwrap();
    let cors = "../../crates/crucible-smithy/tests/fixtures/cors.json";
    println!("cargo::rerun-if-changed={cors}");
    std::fs::write(
        out.join("cors.rs"),
        crucible_build::model(cors).server().render().unwrap(),
    )
    .unwrap();
    let model = "../../crates/crucible-smithy/tests/fixtures/internal-cors.json";
    println!("cargo::rerun-if-changed={model}");
    std::fs::write(
        out.join("audience_server.rs"),
        crucible_build::model(model)
            .server()
            .mcp()
            .explorer()
            .render()
            .unwrap(),
    )
    .unwrap();
    // Three releases of one API, the older two served from the current
    // implementation (docs/api-versions.md).
    std::fs::write(
        out.join("versioned.rs"),
        crucible_build::model("versioned")
            .server()
            .mcp()
            .render()
            .unwrap(),
    )
    .unwrap();
    crucible_build::model("weather_model")
        .server()
        .explorer()
        .mcp()
        .conformance()
        .generate();
}
