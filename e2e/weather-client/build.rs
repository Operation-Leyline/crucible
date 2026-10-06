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
    let fixture = "../../crates/crucible-codegen-rust/tests/fixtures/keyword-waiters.json";
    println!("cargo::rerun-if-changed={fixture}");
    std::fs::write(
        out.join("keyword_client.rs"),
        crucible_build::Codegen::client_from_model(fixture)
            .render()
            .expect("keyword waiter fixture"),
    )
    .unwrap();
    let fixture = "../../crates/crucible-smithy/tests/fixtures/error-payloads.json";
    std::fs::write(
        out.join("wire_server.rs"),
        crucible_build::Codegen::from_model(fixture)
            .render()
            .unwrap(),
    )
    .unwrap();
    let fixture = "../../crates/crucible-codegen-ts/tests/fixtures/auth-header.json";
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
        crucible_build::Codegen::from_model(&path).render().unwrap(),
    )
    .unwrap();
    wire_extensions(&out);
    event_streams(&out);
}

/// The event stream fixture under each protocol shape: HTTP bindings
/// (restJson1), an initial message (awsJson1_0), and SigV4-signed events.
fn event_streams(out: &std::path::Path) {
    let fixture = "../../crates/crucible-smithy/tests/fixtures/event-streams.json";
    println!("cargo::rerun-if-changed={fixture}");
    let base: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(fixture).unwrap()).unwrap();
    for (name, traits) in [
        (
            "events_rest_client.rs",
            serde_json::json!({"aws.protocols#restJson1": {}}),
        ),
        (
            "events_rpc_client.rs",
            serde_json::json!({"aws.protocols#awsJson1_0": {}}),
        ),
        (
            "events_signed_client.rs",
            serde_json::json!({"aws.protocols#restJson1": {}, "aws.auth#sigv4": {"name": "events"}, "smithy.api#auth": ["aws.auth#sigv4"]}),
        ),
    ] {
        let mut model = base.clone();
        model["shapes"]["example.events#Events"]["traits"] = traits;
        let path = out.join(name.replace(".rs", ".json"));
        std::fs::write(&path, model.to_string()).unwrap();
        std::fs::write(
            out.join(name),
            crucible_build::Codegen::client_from_model(&path)
                .render()
                .unwrap_or_else(|error| panic!("{name}: {error}")),
        )
        .unwrap();
    }
    for protocol in [
        "restJson1",
        "awsJson1_0",
        "awsJson1_1",
        "rpcv2Json",
        "rpcv2Cbor",
    ] {
        let mut model = base.clone();
        let namespace = if protocol.starts_with("rpcv2") {
            "smithy.protocols"
        } else {
            "aws.protocols"
        };
        model["shapes"]["example.events#Events"]["traits"] =
            serde_json::json!({format!("{namespace}#{protocol}"): {}});
        model["shapes"]["example.events#Events"]["operations"]
            .as_array_mut()
            .unwrap()
            .retain(|op| op["target"] != "example.events#SubscribeChecksums");
        if protocol == "rpcv2Cbor" {
            model["shapes"]["example.events#StructuredEvent"]["members"]["stamp"] =
                serde_json::json!({"target":"smithy.api#Timestamp"});
            model["shapes"]["example.events#StructuredEvent"]["members"]["blob"] =
                serde_json::json!({"target":"smithy.api#Blob"});
            model["shapes"]["example.events#StructuredEvent"]["members"]["big"] =
                serde_json::json!({"target":"smithy.api#BigInteger"});
            model["shapes"]["example.events#StructuredEvent"]["members"]["number"] =
                serde_json::json!({"target":"smithy.api#Double"});
        }
        let path = out.join(format!("events_{protocol}.json"));
        std::fs::write(&path, model.to_string()).unwrap();
        for (target, codegen) in [
            ("client", crucible_build::Codegen::client_from_model(&path)),
            ("server", crucible_build::Codegen::from_model(&path)),
        ] {
            std::fs::write(
                out.join(format!("events_{protocol}_{target}.rs")),
                codegen
                    .render()
                    .unwrap_or_else(|error| panic!("{protocol}/{target}: {error}")),
            )
            .unwrap();
        }
    }
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
    for protocol in [
        "restJson1",
        "awsJson1_0",
        "awsJson1_1",
        "rpcv2Json",
        "rpcv2Cbor",
    ] {
        let mut model: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(protocols).unwrap()).unwrap();
        let service = &mut model["shapes"]["example.json#JsonService"];
        let namespace = if protocol.starts_with("rpcv2") {
            "smithy.protocols"
        } else {
            "aws.protocols"
        };
        service["traits"] = serde_json::json!({ format!("{namespace}#{protocol}"): {} });
        if !protocol.starts_with("rpcv2") {
            service["rename"] = serde_json::json!({"example.json#Problem": "ProblemFault"});
        }
        if protocol == "rpcv2Cbor" {
            for shape in ["ValuesInput", "ValuesOutput"] {
                model["shapes"][format!("example.json#{shape}")]["members"]
                    .as_object_mut()
                    .unwrap()
                    .remove("document");
            }
        }
        if protocol.starts_with("rpcv2") {
            for shape in ["ValuesInput", "ValuesOutput"] {
                model["shapes"][format!("example.json#{shape}")]["members"]["blob"] =
                    serde_json::json!({"target":"smithy.api#Blob"});
                model["shapes"][format!("example.json#{shape}")]["members"]["large"] =
                    serde_json::json!({"target":"smithy.api#BigInteger"});
                model["shapes"][format!("example.json#{shape}")]["members"]["decimal"] =
                    serde_json::json!({"target":"smithy.api#BigDecimal"});
            }
        }
        let path = out.join(format!("protocol-{protocol}.json"));
        std::fs::write(&path, model.to_string()).unwrap();
        std::fs::write(
            out.join(format!("protocol_{protocol}_client.rs")),
            crucible_build::Codegen::client_from_model(&path)
                .render()
                .unwrap_or_else(|error| panic!("{protocol}: {error}")),
        )
        .unwrap();
        std::fs::write(
            out.join(format!("protocol_{protocol}_server.rs")),
            crucible_build::Codegen::from_model(&path)
                .render()
                .unwrap_or_else(|error| panic!("{protocol} server: {error}")),
        )
        .unwrap();
    }
}
