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
    // Typed permissions: tests/permissions.rs serves this model's bindings
    // and enforces their requirements end to end.
    println!("cargo::rerun-if-changed=permissions");
    std::fs::write(
        out.join("permissions.rs"),
        crucible_build::model("permissions")
            .server()
            .mcp()
            .render()
            .unwrap(),
    )
    .unwrap();
    // The generators' shared fixture: a binding through a union arm, a
    // filter, a handler clause and an internal permission.
    let leyline = "../../tests/fixtures/permissions";
    println!("cargo::rerun-if-changed={leyline}");
    std::fs::write(
        out.join("leyline_permissions.rs"),
        crucible_build::model(leyline).server().render().unwrap(),
    )
    .unwrap();
    // A service with errors of its own that designates its 404: Hidden is
    // built by the service's not-found factory (tests/permissions_not_found.rs).
    let desk = "../../tests/fixtures/permissions-not-found";
    println!("cargo::rerun-if-changed={desk}");
    std::fs::write(
        out.join("permissions_not_found.rs"),
        crucible_build::model(desk).server().mcp().render().unwrap(),
    )
    .unwrap();
    // And its client: open permission constants without the internal ones.
    std::fs::write(
        out.join("permissions_client.rs"),
        crucible_build::model("permissions")
            .client()
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
    // Every kind of `when` condition and binding, through the interceptor.
    std::fs::write(
        out.join("conditions.rs"),
        crucible_build::model("conditions")
            .server()
            .render()
            .unwrap(),
    )
    .unwrap();
    // Permissions across releases: a removed operation's legacy handler is
    // authorized by its own release's requirements.
    std::fs::write(
        out.join("versioned_permissions.rs"),
        crucible_build::model("versioned_permissions")
            .server()
            .render()
            .unwrap(),
    )
    .unwrap();
    // A client of the current release, which names it in `Crucible-Version`.
    std::fs::write(
        out.join("versioned_client.rs"),
        crucible_build::model("versioned")
            .client()
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
