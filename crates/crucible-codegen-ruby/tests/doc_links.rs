//! `[Name]` doc links (decision 80) as YARD references.
use crucible_codegen_ruby::{Options, emit};
use std::path::Path;

#[test]
fn doc_links_are_yard_references() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/doc-links");
    let service = crucible_ir::project(
        &crucible_lang::load_service(&dir).unwrap(),
        crucible_ir::Audience::External,
    )
    .unwrap();
    let options = Options {
        package_name: "links_client".into(),
        module_name: "Links".into(),
        package_version: "0.0.0".into(),
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
    let client = file("lib/links_client/client.rb");
    for text in [
        // Operation, type, member, enum value, the service.
        "# Widgets, linked. Make one with {LinksClient#create_widget} and read it back with",
        "# the read call; a {Widget} has a {Widget#name} and a",
        "# {Color}, such as {Color::RED}. Gadget is a resource, {LinksClient} is this",
        // A prelude type and internal API are plain text, without brackets.
        "# service and String a prelude type. Internal: Reveal and Secret.",
        "# `[NotALink]` in code, and [an ordinary link](https://example.com).",
        "{GetWidgetInput#widget_id}",
        "{GetWidgetOutput#widget}",
        "Fails with {WidgetError}.",
    ] {
        assert!(client.contains(text), "{text}\n{client}");
    }
    let widget = file("lib/links_client/models/widget.rb");
    assert!(
        widget.contains("# A widget; {LinksClient#create_widget} makes one."),
        "{widget}"
    );
    assert!(
        widget.contains("One of {Color}; {Color::BLUE} by default."),
        "{widget}"
    );
    let all: String = files.iter().map(|f| f.contents.as_str()).collect();
    assert!(!all.contains("{Secret") && !all.contains("reveal"));
}
