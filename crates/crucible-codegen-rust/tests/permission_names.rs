//! Spellings the permission bindings must get right on unusual models
//! (docs/permissions.md §6.2): a resource named `Self`, obligation names
//! that collide with an ordinal, and a `when` literal Rust cannot spell.

use std::fs;
use std::path::Path;

use crucible_codegen_rust::{Options, generate};
use crucible_ir::ir::{Condition, Requirements, Service};

const MANIFEST: &str = "[model]\nnamespace = \"example.names\"\nimports = [\"crucible.errors\"]\n";

/// `source`, compiled as a one-file package named `name`.
fn compile(name: &str, source: &str) -> Service {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("permission-names-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("crucible.toml"), MANIFEST).unwrap();
    fs::write(dir.join("names.cru"), source).unwrap();
    let compiled = crucible_lang::compile(&crucible_lang::load_package(&dir).unwrap(), None);
    let rendered = compiled.render();
    compiled
        .service
        .unwrap_or_else(|| panic!("the model compiles:\n{rendered}"))
}

const SERVICE: &str = r#"service Names {
    version "2026-10-01"
    protocol json
    auth bearer
    resources [RESOURCES]
    errors [ValidationException, InternalServerException]
}
"#;

/// `Self` names no enum variant: the unions spell it `Self_`, while the
/// resource's own items keep the name (`SelfPermission`, `SelfRef`).
#[test]
fn a_resource_named_self_gets_a_variant_rust_accepts() {
    let source = SERVICE.replace("RESOURCES", "Self")
        + r#"
string SelfId

resource Self {
    id selfId: SelfId
    permission read = "self:read"
}

readonly operation GetSelf reads Self {
    http GET "/selves/{selfId}"
    requires read
    input { $selfId }
    output { name: string }
}
"#;
    let code = generate(&compile("self", &source), &Options::default()).unwrap();
    assert!(code.contains("    Self_(SelfPermission),"), "{code}");
    assert!(code.contains("    Self_(SelfRef),"), "{code}");
    assert!(code.contains("Self::Self_(SelfPermission::Read)"), "{code}");
    assert!(!code.contains("    Self(SelfPermission),"), "{code}");
}

/// The ordinal a repeated obligation name gets is never a name another
/// clause spells: `write` on `OnGraph_2` would be `WRITE_ON_ON_GRAPH_2`,
/// which the second clause already took.
#[test]
fn obligation_ordinals_never_reuse_a_spelled_name() {
    let source = SERVICE.replace("RESOURCES", "Graph, OnGraph, OnGraph_2")
        + r#"
string GraphId
string OnGraphId
string OtherId

resource Graph {
    id graphId: GraphId
    permission writeOn = "graph:write-on"
}

resource OnGraph {
    id onGraphId: OnGraphId
    permission write = "on-graph:write"
}

@authorizationName("other")
resource OnGraph_2 {
    id otherId: OtherId
    permission write = "other:write"
}

operation Touch on Graph {
    http POST "/graphs/{graphId}/touch"
    requires writeOn by handler
    requires write on OnGraph by handler
    requires write on OnGraph_2 by handler
    input { $graphId }
    output { touched: boolean }
}
"#;
    let code = generate(&compile("ordinals", &source), &Options::default()).unwrap();
    for (name, index) in [
        ("WRITE_ON_ON_GRAPH", 0),
        ("WRITE_ON_ON_GRAPH_2", 1),
        ("WRITE_ON_ON_GRAPH_2_2", 2),
    ] {
        assert!(
            code.contains(&format!(
                "pub const {name}: ::crucible_server_runtime_rust::__generated::permissions::Obligation = ::crucible_server_runtime_rust::__generated::permissions::Obligation {{ operation: \"example.names#Touch\", index: {index},"
            )),
            "{name}\n{code}"
        );
    }
}

/// A float literal beyond `f32`'s range has no nearest value of the
/// member's type. The shared check refuses it in a model; an IR that skipped
/// it (a hand-built one) still gets a diagnostic here, never `1e300_f32`.
#[test]
fn a_float_literal_beyond_f32_is_a_diagnostic() {
    let source = SERVICE.replace("RESOURCES", "Graph")
        + r#"
string GraphId

resource Graph {
    id graphId: GraphId
    permission read = "graph:read"
}

readonly operation Measure on Graph {
    http POST "/graphs/{graphId}/measure"
    requires read when ratio == 2.5
    input {
        $graphId
        ratio?: float
    }
    output { measured: boolean }
}
"#;
    let mut service = compile("float", &source);
    let code = generate(&service, &Options::default()).unwrap();
    assert!(code.contains("*v == 2.5_f32"), "{code}");
    let op = service
        .operations
        .iter_mut()
        .find(|op| op.id.name() == "Measure")
        .unwrap();
    let Requirements::List(clauses) = &mut op.requirements else {
        panic!("Measure declares its requirements");
    };
    clauses[0].when = Some(Condition::Equals(
        vec!["ratio".to_owned()],
        serde_json::json!(1e300),
    ));
    let diagnostics = generate(&service, &Options::default()).unwrap_err();
    assert!(
        diagnostics
            .to_string()
            .contains("`when` compares the float `ratio` with 1e300, outside the range of a float"),
        "{diagnostics}"
    );
}
