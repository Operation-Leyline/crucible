fn main() {
    crucible_build::model("weather_model").client().generate();
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let render = |fixture: &str, name: &str, side: Side| {
        println!("cargo::rerun-if-changed={fixture}");
        let model = crucible_build::model(fixture);
        let code = match side {
            Side::Client => model.client().render(),
            Side::Server => model.server().render(),
        }
        .unwrap_or_else(|error| panic!("{fixture}: {error}"));
        std::fs::write(out.join(name), code).unwrap();
    };
    render(
        "tests/fixtures/presence.json",
        "presence_client.rs",
        Side::Client,
    );
    render(
        "../../crates/crucible-smithy/tests/fixtures/review-hardening.json",
        "hardening_client.rs",
        Side::Client,
    );
    render(
        "tests/fixtures/extended-runtime.json",
        "types_client.rs",
        Side::Client,
    );
    render(
        "tests/fixtures/extended-runtime.json",
        "types_server.rs",
        Side::Server,
    );
    render(
        "tests/fixtures/payload-kinds.json",
        "kinds_client.rs",
        Side::Client,
    );
    let fixture = "../../crates/crucible-smithy/tests/fixtures/error-payloads.json";
    render(fixture, "wire_server.rs", Side::Server);
    let mut model: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(fixture).unwrap()).unwrap();
    // Preserve the shared wire contract while disambiguating the flat Rust API.
    model["shapes"]["example.wire#Wire"]["rename"] =
        serde_json::json!({"example.wire#TextError": "TextFault"});
    let path = out.join("wire-client.json");
    std::fs::write(&path, model.to_string()).unwrap();
    std::fs::write(
        out.join("wire_client.rs"),
        crucible_build::model(&path).client().render().unwrap(),
    )
    .unwrap();
    let fixture = "../../crates/crucible-smithy/tests/fixtures/internal.json";
    for (file, audience) in [
        ("audience_external.rs", crucible_build::Audience::External),
        ("audience_internal.rs", crucible_build::Audience::Internal),
    ] {
        println!("cargo::rerun-if-changed={fixture}");
        std::fs::write(
            out.join(file),
            crucible_build::model(fixture)
                .client()
                .audience(audience)
                .render()
                .unwrap(),
        )
        .unwrap();
    }
}

#[derive(Clone, Copy)]
enum Side {
    Client,
    Server,
}
