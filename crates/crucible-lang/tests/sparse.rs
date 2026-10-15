//! `sparse list` and `sparse map`: the elements may be null. The IR and the
//! runtimes carry the flag; this is the language side.

use crucible_ir::ir::{Service, Shape};
use crucible_lang::{Compiled, FormatOptions, Package, SourceFile, compile, format};

fn model(row: &str) -> String {
    format!(
        r#"
service Grid {{
    version "1"
    protocol json
    operations [Run]
}}

string Name {{ length 1..20 }}
document Cell
{row}
list Rows: Row {{ length ..5 }}
sparse map Params: Name -> Cell

readonly operation Run {{
    http GET "/run"
    output {{
        rows: Rows
        params: Params
    }}
}}
"#
    )
}

fn compiled(text: &str) -> Compiled {
    compile(
        &Package::new(
            "example.grid",
            vec![SourceFile {
                path: "grid.cru".to_owned(),
                text: text.to_owned(),
            }],
        ),
        None,
    )
}

fn service(text: &str) -> Service {
    let compiled = compiled(text);
    assert!(!compiled.has_errors(), "{}", compiled.render());
    compiled.service.expect("service")
}

fn sparse(service: &Service, name: &str) -> bool {
    match service.shapes.iter().find(|(id, _)| id.name() == name) {
        Some((_, Shape::List(list))) => list.sparse,
        Some((_, Shape::Map(map))) => map.sparse,
        _ => panic!("no collection {name}"),
    }
}

#[test]
fn a_sparse_list_and_map_lower_as_sparse_and_the_rest_stay_dense() {
    let service = service(&model("sparse list Row: Cell { length ..10 }"));
    assert!(sparse(&service, "Row"));
    assert!(sparse(&service, "Params"));
    assert!(!sparse(&service, "Rows"));
    let dense = self::service(&model("list Row: Cell { length ..10 }"));
    assert!(!sparse(&dense, "Row"));
}

#[test]
fn sparse_applies_to_lists_and_maps_only() {
    let compiled = compiled(&format!(
        "{}\nsparse structure Bad {{ a?: Name }}\nsparse string Worse {{ length 1..2 }}\n",
        model("list Row: Cell { length ..10 }")
    ));
    let messages: Vec<_> = compiled
        .diagnostics
        .iter()
        .map(|d| d.diagnostic.message.clone())
        .collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("`sparse` does not apply to a structure")),
        "{messages:#?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("`sparse` does not apply to a scalar")
                || m.contains("`sparse` does not apply to a string")),
        "{messages:#?}"
    );
}

#[test]
fn sparse_combines_with_the_other_modifiers_and_formats() {
    let source = "sensitive sparse list Row: Cell {\n    length ..10\n}\n";
    let formatted = format(source, &FormatOptions::default());
    assert_eq!(formatted, source);
}

#[test]
fn making_a_collection_sparse_or_dense_is_a_compatibility_finding() {
    let dense = service(&model("list Row: Cell { length ..10 }"));
    let sparse = service(&model("sparse list Row: Cell { length ..10 }"));
    let config = crucible_policy::Config::default();
    // Clients built for dense rows break on a null element in a response, and
    // a sparse list's generated type (`Option` elements) differs for SDKs.
    let widened = crucible_policy::diff(&dense, &sparse, &config);
    assert!(
        widened
            .findings
            .iter()
            .any(|f| f.rule == "compatibility.nullability" || f.rule == "compatibility.sdk"),
        "{:#?}",
        widened.findings
    );
    let narrowed = crucible_policy::diff(&sparse, &dense, &config);
    assert!(!narrowed.findings.is_empty());
}

#[test]
fn a_sparse_collection_requires_the_sparse_capability() {
    let sparse = service(&model("sparse list Row: Cell { length ..10 }"));
    assert!(
        sparse
            .requirements
            .iter()
            .any(|r| r.feature == crucible_ir::Feature::Sparse)
    );
    let dense = service(&model("list Row: Cell { length ..10 }").replace("sparse map", "map"));
    assert!(
        dense
            .requirements
            .iter()
            .all(|r| r.feature != crucible_ir::Feature::Sparse)
    );
}
