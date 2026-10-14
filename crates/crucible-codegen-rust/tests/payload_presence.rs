#[path = "../../../tests/support/cargo_target.rs"]
mod cargo_target;

use std::{fs, path::Path, process::Command};

use crucible_codegen_rust::{Options, generate, generate_client};
use crucible_smithy::Model;
use serde_json::json;

#[test]
fn generated_document_payloads_preserve_presence_and_structure_omission() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir_all(directory.path().join("src")).unwrap();
    let mut source = String::new();
    for (module, protocol) in [
        ("crucible", "crucible.protocols#json"),
        ("rest_json", "aws.protocols#restJson1"),
    ] {
        let model = json!({
            "smithy": "2.0",
            "shapes": {
                "test.payloads#Payloads": {"type": "service", "version": "1", "traits": {protocol: {}}, "operations": [{"target": "test.payloads#Document"}, {"target": "test.payloads#Structure"}, {"target": "test.payloads#Required"}]},
                "test.payloads#Document": {"type": "operation", "traits": {"smithy.api#http": {"method": "POST", "uri": "/document", "code": 200}}, "input": {"target": "test.payloads#DocumentInput"}, "output": {"target": "test.payloads#DocumentOutput"}},
                "test.payloads#Structure": {"type": "operation", "traits": {"smithy.api#http": {"method": "POST", "uri": "/structure", "code": 200}}, "input": {"target": "test.payloads#StructureInput"}, "output": {"target": "test.payloads#StructureOutput"}},
                "test.payloads#DocumentInput": {"type": "structure", "members": {"payload": {"target": "smithy.api#Document", "traits": {"smithy.api#httpPayload": {}}}}},
                "test.payloads#DocumentOutput": {"type": "structure", "members": {"payload": {"target": "smithy.api#Document", "traits": {"smithy.api#httpPayload": {}}}}},
                "test.payloads#StructureInput": {"type": "structure", "members": {"payload": {"target": "test.payloads#Record", "traits": {"smithy.api#httpPayload": {}}}}},
                "test.payloads#StructureOutput": {"type": "structure", "members": {"payload": {"target": "test.payloads#Record", "traits": {"smithy.api#httpPayload": {}}}}},
                "test.payloads#Required": {"type": "operation", "traits": {"smithy.api#http": {"method": "POST", "uri": "/required", "code": 200}}, "input": {"target": "test.payloads#RequiredInput"}},
                "test.payloads#RequiredInput": {"type": "structure", "members": {"payload": {"target": "test.payloads#Record", "traits": {"smithy.api#httpPayload": {}, "smithy.api#required": {}}}}},
                "test.payloads#Record": {"type": "structure", "members": {"name": {"target": "smithy.api#String"}}}
            }
        });
        let service = Model::from_json(&model.to_string())
            .unwrap()
            .lower_with_capabilities(None, &crucible_codegen_rust::client_capabilities())
            .unwrap();
        source.push_str(&format!(
            "mod {module} {{\n{}\nmod sdk {{\n{}\n}}\n{}\n}}\n",
            generate(&service, &Options::default()).unwrap(),
            generate_client(&service).unwrap(),
            CONSUMER,
        ));
    }
    fs::write(directory.path().join("src/lib.rs"), source).unwrap();
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    fs::write(
        directory.path().join("Cargo.toml"),
        format!(
            "[package]\nname = 'payload-presence-consumer'\nversion = '0.0.0'\nedition = '2024'\n[dependencies]\nserde_json = '1'\ncrucible-wire = {{ path = {:?} }}\ncrucible-server-runtime-rust = {{ path = {:?}, default-features = false }}\ncrucible-client-runtime-rust = {{ path = {:?} }}\n[workspace]\n",
            crates.join("crucible-wire"),
            crates.join("crucible-server-runtime-rust"),
            crates.join("crucible-client-runtime-rust"),
        ),
    )
    .unwrap();
    let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args(["test", "--manifest-path"])
        .arg(directory.path().join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", cargo_target::target_dir())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

const CONSUMER: &str = r##"
#[cfg(test)]
mod tests {
    use super::{payloads, sdk};
    use crucible_wire::{field::Field, operation::Operation, split};
    use serde_json::{Value, json};

    #[test]
    fn client_document_payload_distinguishes_absent_null_and_value() {
        for (payload, expected) in [
            (Field::Absent, b"".as_slice()),
            (Field::Null, b"null".as_slice()),
            (Field::Value(Value::Null), b"null".as_slice()),
            (Field::Value(json!({"name": "present"})), br#"{"name":"present"}"#.as_slice()),
        ] {
            let input = sdk::DocumentInput { payload };
            let mut body = Vec::new();
            split::write_bound(&input, sdk::Document::INFO.input, &mut body).unwrap();
            assert_eq!(body, expected);
        }
    }

    #[test]
    fn server_document_payload_distinguishes_none_some_null_and_value() {
        for (payload, expected) in [
            (None, b"".as_slice()),
            (Some(Value::Null), b"null".as_slice()),
            (Some(json!({"name": "present"})), br#"{"name":"present"}"#.as_slice()),
        ] {
            let output = payloads::DocumentOutput { payload };
            let mut body = Vec::new();
            split::write_http(&output, payloads::Document::INFO.output, None, &mut body).unwrap();
            assert_eq!(body, expected);
            let mut body = Vec::new();
            split::write_http(&serde_json::to_value(&output).unwrap(), payloads::Document::INFO.output, None, &mut body).unwrap();
            assert_eq!(body, expected);
        }
    }

    #[test]
    fn nullable_structure_payload_omission_is_preserved() {
        let input = sdk::StructureInput { payload: Field::Absent };
        let mut body = Vec::new();
        split::write_bound(&input, sdk::Structure::INFO.input, &mut body).unwrap();
        assert!(body.is_empty());
        let output = payloads::StructureOutput { payload: None };
        let mut body = Vec::new();
        split::write_http(&output, payloads::Structure::INFO.output, None, &mut body).unwrap();
        assert!(body.is_empty());
        let input = sdk::StructureInput { payload: Field::Null };
        let mut body = Vec::new();
        split::write_bound(&input, sdk::Structure::INFO.input, &mut body).unwrap();
        let expected = if sdk::Structure::PROTOCOL == crucible_wire::operation::Protocol::AwsRestJson1 {
            b"".as_slice()
        } else {
            b"null".as_slice()
        };
        assert_eq!(body, expected);
    }

    // A JSON payload of `null` reads as the member's explicit null, as it
    // would in a JSON body (#274): what Crucible JSON clients send for an
    // explicitly null structure payload.
    #[test]
    fn null_json_payloads_read_as_the_members_null() {
        use crucible_wire::bound::{BoundValue, read_http};
        let aws = sdk::Structure::PROTOCOL == crucible_wire::operation::Protocol::AwsRestJson1;
        let null = || vec![("payload", BoundValue::RawJson(b"null"))];
        let input: payloads::StructureInput =
            read_http(b"", null(), payloads::Structure::INFO.input).unwrap();
        assert!(input.payload.is_none());
        let output: sdk::StructureOutput =
            read_http(b"", null(), sdk::Structure::INFO.output).unwrap();
        // AWS REST JSON reads a null structure as absence.
        assert_eq!(output.payload.is_null(), !aws);
        assert!(output.payload.is_unset());
        // Documents follow the same body rule: Crucible JSON servers read a
        // null document member as absent, AWS REST JSON servers keep it.
        let input: payloads::DocumentInput =
            read_http(b"", null(), payloads::Document::INFO.input).unwrap();
        assert_eq!(input.payload, aws.then_some(Value::Null));
        let output: sdk::DocumentOutput =
            read_http(b"", null(), sdk::Document::INFO.output).unwrap();
        assert!(output.payload.is_null());
        let record = vec![("payload", BoundValue::RawJson(br#"{"name":"present"}"#))];
        let input: payloads::StructureInput =
            read_http(b"", record, payloads::Structure::INFO.input).unwrap();
        assert_eq!(input.payload.unwrap().name.as_deref(), Some("present"));
        // A structure payload is a JSON object, never an array of
        // positional values (#292), on the server and in the client.
        for json in [&b"[]"[..], br#"["present"]"#] {
            let array = || vec![("payload", BoundValue::RawJson(json))];
            assert!(read_http::<payloads::StructureInput>(b"", array(), payloads::Structure::INFO.input).is_err());
            assert!(read_http::<sdk::StructureOutput>(b"", array(), sdk::Structure::INFO.output).is_err());
            assert!(read_http::<payloads::RequiredInput>(b"", array(), payloads::Required::INFO.input).is_err());
        }
        // A required structure payload has no null: `null` is still refused.
        assert!(
            read_http::<payloads::RequiredInput>(b"", null(), payloads::Required::INFO.input)
                .is_err()
        );
        let record = vec![("payload", BoundValue::RawJson(br#"{"name":"present"}"#))];
        let input: payloads::RequiredInput =
            read_http(b"", record, payloads::Required::INFO.input).unwrap();
        assert_eq!(input.payload.name.as_deref(), Some("present"));
    }
}
"##;
