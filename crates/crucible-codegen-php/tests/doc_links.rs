//! `[Name]` doc links (decision 80) as PHPDoc `{@see}` tags.
use crucible_codegen_php::{Options, emit};
use std::path::Path;

fn files() -> Vec<crucible_codegen_php::File> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/doc-links");
    let service = crucible_ir::project(
        &crucible_lang::load_service(&dir).unwrap(),
        crucible_ir::Audience::External,
    )
    .unwrap();
    let options = Options {
        package_name: "example/links".into(),
        namespace: "LinksSdk".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
    };
    emit(&service, &options).unwrap()
}

#[test]
fn doc_links_are_see_tags() {
    let files = files();
    let file = |path: &str| {
        files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path}"))
            .contents
            .clone()
    };
    let client = file("src/LinksClient.php");
    for text in [
        // Operation, type, member, enum value, the service.
        r" * Widgets, linked. Make one with {@see \LinksSdk\LinksClient::createWidget()} and read it back with",
        r" * the read call; a {@see \LinksSdk\Widget} has a {@see \LinksSdk\Widget::$name} and a",
        r" * {@see \LinksSdk\Color}, such as {@see \LinksSdk\Color::RED}. Gadget is a resource, {@see \LinksSdk\LinksClient} is this",
        // A prelude type and internal API are plain text, without brackets.
        " * service and String a prelude type. Internal: Reveal and Secret.",
        " * `[NotALink]` in code, and [an ordinary link](https://example.com).",
        r"{@see \LinksSdk\GetWidgetInput::$widgetId}",
        r"{@see \LinksSdk\GetWidgetOutput::$widget}",
        r"Fails with {@see \LinksSdk\WidgetError}.",
    ] {
        assert!(client.contains(text), "{text}\n{client}");
    }
    let widget = file("src/Widget.php");
    assert!(
        widget.contains(r" * A widget; {@see \LinksSdk\LinksClient::createWidget()} makes one.")
    );
    assert!(
        widget.contains(r"One of {@see \LinksSdk\Color}; {@see \LinksSdk\Color::BLUE} by default.")
    );
    let all: String = files.iter().map(|f| f.contents.as_str()).collect();
    assert!(!all.contains(r"\Secret") && !all.contains("reveal"));
}
