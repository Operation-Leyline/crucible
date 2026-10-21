//! The typed IR facts the language produces, which generators read instead of
//! raw traits (decisions 43, 61, 63 and 34a).

use crucible_ir::ShapeId;
use crucible_ir::ir::{Mutability, Service, Shape, Structure};
use crucible_lang::{Package, SourceFile, compile};

const MODEL: &str = r#"
metadata tier on operation: string
metadata review on operation: string retention source

service Vault {
    version "1"
    protocol json
    auth bearer
    operations [PutSecret]
}

/// A secret value: never logged.
sensitive string Secret { length 1..64 }

@tier("write")
@review("security team")
idempotent operation PutSecret {
    http PUT "/secrets/{name}"

    strict input {
        name: string
        @token
        clientToken?: string
        @mutability(createAndRead)
        value: Secret
    }

    output {
        version: long
    }
}
"#;

fn service() -> (Service, crucible_lang::Compiled, Package) {
    let package = Package {
        namespace: "example.vault".to_owned(),
        files: vec![SourceFile {
            path: "vault.cru".to_owned(),
            text: MODEL.to_owned(),
        }],
        naming: Default::default(),
        imports: Vec::new(),
        dependencies: Vec::new(),
        dir: None,
        baseline: None,
        exports: Vec::new(),
    };
    let compiled = compile(&package, None);
    assert!(compiled.diagnostics.is_empty(), "\n{}", compiled.render());
    let service = compiled.service.clone().expect("service");
    (service, compiled, package)
}

fn structure<'a>(service: &'a Service, name: &str) -> &'a Structure {
    match service.shape(&ShapeId::new("example.vault", name)) {
        Some(Shape::Structure(s)) => s,
        other => panic!("{name}: {other:?}"),
    }
}

#[test]
fn inputs_are_typed_as_inputs_and_strict() {
    let (service, ..) = service();
    let input = structure(&service, "PutSecretInput");
    assert!(input.input && input.strict);
    let output = structure(&service, "PutSecretOutput");
    assert!(!output.input && !output.strict);
}

#[test]
fn member_facts_are_typed() {
    let (service, ..) = service();
    let input = structure(&service, "PutSecretInput");
    let token = input.member("clientToken").expect("clientToken");
    assert!(token.idempotency_token);
    let value = input.member("value").expect("value");
    assert_eq!(value.mutability, Some(Mutability::CreateAndRead));
    assert!(service.sensitive(value), "sensitive through its target");
    assert!(!service.sensitive(input.member("name").expect("name")));
}

#[test]
fn raw_traits_carry_project_metadata_only() {
    let (service, ..) = service();
    let operation = &service.operations[0];
    let keys: Vec<String> = operation.traits.keys().map(ToString::to_string).collect();
    // `review` is source-only, so the runtime never sees it.
    assert_eq!(keys, ["example.vault#tier"]);
    assert!(structure(&service, "PutSecretInput").traits.is_empty());
    assert!(service.traits.is_empty());
}

#[test]
fn the_source_map_locates_ir_ids() {
    let (_, compiled, package) = service();
    let text = &package.files[0].text;
    let at = |id: &str| {
        let location = compiled.source_map[&id.parse::<ShapeId>().expect("id")];
        assert_eq!(location.file, 0);
        location.span.text(text).to_owned()
    };
    assert!(at("example.vault#PutSecret").starts_with("@tier"));
    assert!(at("example.vault#Secret").starts_with("sensitive string Secret"));
    assert!(at("example.vault#PutSecretInput").starts_with("strict input"));
    assert!(at("example.vault#PutSecretInput$value").starts_with("@mutability"));
}

#[test]
fn bad_mutability_and_retention_are_reported() {
    let package = Package {
        namespace: "example.vault".to_owned(),
        files: vec![SourceFile {
            path: "vault.cru".to_owned(),
            text: MODEL
                .replace("createAndRead", "sometimes")
                .replace("retention source", "retention forever"),
        }],
        naming: Default::default(),
        imports: Vec::new(),
        dependencies: Vec::new(),
        dir: None,
        baseline: None,
        exports: Vec::new(),
    };
    let compiled = compile(&package, None);
    let messages: Vec<&str> = compiled
        .diagnostics
        .iter()
        .map(|d| d.diagnostic.message.as_str())
        .collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("`@mutability` is one of")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("retention is `source` or `runtime`")),
        "{messages:?}"
    );
}
