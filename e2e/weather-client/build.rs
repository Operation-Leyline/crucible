fn main() {
    crucible_build::model("weather_model").client().generate();
    println!("cargo::rerun-if-changed=tests/fixtures/presence.json");
    let code = crucible_build::model("tests/fixtures/presence.json")
        .client()
        .render()
        .expect("client presence fixture");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let fixture = "tests/fixtures/endpoints.json";
    println!("cargo::rerun-if-changed={fixture}");
    std::fs::write(
        out.join("endpoint_client.rs"),
        crucible_build::model(fixture)
            .client()
            .render()
            .expect("endpoint and waiter fixture"),
    )
    .unwrap();
    let fixture = "../../crates/crucible-codegen-ts/tests/fixtures/sigv4.json";
    println!("cargo::rerun-if-changed={fixture}");
    std::fs::write(
        out.join("sigv4_client.rs"),
        crucible_build::model(fixture)
            .client()
            .render()
            .expect("SigV4 fixture"),
    )
    .unwrap();
    std::fs::write(out.join("presence_client.rs"), code).unwrap();
    let fixture = "../../crates/crucible-smithy/tests/fixtures/review-hardening.json";
    println!("cargo::rerun-if-changed={fixture}");
    let code = crucible_build::model(fixture)
        .client()
        .render()
        .expect("hardening fixture");
    std::fs::write(out.join("hardening_client.rs"), code).unwrap();
    let fixture = "../../crates/crucible-codegen-ts/tests/fixtures/dynamic-bindings.json";
    println!("cargo::rerun-if-changed={fixture}");
    for (name, codegen) in [
        (
            "dynamic_client.rs",
            crucible_build::model(fixture).client().render(),
        ),
        (
            "dynamic_server.rs",
            crucible_build::model(fixture).server().render(),
        ),
    ] {
        std::fs::write(out.join(name), codegen.expect("dynamic bindings fixture")).unwrap();
    }
    let fixture = "../../crates/crucible-codegen-ts/tests/fixtures/streaming.json";
    println!("cargo::rerun-if-changed={fixture}");
    for (name, codegen) in [
        (
            "streaming_client.rs",
            crucible_build::model(fixture).client().render(),
        ),
        (
            "streaming_server.rs",
            crucible_build::model(fixture).server().render(),
        ),
    ] {
        std::fs::write(out.join(name), codegen.expect("streaming fixture")).unwrap();
    }
    let fixture = "tests/fixtures/extended-runtime.json";
    println!("cargo::rerun-if-changed={fixture}");
    for (name, codegen) in [
        (
            "types_client.rs",
            crucible_build::model(fixture).client().render(),
        ),
        (
            "types_server.rs",
            crucible_build::model(fixture).server().render(),
        ),
    ] {
        std::fs::write(out.join(name), codegen.expect("extended types fixture")).unwrap();
    }
    for name in ["header", "query"] {
        let fixture = format!("../../crates/crucible-codegen-ts/tests/fixtures/auth-{name}.json");
        println!("cargo::rerun-if-changed={fixture}");
        let code = crucible_build::model(&fixture)
            .client()
            .render()
            .expect("auth fixture");
        std::fs::write(out.join(format!("auth_{name}_client.rs")), code).unwrap();
    }
    let fixture = "../../crates/crucible-codegen-rust/tests/fixtures/keyword-waiters.json";
    println!("cargo::rerun-if-changed={fixture}");
    std::fs::write(
        out.join("keyword_client.rs"),
        crucible_build::model(fixture)
            .client()
            .render()
            .expect("keyword waiter fixture"),
    )
    .unwrap();
    let fixture = "../../crates/crucible-smithy/tests/fixtures/error-payloads.json";
    println!("cargo::rerun-if-changed={fixture}");
    std::fs::write(
        out.join("wire_server.rs"),
        crucible_build::model(fixture).server().render().unwrap(),
    )
    .unwrap();
    let fixture = "../../crates/crucible-codegen-ts/tests/fixtures/auth-header.json";
    println!("cargo::rerun-if-changed={fixture}");
    let mut model: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(fixture).unwrap()).unwrap();
    model["shapes"]["example.auth#AuthHeader"]["traits"]["aws.auth#sigv4"] =
        serde_json::json!({"name":"example"});
    model["shapes"]["example.auth#AuthHeader"]["traits"]["smithy.api#auth"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!("aws.auth#sigv4"));
    model["shapes"]["example.auth#Mixed"]["traits"]["aws.auth#unsignedPayload"] =
        serde_json::json!({});
    let path = out.join("auth-server.json");
    std::fs::write(&path, model.to_string()).unwrap();
    std::fs::write(
        out.join("auth_server.rs"),
        crucible_build::model(&path).server().render().unwrap(),
    )
    .unwrap();
    let fixture = "../../crates/crucible-smithy/tests/fixtures/internal.json";
    println!("cargo::rerun-if-changed={fixture}");
    for (file, audience) in [
        ("audience_external.rs", crucible_build::Audience::External),
        ("audience_internal.rs", crucible_build::Audience::Internal),
    ] {
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
    wire_extensions(&out);
    event_streams(&out);
}

