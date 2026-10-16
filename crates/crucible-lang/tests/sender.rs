//! Sender-signed operations (decision 81): `@sender` lowers to
//! `Operation::sender`, what the compiler refuses of it, and the client
//! view that leaves such operations out.

use std::path::{Path, PathBuf};

use crucible_ir::ir::Sender;
use crucible_ir::{ShapeId, client_view};
use crucible_lang::load_service;

fn model(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
}

fn id(name: &str) -> ShapeId {
    ShapeId::new("example.hooks", name)
}

#[test]
fn sender_lowers_to_a_typed_field_beside_public() {
    let service = load_service(&model("sender-signed")).unwrap();
    let operation = |name: &str| {
        service
            .operations
            .iter()
            .find(|op| op.name() == name)
            .unwrap()
    };
    let payment = operation("RecordPaymentEvent");
    assert_eq!(
        payment.sender,
        Some(Sender {
            name: "Payments Inc.".into(),
            signature: "signature".into(),
        })
    );
    assert!(payment.public && payment.auth.is_empty());
    assert_eq!(payment.signature_header(), Some("Payment-Signature"));
    // An optional header member carries a signature too.
    let keys = operation("ReportLeakedKeys");
    assert_eq!(keys.sender.as_ref().unwrap().name, "CodeHost");
    assert_eq!(keys.signature_header(), Some("Scanner-Signature"));
    let account = operation("GetAccount");
    assert_eq!(account.sender, None);
    assert_eq!(account.signature_header(), None);
}

#[test]
fn misused_sender_is_refused_at_each_use() {
    let refused = load_service(&model("sender-signed-refusals")).unwrap_err();
    for (position, message) in [
        (
            "refused.cru:8:1:",
            "sender.public: NotPublic is called by Stripe, which holds no credential of this service: declare it `public`, and let its signature authenticate it",
        ),
        (
            "refused.cru:17:1:",
            "sender.signature: `signature: signature` on BodySignature names no input member bound with `@header`",
        ),
        (
            "refused.cru:25:1:",
            "sender.signature: `signature: missing` on NoSuchMember names no input member bound with `@header`",
        ),
        (
            "refused.cru:34:1:",
            "sender.name: `@sender` on Unnamed names no sender",
        ),
        (
            "refused.cru:43:20:",
            "`@sender` takes who sends the request and the input member that carries its signature",
        ),
        (
            "refused.cru:52:1:",
            "`@sender` applies to an operation, not to a structure",
        ),
    ] {
        assert!(
            refused
                .lines()
                .any(|line| line.contains(position) && line.contains(message)),
            "{position} {message}:\n{refused}"
        );
    }
}

#[test]
fn a_metadata_key_cannot_take_the_name_sender() {
    let package = crucible_lang::Package {
        namespace: "example.meta".into(),
        files: vec![crucible_lang::SourceFile {
            path: "meta.cru".into(),
            text: "metadata sender on operation: string\n".into(),
        }],
        naming: Default::default(),
        imports: Vec::new(),
        dependencies: Vec::new(),
        dir: None,
        baseline: None,
    };
    let compiled = crucible_lang::compile(&package, None);
    assert!(
        compiled
            .render()
            .contains("`@sender` is a built-in annotation"),
        "{}",
        compiled.render()
    );
}

/// What clients call: the sender-signed operations go, with the shapes only
/// they reached and the doc links to them; the rest of the service stays.
#[test]
fn the_client_view_leaves_sender_signed_operations_out() {
    let service = load_service(&model("sender-signed")).unwrap();
    let view = client_view(&service);
    let names: Vec<&str> = view.operations.iter().map(|op| op.name()).collect();
    assert_eq!(names, ["GetAccount"]);
    for gone in [
        "RecordPaymentEventInput",
        "RecordPaymentEventOutput",
        "PaymentSignature",
        "PaymentEventBody",
        "ReportLeakedKeysInput",
        "LeakedKeys",
        "LeakedKey",
    ] {
        assert!(service.shapes.contains_key(&id(gone)), "{gone}");
        assert!(!view.shapes.contains_key(&id(gone)), "{gone}");
    }
    for kept in [
        "GetAccountInput",
        "GetAccountOutput",
        "AccountId",
        "AccessDenied",
    ] {
        assert!(view.shapes.contains_key(&id(kept)), "{kept}");
    }
    assert!(service.doc_link("RecordPaymentEvent").is_some());
    assert!(view.doc_link("RecordPaymentEvent").is_none());
    // A service without sender-signed operations is its own client view.
    let plain = load_service(&model("service-docs-weather")).unwrap();
    assert_eq!(format!("{:?}", client_view(&plain)), format!("{plain:?}"));
}
