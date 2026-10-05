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
    wire_extensions(&out);
}

/// Fixtures for payload kinds, media-type headers, AWS JSON protocols and
/// HTTP checksums. Each protocol variant keeps one protocol trait, as
/// `--protocol` selects one.
fn wire_extensions(out: &std::path::Path) {
    let generate = |fixture: &str, name: &str| {
        println!("cargo::rerun-if-changed={fixture}");
        let code = crucible_build::Codegen::client_from_model(fixture)
            .render()
            .unwrap_or_else(|error| panic!("{fixture}: {error}"));
        std::fs::write(out.join(name), code).unwrap();
    };
    generate(
        "../../crates/crucible-smithy/tests/fixtures/error-payloads.json",
        "wire_client.rs",
    );
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
        crucible_build::Codegen::client_from_model(&path)
            .render()
            .expect("streaming checksum fixture"),
    )
    .unwrap();
    let protocols = "../../crates/crucible-smithy/tests/fixtures/protocol-json.json";
    println!("cargo::rerun-if-changed={protocols}");
    for protocol in ["restJson1", "awsJson1_0", "awsJson1_1"] {
        let mut model: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(protocols).unwrap()).unwrap();
        let service = &mut model["shapes"]["example.json#JsonService"];
        service["traits"] = serde_json::json!({ format!("aws.protocols#{protocol}"): {} });
        service["rename"] = serde_json::json!({"example.json#Problem": "ProblemFault"});
        let path = out.join(format!("protocol-{protocol}.json"));
        std::fs::write(&path, model.to_string()).unwrap();
        std::fs::write(
            out.join(format!("protocol_{protocol}_client.rs")),
            crucible_build::Codegen::client_from_model(&path)
                .render()
                .unwrap_or_else(|error| panic!("{protocol}: {error}")),
        )
        .unwrap();
    }
}
