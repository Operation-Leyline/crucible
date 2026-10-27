#![cfg(feature = "all-clients")]
//! Model text that the generated languages' lexers read differently: every
//! line break Unicode has, a byte order mark, control characters,
//! bidirectional controls, comment terminators, doc-tool tags and
//! interpolation syntax, in documentation, a deprecation message, the service version, enum values, a default and a pattern
//! (the `hostile-text` package under `tests/models`). Each was a compile error
//! somewhere: rustc refuses a bare CR or a bidirectional control in a doc
//! comment, Go a BOM anywhere but a file's start, C# U+2028 or NEL in a string
//! literal, R a NUL anywhere, and `*/` in a member's docs ended PHP's
//! constructor docblock. Ruby's lexer reads a raw CRLF in a literal as LF,
//! and a line that starts with `@` is a tag to JSDoc, PHPDoc, YARD, Doxygen
//! and roxygen.

use clients::clients;
#[cfg(feature = "all-outputs")]
use crucible_codegen::Generator;
use crucible_codegen::{File, Server};
use crucible_ir::ir::Service;
#[cfg(feature = "all-outputs")]
use serde_json::Value;

#[path = "common/clients.rs"]
mod clients;
mod common;

/// The service version the compiler refuses to read from source (the
/// version names the `Crucible-Version` header, visible ASCII only): set on
/// the lowered service instead.
const HOSTILE_VERSION: &str = "1 cr\rver-lf\n ver-ls\u{2028} bom\u{feff} bidi\u{202e} end*/";

/// Characters no generated source file carries raw. Each language has an
/// escape for them in a literal, and documentation drops or rewrites them.
/// The bidirectional controls are what rustc's deny-by-default
/// `text_direction_codepoint_in_comment` and `_in_literal` lints and
/// GCC/Clang's `-Wbidi-chars` refuse; the marks beside them are as invisible.
fn forbidden(c: char) -> bool {
    matches!(
        c,
        '\u{2028}'
            | '\u{2029}'
            | '\u{feff}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{061c}'
    ) || (c.is_control() && !matches!(c, '\n' | '\t'))
}

/// The hostile package as the generators receive it.
fn service() -> Service {
    let mut service = common::load("hostile-text");
    service.version = HOSTILE_VERSION.to_owned();
    service
}

/// Every client, the Rust server and the server's operation stubs.
fn outputs() -> Vec<(&'static str, Vec<File>)> {
    outputs_of(&service())
}