/// The event stream fixture as a plain Crucible JSON service, and with
/// SigV4-signed events.
fn event_streams(out: &std::path::Path) {
    let fixture = "../../crates/crucible-smithy/tests/fixtures/event-streams.json";
    println!("cargo::rerun-if-changed={fixture}");
    let base: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(fixture).unwrap()).unwrap();
    for (name, traits) in [
        (
            "events_rest_client.rs",
            serde_json::json!({"crucible.protocols#json": {}}),
        ),
        (
            "events_signed_client.rs",
            serde_json::json!({"crucible.protocols#json": {}, "aws.auth#sigv4": {"name": "events"}, "smithy.api#auth": ["aws.auth#sigv4"]}),
        ),
    ] {
        let mut model = base.clone();
        model["shapes"]["example.events#Events"]["traits"] = traits;
        let path = out.join(name.replace(".rs", ".json"));
        std::fs::write(&path, model.to_string()).unwrap();
        std::fs::write(
            out.join(name),
            crucible_build::model(&path)
                .client()
                .render()
                .unwrap_or_else(|error| panic!("{name}: {error}")),
        )
        .unwrap();
    }
    let mut model = base.clone();
    model["shapes"]["example.events#Events"]["traits"] =
        serde_json::json!({"crucible.protocols#json": {}});
    model["shapes"]["example.events#Events"]["operations"]
        .as_array_mut()
        .unwrap()
        .retain(|op| op["target"] != "example.events#SubscribeChecksums");
    let path = out.join("events_crucible.json");
    std::fs::write(&path, model.to_string()).unwrap();
    for (target, codegen) in [
        ("client", crucible_build::model(&path).client().render()),
        ("server", crucible_build::model(&path).server().render()),
    ] {
        std::fs::write(
            out.join(format!("events_crucible_{target}.rs")),
            codegen.unwrap_or_else(|error| panic!("crucible/{target}: {error}")),
        )
        .unwrap();
    }
}

/// Fixtures for payload kinds, media-type headers and HTTP checksums.
fn wire_extensions(out: &std::path::Path) {
    let generate = |fixture: &str, name: &str| {
        println!("cargo::rerun-if-changed={fixture}");
        let code = crucible_build::model(fixture)
            .client()
            .render()
            .unwrap_or_else(|error| panic!("{fixture}: {error}"));
        std::fs::write(out.join(name), code).unwrap();
    };
    let fixture = "../../crates/crucible-smithy/tests/fixtures/error-payloads.json";
    println!("cargo::rerun-if-changed={fixture}");
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
    generate("tests/fixtures/payload-kinds.json", "kinds_client.rs");
    let checksums = "../../crates/crucible-smithy/tests/fixtures/checksums.json";
    generate(checksums, "checksum_client.rs");
    // The same operations with a streamed output, verified as it is read.
    println!("cargo::rerun-if-changed={checksums}");
    let mut model: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(checksums).unwrap()).unwrap();
    model["shapes"]["example.checksums#Stream"] =
        serde_json::json!({"type":"blob","traits":{"smithy.api#streaming":{}}});
    model["shapes"]["example.checksums#Output"]["members"]["data"]["target"] =
        serde_json::json!("example.checksums#Stream");
    model["shapes"]["example.checksums#Input"]["members"]["data"]["target"] =
        serde_json::json!("example.checksums#Stream");
    for structure in ["Input", "Output"] {
        model["shapes"][format!("example.checksums#{structure}")]["members"]["data"]["traits"]["smithy.api#required"] =
            serde_json::json!({});
    }
    let path = out.join("checksum-streaming.json");
    std::fs::write(&path, model.to_string()).unwrap();
    std::fs::write(
        out.join("checksum_streaming_client.rs"),
        crucible_build::model(&path)
            .client()
            .render()
            .expect("streaming checksum fixture"),
    )
    .unwrap();
}
