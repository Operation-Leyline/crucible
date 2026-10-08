//! mdBook API reference pages, rendered from the shared versioned description.
mod markdown;

use crucible_codegen_common::describe;
use crucible_smithy::ir::Service;
use crucible_smithy::{Diagnostics, ShapeId};

/// One generated artifact; publication is owned by `crucible-codegen`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct File {
    pub path: String,
    pub contents: String,
}

/// Generate all files before publication. The caller projects the audience first.
pub fn generate(service: &Service) -> Result<Vec<File>, Diagnostics> {
    let description = describe::describe(service)?;
    let mut diagnostics = Diagnostics::default();
    let mut paths = std::collections::BTreeSet::new();
    for (id, item) in description.operations.iter().chain(&description.shapes) {
        if id.starts_with("smithy.api#") {
            continue;
        }
        let directory = if description.operations.contains_key(id) {
            "operations"
        } else {
            "shapes"
        };
        let name = item["name"].as_str().expect("description name");
        if !paths.insert(format!("{directory}/{}", name.to_ascii_lowercase())) {
            diagnostics.push(
                &id.parse::<ShapeId>().expect("lowered id"),
                "docs.filename: case-insensitive API page name collision",
            );
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let mut files = markdown::render(&description);
    files.push(File {
        path: "describe.json".into(),
        contents: description.to_json(),
    });
    files.push(File { path:"book.toml".into(), contents:"[book]\nlanguage = \"en\"\nsrc = \"src\"\n\n[output.html]\n\"smart-punctuation\" = false\n".into() });
    for file in &mut files {
        file.contents.truncate(file.contents.trim_end().len());
        file.contents.push('\n');
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}
