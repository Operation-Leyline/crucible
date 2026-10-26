use crucible_codegen_common::{describe, documentation};
use crucible_ir::ir::OperationIntent;
use pulldown_cmark::{Event, Parser, Tag};

fn fragment(markdown: &str) -> String {
    OperationIntent::from_markdown(markdown)
        .unwrap()
        .use_when
        .unwrap()
}

fn links(markdown: &str) -> Vec<(String, String)> {
    Parser::new(markdown)
        .filter_map(|event| match event {
            Event::Start(
                Tag::Link {
                    dest_url, title, ..
                }
                | Tag::Image {
                    dest_url, title, ..
                },
            ) => Some((dest_url.to_string(), title.to_string())),
            _ => None,
        })
        .collect()
}

#[test]
fn intent_context_preserves_initial_indented_code_and_its_rendered_html() {
    let body = "    send_request()\n    next_line()\n";
    let markdown = format!("## Use when\n\n{body}\n\n## Details\nOther.");
    let extracted = fragment(&markdown);
    assert_eq!(documentation::html(&extracted), documentation::html(body));
    assert_eq!(extracted, body);
}

#[test]
fn intent_context_preserves_first_reference_definition_even_with_an_in_section_duplicate() {
    let markdown = "[guide]: https://example.com/first \"First title\"\n\n## Use when\nRead [guide].\n\n[guide]: https://example.com/second \"Second title\"\n\n## Details\nOther.";
    let extracted = fragment(markdown);
    assert_eq!(links(&extracted), links(markdown));
    let html = documentation::html(&extracted);
    assert!(
        html.contains("href=\"https://example.com/first\" title=\"First title\""),
        "{html}"
    );
    assert!(!html.contains("second"), "{html}");
}

#[test]
fn intent_context_preserves_reference_links_before_an_unclosed_fence() {
    let markdown =
        "[guide]: https://example.com/first\n\n## Use when\nRead [guide].\n\n```txt\nunclosed";
    let extracted = fragment(markdown);
    assert_eq!(links(&extracted), links(markdown));
    assert_eq!(
        documentation::html(&extracted),
        documentation::html(markdown).replace("<h2>Use when</h2>\n", "")
    );
}

#[test]
fn intent_context_preserves_reference_titles_containers_and_normalized_labels() {
    for (definitions, body) in [
        (
            "> [GuIdE   NAME]:\n>   <https://example.com/quoted?x=1&y=2>\n>   \"A &quot;quoted&quot;\n>   title\"",
            "Read [guide name].",
        ),
        (
            "- [guide]: <https://example.com/list>\n  \"List title\"",
            "Read [guide].",
        ),
        ("[Straße]: https://example.com/unicode", "Read [STRASSE]."),
        ("[a\\]b]: https://example.com/escaped", "Read [a\\]b]."),
        (
            "[guide\n name]: https://example.com/multiline",
            "Read [GUIDE NAME].",
        ),
        (
            "[guide]: <https://example.com/a&#62;b?q=a&amp;x=&#92;path> \"A &quot;quote&quot; &amp; slash &#92;\"",
            "Read [guide].",
        ),
        (
            "[guide]: https://example.com/image \"Image title\"",
            "![Guidance][guide]",
        ),
    ] {
        let markdown = format!("{definitions}\n\n## Use when\n{body}");
        let extracted = fragment(&markdown);
        let original_links = links(&markdown);
        assert_eq!(
            original_links.len(),
            1,
            "invalid reference test case: {markdown}"
        );
        assert_eq!(links(&extracted), original_links, "{markdown}\n{extracted}");
        let original_html = documentation::html(&markdown);
        let fragment_html = documentation::html(&extracted);
        let original_link = original_html
            .split(if body.starts_with('!') {
                "<img "
            } else {
                "<a "
            })
            .nth(1)
            .unwrap()
            .split('>')
            .next()
            .unwrap();
        assert!(
            fragment_html.contains(original_link),
            "{markdown}\n{original_html}\n{fragment_html}"
        );
    }
}

#[test]
fn intent_context_describe_keeps_raw_docs_and_indented_code_after_public_lowering() {
    let markdown = "## Use when\n\n    send_request()\n    next_line()\n\n## Don't use when\nUse the list operation for several items.";
    let comments = markdown
        .lines()
        .map(|line| format!("/// {line}\n"))
        .collect::<String>();
    let package = crucible_lang::Package::new(
        "example.context",
        vec![crucible_lang::SourceFile {
            path: "context.cru".into(),
            text: format!(
                "service Context {{ version \"1\" protocol json operations [Call] }}\n{comments}readonly operation Call {{ http GET \"/call\" }}"
            ),
        }],
    );
    let compiled = crucible_lang::compile(&package, None);
    assert!(!compiled.has_errors(), "{}", compiled.render());
    let service = compiled.service.unwrap();
    assert_eq!(service.operations[0].docs.as_deref(), Some(markdown));
    let description = describe::describe(&service).unwrap();
    assert_eq!(
        description.operations["example.context#Call"]["intent"]["useWhen"]["html"],
        "<pre><code>send_request()\nnext_line()\n</code></pre>\n"
    );
    assert_eq!(
        description.operations["example.context#Call"]["documentation"]["markdown"],
        markdown
    );
}
