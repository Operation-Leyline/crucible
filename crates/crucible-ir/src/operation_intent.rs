use pulldown_cmark::{BrokenLink, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

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
        // A section can use a reference link whose definition is elsewhere in
        // the comment. Keep only those needed definitions, in source order,
        // so rendering this Markdown fragment preserves the original link.
        let context = Parser::new(markdown);
        for text in [&mut intent.use_when, &mut intent.dont_use_when]
            .into_iter()
            .flatten()
        {
            let mut definitions = std::collections::BTreeSet::new();
            let callback = |link: BrokenLink<'_>| {
                if let Some(definition) = context.reference_definitions().get(&link.reference) {
                    definitions.insert((definition.span.start, definition.span.end));
                }
                None
            };
            Parser::new_with_broken_link_callback(text, Options::empty(), Some(callback))
                .for_each(drop);
            for (start, end) in definitions {
                text.push_str("\n\n");
                text.push_str(markdown[start..end].trim());
            }
        }
        (intent.use_when.is_some() || intent.dont_use_when.is_some()).then_some(intent)
    }

    /// Both sections contain guidance; used by opt-in documentation policy.
    pub fn is_complete(&self) -> bool {
        [&self.use_when, &self.dont_use_when]
            .into_iter()
            .all(|section| {
                section
                    .as_deref()
                    .is_some_and(|text| !text.trim().is_empty())
            })
    }

    fn finish(&mut self, markdown: &str, section: Option<(bool, usize)>, end: usize) {
        let Some((use_when, start)) = section else {
            return;
        };
        let text = markdown[start..end].trim();
        if text.is_empty() || Parser::new(text).next().is_none() {
            return;
        }
        let slot = if use_when {
            &mut self.use_when
        } else {
            &mut self.dont_use_when
        };
        if let Some(previous) = slot {
            previous.push_str("\n\n");
            previous.push_str(text);
        } else {
            *slot = Some(text.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OperationIntent;

    #[test]
    fn sections_keep_markdown_and_subheadings_but_stop_at_peer_headings() {
        let intent = OperationIntent::from_markdown("Summary.\n\n## Use when\n\n- Need **one** item.\n\n### Details\nKeep this.\n\n## Don't use when\n\nUse [ListItems] for several.\n\n## Examples\nOther prose.").unwrap();
        assert_eq!(
            intent.use_when.as_deref(),
            Some("- Need **one** item.\n\n### Details\nKeep this.")
        );
        assert_eq!(
            intent.dont_use_when.as_deref(),
            Some("Use [ListItems] for several.")
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
        assert_eq!(intent.use_when.as_deref(), Some("First.\n\nSecond."));
        assert_eq!(intent.dont_use_when, None);
        assert!(!intent.is_complete());
    }

    #[test]
    fn reference_definitions_outside_the_section_keep_links_working() {
        let intent = OperationIntent::from_markdown("[guide]: https://example.com/guide\n\n## Use when\nRead the [guide].\n\n## Details\nOther content.").unwrap();
        assert_eq!(
            intent.use_when.as_deref(),
            Some("Read the [guide].\n\n[guide]: https://example.com/guide")
        );
        assert_eq!(
            OperationIntent::from_markdown("## Use when\n[guide]: https://example.com/guide"),
            None
        );
    }
}
