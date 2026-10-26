use std::collections::BTreeMap;

use pulldown_cmark::{BrokenLink, Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd};

/// Operation guidance extracted from conventional Markdown sections. The
/// original documentation stays intact on the operation.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OperationIntent {
    /// Markdown beneath the top-level `## Use when` heading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_when: Option<String>,
    /// Markdown beneath the top-level `## Don't use when` heading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dont_use_when: Option<String>,
}

impl OperationIntent {
    /// Extracts level-two sections, stopping at the next top-level level-one
    /// or level-two heading. Fenced code, quoted/list headings and inline
    /// formatting in a heading cannot accidentally declare an intent section.
    pub fn from_markdown(markdown: &str) -> Option<Self> {
        let mut intent = Self::default();
        let mut depth = 0;
        let mut heading: Option<(String, bool)> = None;
        let mut section: Option<(bool, usize)> = None;
        for (event, span) in Parser::new(markdown).into_offset_iter() {
            match event {
                Event::Start(tag) => {
                    if depth == 0
                        && matches!(
                            tag,
                            Tag::Heading {
                                level: HeadingLevel::H1 | HeadingLevel::H2,
                                ..
                            }
                        )
                    {
                        intent.finish(markdown, section.take(), span.start);
                        if matches!(
                            tag,
                            Tag::Heading {
                                level: HeadingLevel::H2,
                                ..
                            }
                        ) {
                            heading = Some((String::new(), true));
                        }
                    } else if let Some((_, plain)) = &mut heading {
                        *plain = false;
                    }
                    depth += 1;
                }
                Event::End(tag) => {
                    depth -= 1;
                    if matches!(tag, TagEnd::Heading(_))
                        && let Some((title, true)) = heading.take()
                    {
                        let use_when = match title.trim() {
                            "Use when" => Some(true),
                            "Don't use when" => Some(false),
                            _ => None,
                        };
                        section = use_when.map(|side| (side, span.end));
                    }
                }
                Event::Text(text) => {
                    if let Some((title, _)) = &mut heading {
                        title.push_str(&text);
                    }
                }
                _ => {
                    if let Some((_, plain)) = &mut heading {
                        *plain = false;
                    }
                }
            }
        }
        intent.finish(markdown, section, markdown.len());
        for text in [&mut intent.use_when, &mut intent.dont_use_when]
            .into_iter()
            .flatten()
        {
            preserve_reference_context(markdown, text);
        }
        (intent.use_when.is_some() || intent.dont_use_when.is_some()).then_some(intent)
    }

    /// Both sections contain guidance; used by opt-in documentation policy.
    pub fn is_complete(&self) -> bool {
        [&self.use_when, &self.dont_use_when]
            .into_iter()
            .all(|section| section.as_deref().is_some_and(has_guidance))
    }

    fn finish(&mut self, markdown: &str, section: Option<(bool, usize)>, end: usize) {
        let Some((use_when, start)) = section else {
            return;
        };
        let text = boundary_lines(&markdown[start..end]);
        if !has_guidance(text) {
            return;
        }
        let slot = if use_when {
            &mut self.use_when
        } else {
            &mut self.dont_use_when
        };
        if let Some(previous) = slot {
            previous.push_str(if previous.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            });
            previous.push_str(text);
        } else {
            *slot = Some(text.to_owned());
        }
    }
}

/// Strip only blank boundary lines: indentation, trailing spaces and the last
/// content line's newline can all affect CommonMark rendering.
fn boundary_lines(text: &str) -> &str {
    let blank = |c| matches!(c, ' ' | '\t' | '\r' | '\n');
    let Some(first) = text.find(|c| !blank(c)) else {
        return "";
    };
    let start = text[..first].rfind('\n').map_or(0, |at| at + 1);
    let last = text.rfind(|c| !blank(c)).expect("nonblank text");
    let end = text[last..]
        .find('\n')
        .map_or(text.len(), |at| last + at + 1);
    &text[start..end]
}

fn has_guidance(markdown: &str) -> bool {
    Parser::new(markdown).any(|event| match event {
        Event::Text(text) | Event::Code(text) | Event::Html(text) | Event::InlineHtml(text) => {
            !text.trim().is_empty()
        }
        _ => false,
    })
}

