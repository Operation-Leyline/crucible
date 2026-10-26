use std::path::Path;

use crucible_ir::{ir::Operation, snapshot};
use crucible_lang::load_service;

fn service() -> crucible_ir::ir::Service {
    load_service(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/operation-intent"))
        .unwrap()
}

fn operation<'a>(service: &'a crucible_ir::ir::Service, name: &str) -> &'a Operation {
    service
        .operations
        .iter()
        .find(|op| op.name() == name)
        .unwrap()
}

#[test]
fn lowering_extracts_intent_and_keeps_all_original_documentation() {
    let service = service();
    let get = operation(&service, "GetItem");
    let intent = get.intent.as_ref().unwrap();
    assert_eq!(
        intent.use_when.as_deref(),
        Some("You know the item's **identifier**.")
    );
    assert_eq!(
        intent.dont_use_when.as_deref(),
        Some("You need several items; use [ListItems].")
    );
    assert!(intent.is_complete());
    assert_eq!(
        get.docs.as_deref(),
        Some(
            "Reads one item.\n\n## Use when\n\nYou know the item's **identifier**.\n\n## Don't use when\n\nYou need several items; use [ListItems].\n\n## Details\n\nReturns the current value."
        )
    );
    assert!(
        operation(&service, "ListItems")
            .intent
            .as_ref()
            .unwrap()
            .use_when
            .as_ref()
            .unwrap()
            .contains("### Ordering")
    );
    assert!(
        !operation(&service, "Partial")
            .intent
            .as_ref()
            .unwrap()
            .is_complete()
    );
    assert!(operation(&service, "Fenced").intent.is_none());
    assert!(operation(&service, "Missing").intent.is_none());
}

#[test]
fn release_snapshots_round_trip_intent_and_accept_older_missing_fields() {
    let service = service();
    let text = snapshot::write(&service, "test");
    let restored = snapshot::read(&text).unwrap();
    assert_eq!(
        operation(&restored, "GetItem").intent,
        operation(&service, "GetItem").intent
    );
    assert_eq!(snapshot::write(&restored, "test"), text);

    let mut old: serde_json::Value = serde_json::from_str(&text).unwrap();
    for op in old["service"]["operations"].as_array_mut().unwrap() {
        if op["id"] == "example.intent#Missing" {
            assert!(op.get("intent").is_none());
        }
        op.as_object_mut().unwrap().remove("intent");
    }
    let older = snapshot::read(&old.to_string()).unwrap();
    assert!(older.operations.iter().all(|op| op.intent.is_none()));
    let contract = snapshot::read(&snapshot::write_contract(&service, "test")).unwrap();
    assert!(
        contract
            .operations
            .iter()
            .all(|op| op.intent.is_none() && op.docs.is_none())
    );
}
