#![cfg(feature = "all-clients")]
//! Doc links (decision 80) in every client's doc comments: each line of the
//! generated source that carries one of `tests/models/doc-links`' doc
//! sentences, per language, reviewed as a golden under
//! `tests/golden/doc-links/`. Bless with `CRUCIBLE_BLESS=1`.

use std::fs;
use std::path::Path;

use clients::clients;
use crucible_ir::{Audience, project};

#[path = "common/clients.rs"]
mod clients;
mod common;

/// Fragments of the fixture's documentation: a generated line that holds
/// one renders a doc comment the model wrote.
const SENTENCES: &[&str] = &[
    "Widgets, linked",
    "read it back with",
    "has a",
    "such as",
    "is a resource",
    "prelude type",
    "Internal:",
    "NotALink",
    "makes one",
    "chose",
    "by default",
    "is the first",
    "unlike",
    "made of",
    "Makes a",
    "becomes",
    "Reads the",
    "returns it as",
    "Raised by",
    "Reads a",
    "a list.",
    "a map.",
    "plain text where",
    "Why a call failed",
    "one of two",
];

#[test]
fn every_client_renders_doc_links_in_its_own_form() {
    let service = project(&common::load("doc-links"), Audience::External).unwrap();
    for (language, client) in clients() {
        let files = client
            .emit(&service)
            .unwrap_or_else(|error| panic!("{language}: {error:?}"));
        let mut lines = Vec::new();
        let mut files: Vec<_> = files
            .iter()
            .filter(|f| !f.path.ends_with(".json") && !f.path.ends_with(".md"))
            .filter(|f| !f.path.contains("runtime"))
            .collect();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        for file in files {
            for line in file.contents.lines() {
                if SENTENCES.iter().any(|s| line.contains(s)) {
                    lines.push(format!("{}: {}", file.path, line.trim()));
                }
            }
        }
        let text = lines.join("\n") + "\n";
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden/doc-links")
            .join(format!("{language}.txt"));
        if std::env::var_os("CRUCIBLE_BLESS").is_some() {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &text).unwrap();
        }
        let golden = fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("{language}: no golden; bless with CRUCIBLE_BLESS=1"))
            .replace("\r\n", "\n");
        assert_eq!(golden, text, "{language}");
        // Internal API never reaches an external client as a link or an id.
        assert!(!text.contains("example.links#"), "{language}");
    }
}
