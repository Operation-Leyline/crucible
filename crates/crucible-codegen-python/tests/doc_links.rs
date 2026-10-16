//! `[Name]` doc links (decision 80) in plain-text docstrings: the
//! generated Python name, without brackets.
use crucible_codegen_python::{Options, emit};
use std::path::Path;

#[test]
fn doc_links_are_python_names() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/doc-links");
    let service = crucible_ir::project(
        &crucible_lang::load_service(&dir).unwrap(),
        crucible_ir::Audience::External,
    )
    .unwrap();
    let options = Options {
        metadata: Default::default(),
        package_name: "links-client".into(),
        module_name: "links_client".into(),
        package_version: "0.0.0.dev0".into(),
        endpoint: None,
    };
    let files = emit(&service, &options).unwrap();
    let file = |path: &str| {
        files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path}"))
            .contents
            .clone()
    };
    let (models, client) = (
        file("src/links_client/models.py"),
        file("src/links_client/client.py"),
    );
    for text in [
        // Operation, type, member, enum value, the service.
        "Widgets, linked. Make one with LinksClient.create_widget and read it back with",
        "the read call; a Widget has a Widget.name and a",
        "Color, such as Color.RED. Gadget is a resource, LinksClient is this",
        // A prelude type and internal API are plain text, without brackets.
        "service and String a prelude type. Internal: Reveal and Secret.",
        "[NotALink] in code",
        "by GetWidgetInput.widget_id,",
        "returns it as GetWidgetOutput.widget. Fails with WidgetError.",
    ] {
        assert!(client.contains(text), "{text}\n{client}");
    }
    for text in [
        "A widget; LinksClient.create_widget makes one.",
        "One of Color; Color.BLUE by default.",
    ] {
        assert!(models.contains(text), "{text}\n{models}");
    }
    assert!(!client.contains("[Widget]") && !models.contains("[Color]"));
}
