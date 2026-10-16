//! Doc links (decision 80) in the R package: Rd links in the pages it
//! ships, roxygen links in the comments beside the code.
use crucible_codegen_r::{Options, emit};
use crucible_ir::{Audience, project};
use std::path::Path;

fn files() -> Vec<crucible_codegen_r::File> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/doc-links");
    let service = crucible_lang::load_service(&dir).unwrap();
    let service = project(&service, Audience::External).unwrap();
    let options = Options {
        package_name: "links.client".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
    };
    emit(&service, &options).unwrap()
}

fn file(files: &[crucible_codegen_r::File], path: &str) -> String {
    files
        .iter()
        .find(|f| f.path == path)
        .unwrap_or_else(|| panic!("{path}"))
        .contents
        .clone()
}

#[test]
fn rd_pages_link_help_topics() {
    let files = files();
    let operations = file(&files, "man/operations.Rd");
    for expected in [
        r"Reads the \code{\link{Widget}} that \code{\link[=operations]{client$create_widget()}} made, by \code{\link[=GetWidgetInput]{GetWidgetInput$widget_id}},",
        r"returns it as \code{\link[=GetWidgetOutput]{GetWidgetOutput$widget}}. Fails with \code{\link{WidgetError}}.",
    ] {
        assert!(operations.contains(expected), "{expected}\n{operations}");
    }
    let color = file(&files, "man/Color.Rd");
    assert!(
        color.contains(r#"A colour. \code{\link[=Color]{Color("RED")}} is the first."#),
        "{color}"
    );
    let widget = file(&files, "man/Widget.Rd");
    assert!(
        widget
            .contains(r"The name \code{\link[=CreateWidgetInput]{CreateWidgetInput$name}} chose."),
        "{widget}"
    );
}

#[test]
fn roxygen_comments_link_help_topics() {
    let files = files();
    let client = files
        .iter()
        .filter(|f| f.path.ends_with(".R"))
        .map(|f| f.contents.as_str())
        .collect::<String>();
    for expected in [
        "#' Widgets, linked. Make one with [client$create_widget()][operations] and read it back with",
        "#' the read call; a [Widget] has a [Widget$name][Widget] and a",
        "#' [Color], such as [Color(\"RED\")][Color]. Gadget is a resource, [LinksClient()][LinksClient] is this",
        "#' service and String a prelude type. Internal: Reveal and Secret.",
        "#' `[NotALink]` in code",
    ] {
        assert!(client.contains(expected), "{expected}\n{client}");
    }
}