fn preserve_reference_context(markdown: &str, text: &mut String) {
    let context = Parser::new(markdown);
    let callback = |link: BrokenLink<'_>| {
        context
            .reference_definitions()
            .get(&link.reference)
            .map(|definition| {
                (
                    definition.dest.clone(),
                    definition.title.clone().unwrap_or_else(|| "".into()),
                )
            })
    };
    let mut definitions = BTreeMap::new();
    for event in Parser::new_with_broken_link_callback(text, Options::empty(), Some(callback)) {
        if let Event::Start(Tag::Link { link_type, id, .. } | Tag::Image { link_type, id, .. }) =
            event
            && matches!(
                link_type,
                LinkType::Reference
                    | LinkType::ReferenceUnknown
                    | LinkType::Collapsed
                    | LinkType::CollapsedUnknown
                    | LinkType::Shortcut
                    | LinkType::ShortcutUnknown
            )
            && let Some(definition) = context.reference_definitions().get(&id)
        {
            // Use the full comment's first definition even when the fragment
            // contains a later one. Serialize parsed values to detach quoted
            // and continued definitions from their original block containers.
            let mut value = format!("[{id}]: <{}>", definition_value(&definition.dest));
            if let Some(title) = &definition.title {
                value.push_str(&format!(" \"{}\"", definition_value(title)));
            }
            value.push('\n');
            definitions.insert(definition.span.start, value);
        }
    }
    if !definitions.is_empty() {
        // Definitions must precede the fragment: its last block can be an
        // unclosed fence, in which an appended definition would become code.
        let mut prefix: String = definitions.into_values().collect();
        prefix.push('\n');
        text.insert_str(0, &prefix);
    }
}

fn definition_value(text: &str) -> String {
    let mut escaped = String::new();
    for character in text.chars() {
        if matches!(character, '&' | '<' | '>' | '"' | '\\') || character.is_control() {
            escaped.push_str(&format!("&#{};", u32::from(character)));
        } else {
            escaped.push(character);
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::OperationIntent;

    #[test]
    fn sections_keep_markdown_and_subheadings_but_stop_at_peer_headings() {
        let intent = OperationIntent::from_markdown("Summary.\n\n## Use when\n\n- Need **one** item.\n\n### Details\nKeep this.\n\n## Don't use when\n\nUse [ListItems] for several.\n\n## Examples\nOther prose.").unwrap();
        assert_eq!(
            intent.use_when.as_deref(),
            Some("- Need **one** item.\n\n### Details\nKeep this.\n")
        );
        assert_eq!(
            intent.dont_use_when.as_deref(),
            Some("Use [ListItems] for several.\n")
        );
        assert!(intent.is_complete());
    }

    #[test]
    fn code_quotes_lists_and_other_headings_are_not_intent_sections() {
        for markdown in [
            "```md\n## Use when\nExample only.\n```",
            "~~~md\n## Use when\nExample only.\n~~~",
            "    ## Use when\n    Example only.",
            "> ## Use when\n> Quoted.",
            "- ## Use when\n  List item.",
            "### Use when\nWrong level.",
            "# Use when\nWrong level.",
            "## **Use when**\nFormatted title.",
            "## `Use when`\nCode title.",
            "Use when: ordinary prose.",
            "## Use whenever\nDifferent title.",
            "## Use when\n\n## Don't use when\n",
        ] {
            assert_eq!(OperationIntent::from_markdown(markdown), None, "{markdown}");
        }
    }

    #[test]
    fn setext_level_two_and_repeated_sections_follow_commonmark_order() {
        let intent = OperationIntent::from_markdown(
            "Use when\n--------\nFirst.\n\n## Use when\nSecond.\n\n# Appendix\nNot intent.",
        )
        .unwrap();
        assert_eq!(intent.use_when.as_deref(), Some("First.\n\nSecond.\n"));
        assert_eq!(intent.dont_use_when, None);
        assert!(!intent.is_complete());
    }

    #[test]
    fn reference_definitions_outside_the_section_keep_links_working() {
        let intent = OperationIntent::from_markdown("[guide]: https://example.com/guide\n\n## Use when\nRead the [guide].\n\n## Details\nOther content.").unwrap();
        assert_eq!(
            intent.use_when.as_deref(),
            Some("[guide]: <https://example.com/guide>\n\nRead the [guide].\n")
        );
        assert_eq!(
            OperationIntent::from_markdown("## Use when\n[guide]: https://example.com/guide"),
            None
        );
    }

    #[test]
    fn markup_without_guidance_text_does_not_satisfy_intent() {
        for markdown in ["---", ">", "```\n```", "[guide]: https://example.com"] {
            assert_eq!(
                OperationIntent::from_markdown(&format!("## Use when\n\n{markdown}\n")),
                None
            );
            assert!(
                !OperationIntent {
                    use_when: Some(markdown.into()),
                    dont_use_when: Some("Guidance.".into())
                }
                .is_complete()
            );
        }
        // The existing reference renderer displays raw HTML as literal text.
        for markdown in [
            "Guidance.",
            "`command`",
            "```\ncommand\n```",
            "<div>Guidance.</div>",
        ] {
            assert!(
                OperationIntent::from_markdown(&format!("## Use when\n\n{markdown}")).is_some()
            );
        }
    }
}