fn outputs_of(service: &Service) -> Vec<(&'static str, Vec<File>)> {
    let mut outputs: Vec<(&'static str, Vec<File>)> = clients()
        .into_iter()
        .map(|(language, client)| {
            let files = client
                .emit(service)
                .unwrap_or_else(|error| panic!("{language}: {error:?}"));
            (language, files)
        })
        .collect();
    let server = Server::Rust(Default::default())
        .emit(service)
        .unwrap_or_else(|error| panic!("rust-server: {error:?}"));
    outputs.push(("rust-server", server));
    let stubs = crucible_codegen_rust::scaffold(service, &Default::default())
        .into_iter()
        .map(|stub| File {
            path: stub.path().to_string_lossy().replace('\\', "/"),
            contents: stub.contents,
        })
        .collect();
    outputs.push(("rust-scaffold", stubs));
    outputs
}

/// Source files: not data (JSON, whose encoder escapes what it must) and
/// not prose (Markdown, read as text).
fn source(file: &File) -> bool {
    !file.path.ends_with(".json") && !file.path.ends_with(".md")
}

/// A comment line in any of the generated languages.
fn comment(line: &str) -> bool {
    let line = line.trim_start();
    ["*", "/*", "//", "#"]
        .iter()
        .any(|marker| line.starts_with(marker))
}

/// Each line of `files`' source that carries a [`forbidden`] character raw.
fn raw(language: &str, files: &[File], problems: &mut Vec<String>) {
    for file in files.iter().filter(|file| source(file)) {
        for (number, line) in file.contents.split('\n').enumerate() {
            // PHP is no exception: a single-quoted literal takes every byte
            // as written, but `git`'s `text=auto` rewrites a CR in a
            // committed SDK, so such a value is double-quoted with escapes.
            let Some(c) = line.chars().find(|c| forbidden(*c)) else {
                continue;
            };
            problems.push(format!(
                "{language} {}:{}: U+{:04X} in {line:?}",
                file.path,
                number + 1,
                u32::from(c)
            ));
        }
    }
}

#[test]
fn hostile_model_text_never_reaches_generated_source_raw() {
    let mut problems = Vec::new();
    for (language, files) in outputs() {
        raw(language, &files, &mut problems);
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The service version (`1 cr\rver-lf\n ver-ls…`) stays on the comment line
/// that names it: no generated line starts with what followed a line break.
#[test]
fn hostile_service_version_never_leaves_its_comment() {
    let mut problems = Vec::new();
    for (language, files) in outputs() {
        for file in files.iter().filter(|file| source(file)) {
            for (number, line) in file.contents.lines().enumerate() {
                if line.trim_start().starts_with("ver-") {
                    problems.push(format!("{language} {}:{}: {line:?}", file.path, number + 1));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The model's `*/` (`end*/` in its documentation and deprecation message)
/// never closes a block comment early, in the languages whose documentation
/// is `/** */`.
#[test]
fn hostile_model_text_never_ends_a_block_comment() {
    let mut problems = Vec::new();
    for (language, files) in outputs() {
        if !["ts", "java", "kotlin", "php", "c", "cpp"].contains(&language) {
            continue;
        }
        for file in files.iter().filter(|file| source(file)) {
            for (number, line) in file.contents.lines().enumerate() {
                // In a string literal (Kotlin's `@Deprecated("…")`) it ends nothing.
                if comment(line) && line.contains("end*/") {
                    problems.push(format!("{language} {}:{}: {line:?}", file.path, number + 1));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The text of a documentation comment line, after its marker.
fn comment_body(line: &str) -> Option<&str> {
    let line = line.trim_start();
    ["#'", "///", "//", "/**", "*", "#"]
        .iter()
        .find_map(|marker| line.strip_prefix(marker))
        .map(str::trim_start)
}

/// A model line that starts with `@` (`@returns injected`) stays prose in
/// the doc tools that read such a line as a tag: JSDoc, Javadoc, KDoc,
/// PHPDoc, YARD, Doxygen and roxygen. JSDoc, PHPDoc and YARD read one inside
/// a code fence (`@throws fenced`) as a tag too; KDoc's lexer knows fences.
#[test]
fn hostile_model_text_never_becomes_a_doc_tag() {
    let mut problems = Vec::new();
    for (language, files) in outputs() {
        if !["ts", "java", "kotlin", "php", "ruby", "c", "cpp", "r"].contains(&language) {
            continue;
        }
        let fences = ["ts", "php", "ruby"].contains(&language);
        for file in files.iter().filter(|file| source(file)) {
            for (number, line) in file.contents.lines().enumerate() {
                if comment_body(line).is_some_and(|body| {
                    body.starts_with("@returns injected")
                        || (fences && body.starts_with("@throws fenced"))
                }) {
                    problems.push(format!("{language} {}:{}: {line:?}", file.path, number + 1));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// An enum value with a NUL: C++ builds it with its length, so the value is
/// not cut short at the NUL; R, whose strings cannot hold one, refuses the
/// model; every other client escapes it.
#[test]
fn nul_in_an_enum_value_is_kept_or_refused() {
    let service = {
        let mut service = common::load_with("hostile-text", |package| {
            for file in &mut package.files {
                file.text = file
                    .text
                    .replace("    BIDI = ", "    NUL = \"n\\u{0}l\"\n    BIDI = ");
            }
        });
        service.version = HOSTILE_VERSION.to_owned();
        service
    };
    let mut problems = Vec::new();
    for (language, client) in clients() {
        match client.emit(&service) {
            Err(error) if language == "r" => assert!(error.to_string().contains("NUL"), "{error}"),
            Ok(_) if language == "r" => {
                problems.push("r: emitted a NUL it cannot represent".into())
            }
            Err(error) => problems.push(format!("{language}: {error}")),
            Ok(files) => {
                raw(language, &files, &mut problems);
                if language == "cpp"
                    && !files
                        .iter()
                        .any(|file| file.contents.contains("std::string(\"n\\000l\", 3)"))
                {
                    problems.push("cpp: the NUL value is not built with its length".into());
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// A model may name a shape `type`; Rust cannot name a type that, and
/// both Rust generators say so instead of emitting `pub type type = …`.
#[test]
fn rust_keyword_type_names_are_diagnosed() {
    let mut service = service();
    service.rename_shape(&"example.hostile#Text".parse().unwrap(), "type");
    let client = clients()
        .remove(0)
        .1
        .emit(&service)
        .unwrap_err()
        .to_string();
    assert!(client.contains("`type`"), "{client}");
    let server = Server::Rust(Default::default())
        .emit(&service)
        .unwrap_err()
        .to_string();
    assert!(server.contains("`type`"), "{server}");
}

/// Terraform, CDK and the service CLI, from the Terraform fixture with the
/// hostile documentation on its members and service, the hostile version, and
/// the hostile enum as a property's type.
#[cfg(feature = "all-outputs")]
#[test]
fn hostile_model_text_never_reaches_infrastructure_or_cli_source_raw() {
    let hostile = service();
    let hostile_doc = hostile.docs.clone();
    let mut service = common::load("terraform-inventory");
    service.version = hostile.version.clone();
    service.docs = hostile_doc.clone();
    let kind: crucible_ir::ShapeId = "example.infra#Kind".parse().unwrap();
    service.shapes.insert(
        kind.clone(),
        hostile.shapes[&"example.hostile#Kind"
            .parse::<crucible_ir::ShapeId>()
            .unwrap()]
            .clone(),
    );
    for resource in service.resources.values_mut() {
        if let Some(slug) = resource.properties.get_mut("slug") {
            *slug = kind.clone();
        }
    }
    for shape in service.shapes.values_mut() {
        if let crucible_ir::ir::Shape::Structure(structure) = shape {
            for member in &mut structure.members {
                if member.name == "slug" {
                    member.target = kind.clone();
                }
                member.docs = hostile_doc.clone();
            }
        }
    }
    let mut bindings: Value = serde_json::from_str(include_str!(
        "../../crucible-codegen-terraform/tests/fixtures/bindings.json"
    ))
    .unwrap();
    bindings["resources"]["example.infra#Widget"]["cdk"] =
        serde_json::json!({"resource_type": "Custom::Widget"});
    let bindings: crucible_codegen::ProvisioningBindings =
        serde_json::from_value(bindings).unwrap();
    let generators = [
        Generator::Terraform {
            bindings: bindings.clone(),
            options: crucible_codegen::TerraformOptions {
                module_path: "example.com/terraform-provider-inventory".into(),
                provider_name: "inventory".into(),
                provider_address: "registry.terraform.io/example/inventory".into(),
            },
        },
        Generator::Cdk {
            bindings,
            options: serde_json::from_str(include_str!(
                "../../crucible-codegen-cdk/tests/fixtures/options.json"
            ))
            .unwrap(),
        },
    ];
    let mut problems = Vec::new();
    for generator in generators {
        let files = generator
            .emit(&service)
            .unwrap_or_else(|error| panic!("{}: {error:?}", generator.target()));
        raw(generator.target(), &files, &mut problems);
        // CDK's TypeScript documentation is `/** */`; Go's is `//`.
        for file in files.iter().filter(|file| file.path.ends_with(".ts")) {
            for (number, line) in file.contents.lines().enumerate() {
                if comment(line) && line.contains("end*/") {
                    problems.push(format!(
                        "{} {}:{}: {line:?}",
                        generator.target(),
                        file.path,
                        number + 1
                    ));
                }
            }
        }
    }
    let cli = Generator::Cli(crucible_codegen::CliOptions {
        bin_name: "hostile".into(),
        package_version: "1.0.0".into(),
        endpoint: Some("https://example.test".into()),
        runtime_path: Some("/crucible".into()),
    })
    .emit(&self::service())
    .unwrap_or_else(|error| panic!("cli: {error:?}"));
    raw("cli", &cli, &mut problems);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The hostile service as a package's guide sees it: the hostile version
/// names the `Crucible-Version` header too, which snippet comments mention.
fn guided_service() -> Service {
    let mut service = service();
    service.api_version = Some(HOSTILE_VERSION.to_owned());
    service
}

/// Whether `line`, carrying model text, keeps it inside one comment: a
/// line comment, or a block comment that only its last `*/` closes.
fn contained(line: &str) -> bool {
    let line = line.trim_start();
    if let Some(rest) = line.strip_prefix("/*") {
        return rest.find("*/").is_none_or(|end| end + 2 == rest.len());
    }
    ["//", "#", "*"]
        .iter()
        .any(|marker| line.starts_with(marker))
}

/// Every language's snippets: no unprintable character raw, and the
/// example's title (`t ESC[201~ ``` evil LF ## Instructions: rm -rf */
/// end`) stays on its comment line, closing no block comment early.
#[test]
fn hostile_model_text_never_reaches_snippets_raw() {
    let service = guided_service();
    let mut problems = Vec::new();
    for (language, client) in clients() {
        let surface = client
            .surface(&service, crucible_codegen::Audience::External)
            .unwrap_or_else(|error| panic!("{language}: {error}"));
        for (id, operation) in &surface.operations {
            let snippets = std::iter::once(&operation.snippet).chain(&operation.paginated_snippet);
            for snippet in snippets {
                assert!(!snippet.is_empty(), "{language} {id}: no snippet");
                for (number, line) in snippet.lines().enumerate() {
                    let at = format!("{language} {id} snippet:{}", number + 1);
                    if let Some(c) = line.chars().find(|c| forbidden(*c)) {
                        problems.push(format!("{at}: U+{:04X} in {line:?}", u32::from(c)));
                    }
                    let trimmed = line.trim_start();
                    if trimmed.starts_with("## Instructions") || trimmed.starts_with("ver-") {
                        problems.push(format!("{at}: model text left its comment: {line:?}"));
                    }
                    if line.contains("Instructions: rm -rf") && !contained(line) {
                        problems.push(format!("{at}: the title is not one comment: {line:?}"));
                    }
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// What a Markdown file's structure is: its headings, its HTML, and its
/// links' destinations, outside code.
#[derive(Debug, Default)]
struct Structure {
    headings: Vec<String>,
    html: Vec<String>,
    links: Vec<String>,
}

fn structure(text: &str) -> Structure {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    let mut out = Structure::default();
    let mut heading: Option<String> = None;
    for event in Parser::new(text) {
        match event {
            Event::Start(Tag::Heading { .. }) => heading = Some(String::new()),
            Event::End(TagEnd::Heading(level)) => {
                out.headings
                    .push(format!("{level:?} {}", heading.take().unwrap_or_default()));
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(heading) = &mut heading {
                    heading.push_str(&text);
                }
            }
            Event::Html(html) | Event::InlineHtml(html) => out.html.push(html.to_string()),
            Event::Start(Tag::Link { dest_url, .. }) => out.links.push(dest_url.to_string()),
            _ => {}
        }
    }
    out
}

/// README.md and AGENTS.md, for every language: the hostile title,
/// `@externalDocs` name and URL and version add no heading, paragraph,
/// HTML or link, carry no unprintable character, and the documentation
/// link's URL is percent-encoded where it would end the link.
#[test]
fn hostile_model_text_never_restructures_package_guides() {
    let service = guided_service();
    let mut problems = Vec::new();
    for (language, client) in clients() {
        let files = client
            .emit(&service)
            .unwrap_or_else(|error| panic!("{language}: {error:?}"));
        let guides: Vec<&File> = files
            .iter()
            .filter(|file| file.path.ends_with("README.md") || file.path.ends_with("AGENTS.md"))
            .collect();
        assert!(guides.len() >= 2, "{language}: no README.md and AGENTS.md");
        for file in guides {
            let at = format!("{language} {}", file.path);
            if let Some(c) = file.contents.chars().find(|c| forbidden(*c)) {
                problems.push(format!("{at}: U+{:04X}", u32::from(c)));
            }
            let found = structure(&file.contents);
            let allowed: &[&str] = if file.path.ends_with("README.md") {
                &[
                    "Install",
                    "Create a client",
                    "Call an operation",
                    "Pagination",
                    "Errors",
                    "Retries and idempotency",
                    "Links",
                ]
            } else {
                &[
                    "A complete call: Send",
                    "A complete call: Do",
                    "Documentation",
                ]
            };
            let (title, sections) = found.headings.split_first().expect("a title heading");
            if !title.starts_with("H1 ")
                || title.contains("IMPORTANT")
                || title.contains("ignore previous")
            {
                problems.push(format!("{at}: title heading {title:?}"));
            }
            for heading in sections {
                let text = heading.strip_prefix("H2 ").unwrap_or(heading);
                if !heading.starts_with("H2 ") || !allowed.contains(&text) {
                    problems.push(format!("{at}: heading {heading:?}"));
                }
            }
            if found.html.len() != 1 || !found.html[0].starts_with("<!-- Generated by crucible.") {
                problems.push(format!("{at}: HTML {:?}", found.html));
            }
            for link in &found.links {
                let ours = link.starts_with("https://github.com/Operation-Leyline/crucible");
                if !ours && link != "https://x.example/a%3Eb%29%3Cc]%28https://evil.example" {
                    problems.push(format!("{at}: link {link:?}"));
                }
            }
            if !found
                .links
                .iter()
                .any(|link| link.starts_with("https://x.example/"))
            {
                problems.push(format!("{at}: the model's documentation link is missing"));
            }
            for line in file.contents.lines() {
                if line.starts_with("IMPORTANT") || line.starts_with("## SYSTEM") {
                    problems.push(format!("{at}: injected line {line:?}"));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The raw request for `Send`: its example's body holds an interrupt, the
/// end of a bracketed paste, a NUL and line and format characters, so it is
/// sent through `base64 -d` with its digest over the exact bytes; its
/// `X-Note` header holds a line break and controls no request may carry, so
/// the command leaves it out and says why; the label's quote, `$(..)` and
/// backticks stay inside single quotes.
#[test]
fn hostile_example_text_never_reaches_a_curl_command_raw() {
    use std::fs;
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join(".git")).unwrap();
    common::copy_package(
        &common::package_dir("hostile-text"),
        &directory.path().join("model"),
    );
    let path = directory.path().join(crucible_codegen::sdk::FILE_NAME);
    fs::write(
        &path,
        "schema_version = 1\nmodel = \"model\"\n\n[sdk.ts]\npackage = \"hostile\"\nmiddleware = [\"content-sha256\"]\nout = \"generated/ts\"\n",
    )
    .unwrap();
    let file = crucible_codegen::sdk::load(&path).unwrap();
    let service = guided_service();
    let selected: Vec<_> = file.sdks.values().collect();
    let document = crucible_codegen::sdks::document(&file, &service, &selected).unwrap();
    let mut problems = Vec::new();
    for (id, operation) in document["http"]["operations"].as_object().unwrap() {
        let snippet = operation["snippet"].as_str().unwrap();
        if let Some(c) = snippet.chars().find(|c| forbidden(*c)) {
            problems.push(format!("{id}: U+{:04X} in {snippet:?}", u32::from(c)));
        }
    }
    let send = &document["http"]["operations"]["example.hostile#Send"];
    let snippet = send["snippet"].as_str().unwrap();
    assert_eq!(send["bodyEncoding"], "base64", "{send}");
    assert!(snippet.contains("| base64 -d | curl -X POST"), "{snippet}");
    assert!(!snippet.contains("pwned"), "{snippet}");
    let bytes =
        crucible_codegen_common::snippet::base64_bytes(send["body"].as_str().unwrap()).unwrap();
    assert!(bytes.contains(&0), "the NUL is sent");
    let digest: String = <sha2::Sha256 as sha2::Digest>::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert!(
        snippet.contains(&format!("x-amz-content-sha256: {digest}")),
        "{snippet}"
    );
    let omitted: Vec<&str> = send["omitted"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|header| header["name"].as_str())
        .collect();
    assert!(omitted.contains(&"X-Note"), "{send}");
    assert!(omitted.contains(&"Crucible-Version"), "{send}");
    assert!(!snippet.contains("X-Note"), "{snippet}");
    assert!(snippet.contains("\"$HOSTILE_ENDPOINT\"'/s/"), "{snippet}");
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
