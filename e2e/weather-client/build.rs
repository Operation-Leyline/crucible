fn main() {
    crucible_build::client("weather_model").generate();
    println!("cargo::rerun-if-changed=tests/fixtures/presence.json");
    let code = crucible_build::Codegen::client_from_model("tests/fixtures/presence.json")
        .render()
        .expect("client presence fixture");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let fixture = "tests/fixtures/endpoints.json";
    println!("cargo::rerun-if-changed={fixture}");
    std::fs::write(
        out.join("endpoint_client.rs"),
        crucible_build::Codegen::client_from_model(fixture)
            .render()
            .expect("endpoint and waiter fixture"),
    )
    .unwrap();
    let fixture = "../../crates/crucible-codegen-ts/tests/fixtures/sigv4.json";
    println!("cargo::rerun-if-changed={fixture}");
    std::fs::write(
        out.join("sigv4_client.rs"),
        crucible_build::Codegen::client_from_model(fixture)
            .render()
            .expect("SigV4 fixture"),
    )
    .unwrap();
    std::fs::write(out.join("presence_client.rs"), code).unwrap();
    let fixture = "../../crates/crucible-smithy/tests/fixtures/review-hardening.json";
    println!("cargo::rerun-if-changed={fixture}");
    let code = crucible_build::Codegen::client_from_model(fixture)
        .render()
        .expect("hardening fixture");
    std::fs::write(out.join("hardening_client.rs"), code).unwrap();
    let fixture = "../../crates/crucible-codegen-ts/tests/fixtures/dynamic-bindings.json";
    println!("cargo::rerun-if-changed={fixture}");
    for (name, codegen) in [
        (
            "dynamic_client.rs",
            crucible_build::Codegen::client_from_model(fixture),
        ),
        (
            "dynamic_server.rs",
            crucible_build::Codegen::from_model(fixture),
        ),
    ] {
        std::fs::write(
            out.join(name),
            codegen.render().expect("dynamic bindings fixture"),
        )
        .unwrap();
    }
    let fixture = "../../crates/crucible-codegen-ts/tests/fixtures/streaming.json";
    println!("cargo::rerun-if-changed={fixture}");
    for (name, codegen) in [
        (
            "streaming_client.rs",
            crucible_build::Codegen::client_from_model(fixture),
        ),
        (
            "streaming_server.rs",
            crucible_build::Codegen::from_model(fixture),
        ),
    ] {
        std::fs::write(out.join(name), codegen.render().expect("streaming fixture")).unwrap();
    }
    let fixture = "tests/fixtures/extended-runtime.json";
    println!("cargo::rerun-if-changed={fixture}");
    for (name, codegen) in [
        (
            "types_client.rs",
            crucible_build::Codegen::client_from_model(fixture),
        ),
        (
            "types_server.rs",
            crucible_build::Codegen::from_model(fixture),
        ),
    ] {
        std::fs::write(
            out.join(name),
            codegen.render().expect("extended types fixture"),
        )
        .unwrap();
    }
    for name in ["header", "query"] {
        let fixture = format!("../../crates/crucible-codegen-ts/tests/fixtures/auth-{name}.json");
        println!("cargo::rerun-if-changed={fixture}");
        let code = crucible_build::Codegen::client_from_model(&fixture)
            .render()
            .expect("auth fixture");
        std::fs::write(out.join(format!("auth_{name}_client.rs")), code).unwrap();
    }
}
