//! Doc links (decision 80) in the C headers' Doxygen comments.
use crucible_codegen_c::{Options, emit};
use crucible_ir::{Audience, project};
use std::path::Path;

fn headers() -> String {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/doc-links");
    let service = crucible_lang::load_service(&dir).unwrap();
    let service = project(&service, Audience::External).unwrap();
    let options = Options {
        package_name: "links_client".into(),
        prefix: "links".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
    };
    emit(&service, &options)
        .unwrap()
        .into_iter()
        .filter(|f| f.path.ends_with(".h") && !f.path.contains("runtime/"))
        .map(|f| f.contents)
        .collect()
}

#[test]
fn doc_links_name_the_generated_symbols() {
    let out = headers();
    for expected in [
        // An operation, a type, an operation input's member, an error.
        " * Reads the links_models_widget that links_operations_create_widget() made, by links_models_get_widget_input_get_widget_id(),",
        "returns it as links_models_get_widget_output_get_widget(). Fails with links_models_widget_error.",
        // A member's accessor, an enum value, the client, plain text.
        "Make one with links_operations_create_widget() and read it back with",
        " * the read call; a links_models_widget has a links_models_widget_get_name() and a",
        " * links_models_color, such as links_models_color_known_red(). Gadget is a resource, links_client_new() is this",
        " * service and String a prelude type. Internal: Reveal and Secret.",
        " * `[NotALink]` in code",
    ] {
        assert!(out.contains(expected), "{expected}\n{out}");
    }
    assert!(!out.contains("[Widget]") && !out.contains("[GetWidget]"));
}
