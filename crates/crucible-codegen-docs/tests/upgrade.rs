//! Upgrade guides (docs/api-versions-clients.md, 5d): `upgrade_guide`, as
//! `crucible upgrade-guide` writes it, and the same guides as pages of a
//! reference with more than one release.

use std::fs;
use std::path::{Path, PathBuf};

use crucible_codegen_docs::{generate, upgrade_guide};
use crucible_ir::ir::Service;
use crucible_ir::{Audience, project};

fn package_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
}

fn shop_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../e2e/weather-service/versioned")
}

/// A package's service with the older releases it serves, as a build
/// lowers it.
fn versioned(dir: &Path) -> Service {
    crucible_lang::load_versioned_service(dir)
        .unwrap_or_else(|error| panic!("{}:\n{error}", dir.display()))
}

fn guide(service: &Service, audience: Audience, from: &str, to: Option<&str>) -> String {
    upgrade_guide(&project(service, audience).unwrap(), from, to)
        .unwrap_or_else(|error| panic!("{from} to {to:?}: {error}"))
}

/// Compares `contents` with `tests/golden/upgrade/<name>`, rewriting it
/// under `CRUCIBLE_BLESS`.
fn golden(name: &str, contents: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/upgrade")
        .join(name);
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, contents).unwrap();
    }
    assert_eq!(
        fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
            .replace("\r\n", "\n"),
        contents,
        "{}",
        path.display()
    );
}

/// The e2e versioned shop, from its oldest release and from the one before
/// the current, in both audiences; and the release-history fixture, whose
/// migrations are documented and two of whose steps are internal.
#[test]
fn upgrade_guides_match_reviewed_goldens() {
    let shop = versioned(&shop_dir());
    let history = versioned(&package_dir("version-history"));
    for (audience, suffix) in [
        (Audience::External, "external"),
        (Audience::Internal, "internal"),
    ] {
        golden(
            &format!("shop-v1-to-v4-{suffix}.md"),
            &guide(&shop, audience, "v1", None),
        );
        golden(
            &format!("shop-v3-to-v4-{suffix}.md"),
            &guide(&shop, audience, "v3", Some("v4")),
        );
        golden(
            &format!("history-v1-to-v3-{suffix}.md"),
            &guide(&history, audience, "v1", None),
        );
        golden(
            &format!("history-v1-to-v2-{suffix}.md"),
            &guide(&history, audience, "v1", Some("v2")),
        );
    }
}

/// `to` defaults to the current release, and a guide is the same bytes
/// each time.
#[test]
fn a_guide_goes_to_the_current_release_by_default_and_is_deterministic() {
    let shop = versioned(&shop_dir());
    let default = guide(&shop, Audience::External, "v2", None);
    assert_eq!(default, guide(&shop, Audience::External, "v2", Some("v4")));
    assert_eq!(default, guide(&shop, Audience::External, "v2", None));
    assert!(
        default.starts_with("# Upgrading from v2 to v4\n"),
        "{default}"
    );
}

/// The external guide names no internal step, element or older name; the
/// internal one does.
#[test]
fn an_external_guide_names_no_internal_api() {
    let history = versioned(&package_dir("version-history"));
    let external = guide(&history, Audience::External, "v1", None);
    let internal = guide(&history, Audience::Internal, "v1", None);
    for hidden in ["secret", "Audit", "Conflict", "SECRET", "holder"] {
        assert!(!external.contains(hidden), "{hidden} in:\n{external}");
        assert!(internal.contains(hidden), "{hidden} not in:\n{internal}");
    }
}

/// An unknown release, either way round, a pair in the wrong order and a
/// pair of one release are refused, naming the supported releases; so is
/// a service with no releases.
#[test]
fn an_unknown_or_reversed_pair_is_refused() {
    let shop = project(&versioned(&shop_dir()), Audience::External).unwrap();
    let refused = |from: &str, to: Option<&str>| {
        upgrade_guide(&shop, from, to)
            .expect_err("refused")
            .to_string()
    };
    let supported = "the supported releases are \"v1\", \"v2\", \"v3\", \"v4\", oldest first";
    for (from, to, says) in [
        ("v0", None, "\"v0\" is not a supported release"),
        ("v1", Some("v9"), "\"v9\" is not a supported release"),
        ("v4", Some("v1"), "\"v4\" is not older than \"v1\""),
        ("v3", Some("v3"), "\"v3\" is not older than \"v3\""),
        ("v4", None, "\"v4\" is not older than \"v4\""),
    ] {
        let message = refused(from, to);
        assert!(message.contains(says), "{message}");
        assert!(message.contains(supported), "{message}");
        assert!(message.starts_with("example.shop#Shop: "), "{message}");
    }
    // A service of one release has no pair to go between.
    let mut single = crucible_lang::load_service(&package_dir("service-docs-weather")).unwrap();
    let message = upgrade_guide(&single, &single.version.clone(), None)
        .expect_err("one release")
        .to_string();
    assert!(message.contains("is not older than"), "{message}");
    single.api_version = None;
    let message = upgrade_guide(&single, "v1", None)
        .expect_err("no releases")
        .to_string();
    assert!(message.contains("names no release"), "{message}");
}

/// What a Markdown reader shows: text, code and raw HTML's text, with
/// character references read.
fn shown(markdown: &str) -> String {
    let mut text = String::new();
    for event in pulldown_cmark::Parser::new(markdown) {
        match event {
            pulldown_cmark::Event::Text(t) | pulldown_cmark::Event::Code(t) => text.push_str(&t),
            pulldown_cmark::Event::Html(t) | pulldown_cmark::Event::InlineHtml(t) => {
                // A tag shows nothing; the text between tags is read by
                // the HTML parser, character references included.
                let mut inside = false;
                let mut outside = String::new();
                for c in t.chars() {
                    match c {
                        '<' => inside = true,
                        '>' => inside = false,
                        c if !inside => outside.push(c),
                        _ => {}
                    }
                }
                text.push_str(&unescape(&outside));
            }
            _ => {}
        }
    }
    text
}

/// HTML's character references, as the generator writes them.
fn unescape(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let Some(end) = tail.find(';') else {
            out.push_str(tail);
            return out;
        };
        let entity = &tail[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            _ => entity
                .strip_prefix('#')
                .and_then(|n| n.parse::<u32>().ok())
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Every link destination and raw HTML tag in `markdown`.
fn links_and_tags(markdown: &str) -> (Vec<String>, Vec<String>) {
    let mut destinations = Vec::new();
    let mut tags = Vec::new();
    for event in pulldown_cmark::Parser::new(markdown) {
        match event {
            pulldown_cmark::Event::Start(
                pulldown_cmark::Tag::Link { dest_url, .. }
                | pulldown_cmark::Tag::Image { dest_url, .. },
            ) => destinations.push(dest_url.to_string()),
            pulldown_cmark::Event::Html(t) | pulldown_cmark::Event::InlineHtml(t) => {
                tags.push(t.to_string());
            }
            _ => {}
        }
    }
    (destinations, tags)
}

/// The tags in raw HTML, `<...>` each; the text between them, model
/// text included, is escaped and holds none.
fn elements(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        let end = rest[start..]
            .find('>')
            .map_or(rest.len(), |end| start + end + 1);
        out.push(rest[start..end].to_owned());
        rest = &rest[end..];
    }
    out
}

/// Model text with Markdown and HTML in it (release names, a title,
/// migration and sunset documentation, a code block with a blank line in
/// it, a required member's default, a member's default) reads back as
/// written and opens no link, tag or heading of its own, in the guide alone
/// and in every page of the reference. A guide alone keeps only the
/// documentation's web links.
#[test]
fn model_text_in_a_guide_stays_text() {
    let mut service = versioned(&package_dir("version-history"));
    let names = [
        "[v1](https://evil.example)",
        "<b>v2</b>&amp;|~",
        "`v3`*_x_*",
    ];
    service.versions[0].name = names[0].into();
    service.versions[1].name = names[1].into();
    service.version = names[2].into();
    service.api_version = Some(names[2].into());
    service.title = Some("Shop [t](https://title.example) <script>x</script>".into());
    let hostile = "Moved <script>alert(1)</script> [here](javascript:alert(1)) ![i](javascript:alert(2)) [ListItems] [site](https://docs.example/a) [frag](#x) [root](/y)";
    service.versions[1].docs = Some(hostile.into());
    service.versions[0].sunset_docs = Some("# Not a heading <img src=x onerror=alert(1)>".into());
    // A blank line inside a code block would end the documentation's HTML
    // block, and the rest would be Markdown.
    let block =
        "Intro paragraph.\n\n    code line one\n\n    [x](javascript:alert(4)) *em*\n\nAfter.";
    service.versions[0].docs = Some(block.into());
    let sort: crucible_ir::ShapeId = "example.history#ListItemsInput".parse().unwrap();
    let crucible_ir::ir::Shape::Structure(input) = service.shapes.get_mut(&sort).unwrap() else {
        panic!("ListItemsInput");
    };
    let member = input.members.iter_mut().find(|m| m.name == "sort").unwrap();
    member.default = Some(serde_json::Value::String(
        "[m](javascript:alert(5)) *f*".into(),
    ));
    let default = "[d](javascript:alert(3)) *e*";
    let mut defaults = 0;
    for step in &mut service.versions[0].steps {
        if let crucible_ir::ir::VersionStep::Require { default: value, .. } = step {
            *value = serde_json::Value::String(default.into());
            defaults += 1;
        }
    }
    assert_eq!(defaults, 1, "the fixture requires a member by default");
    let projected = project(&service, Audience::External).unwrap();
    let alone = upgrade_guide(&projected, names[0], None).unwrap();
    let reference = generate(&projected).unwrap();
    // No page of the reference opens a link or tag of the model's.
    for file in reference.iter().filter(|file| file.path.ends_with(".md")) {
        let (destinations, tags) = links_and_tags(&file.contents);
        for destination in destinations {
            assert!(
                !destination.to_ascii_lowercase().contains("javascript"),
                "{destination} in {}",
                file.path
            );
        }
        for tag in tags.iter().flat_map(|html| elements(html)) {
            let lower = tag.to_ascii_lowercase();
            assert!(
                !lower.contains("javascript:")
                    && !lower.contains("<script")
                    && !lower.contains("onerror"),
                "{tag} in {}",
                file.path
            );
        }
    }
    let operation = &reference
        .iter()
        .find(|file| file.path == "src/operations/API_ListItems.md")
        .unwrap()
        .contents;
    assert!(
        shown(operation).contains("\"[m](javascript:alert(5)) *f*\""),
        "{operation}"
    );
    let pages: Vec<String> = reference
        .into_iter()
        .filter(|file| file.path.starts_with("src/upgrade/"))
        .map(|file| file.contents)
        .collect();
    assert_eq!(pages.len(), 3, "v1 to v2, v1 to v3, v2 to v3");
    for (index, markdown) in std::iter::once(&alone).chain(&pages).enumerate() {
        let (destinations, tags) = links_and_tags(markdown);
        for destination in &destinations {
            assert!(
                !destination.contains(':'),
                "{destination} in guide {index}:\n{markdown}"
            );
            if index == 0 {
                panic!("a guide alone links no page: {destination}\n{markdown}");
            }
        }
        for tag in tags.iter().flat_map(|html| elements(html)) {
            let lower = tag.to_ascii_lowercase();
            assert!(
                !lower.contains("<script") && !lower.contains("onerror") && !lower.contains("<b>"),
                "{tag} in guide {index}"
            );
            // The documentation's image lost its active destination.
            assert!(
                !lower.contains("<img") || lower.contains("<img src=\"\""),
                "{tag} in guide {index}"
            );
            assert!(!lower.contains("javascript:"), "{tag} in guide {index}");
            if index == 0 && lower.starts_with("<a ") {
                assert_eq!(
                    tag, "<a href=\"https://docs.example/a\">",
                    "in the guide alone"
                );
            }
        }
        let text = shown(markdown);
        // The title names its two releases as written.
        let titles: Vec<String> = [(0, 1), (0, 2), (1, 2)]
            .iter()
            .map(|&(a, b)| format!("Upgrading from {} to {}Moving", names[a], names[b]))
            .collect();
        assert!(
            titles.iter().any(|title| text.starts_with(title.as_str())),
            "guide {index}: {text}"
        );
        assert!(
            text.contains("Shop [t](https://title.example) <script>x</script>"),
            "{text}"
        );
        // The migration to v3's documentation, in each guide to v3.
        assert_eq!(
            text.contains("Moved <script>alert(1)</script> here"),
            !text.starts_with(&titles[0]),
            "{text}"
        );
        // v1's sunset and the migration to v2's documentation, its code
        // block whole, in each guide from v1.
        let from_v1 = text.starts_with(&format!("Upgrading from {} ", names[0]));
        assert_eq!(
            text.contains("Not a heading <img src=x onerror=alert(1)>"),
            from_v1,
            "{text}"
        );
        assert_eq!(
            text.contains("code line one\n\n[x](javascript:alert(4)) *em*\n"),
            from_v1,
            "{text}"
        );
        // No heading but the generator's.
        for line in markdown.lines().filter(|line| line.starts_with('#')) {
            assert!(
                [
                    "# Upgrading",
                    "## Release",
                    "## Sunset",
                    "## New since",
                    "### "
                ]
                .iter()
                .any(|start| line.starts_with(start)),
                "{line}"
            );
        }
    }
    // A doc link is its text alone, and a link to its page in the
    // reference.
    assert!(shown(&alone).contains("ListItems"), "{alone}");
    assert!(shown(&alone).contains("frag root"), "{alone}");
    assert!(
        shown(&alone).contains(&format!("the service uses \"{default}\"")),
        "{alone}"
    );
    assert!(!alone.contains("data-crucible-ref"), "{alone}");
    assert!(
        pages
            .iter()
            .any(|page| page.contains("<a href=\"../operations/API_ListItems.md\">ListItems</a>")),
        "{pages:?}"
    );
}

/// A reference with more than one release carries a guide from each
/// release to the next and to the current one, listed under the release it
/// starts from in `SUMMARY.md`, linked from that release's page and from
/// `llms.txt`, each page the guide `upgrade_guide` writes with its links
/// kept, every one of them to a generated page.
#[test]
fn a_reference_carries_its_guides_and_links_them() {
    let shop = project(&versioned(&shop_dir()), Audience::External).unwrap();
    let files = generate(&shop).unwrap();
    let file = |path: &str| {
        files
            .iter()
            .find(|file| file.path == path)
            .unwrap_or_else(|| panic!("no {path}"))
            .contents
            .as_str()
    };
    let guides: Vec<&str> = files
        .iter()
        .filter(|file| file.path.starts_with("src/upgrade/"))
        .map(|file| file.path.as_str())
        .collect();
    assert_eq!(
        guides,
        [
            "src/upgrade/v1_to_v2.md",
            "src/upgrade/v1_to_v4.md",
            "src/upgrade/v2_to_v3.md",
            "src/upgrade/v2_to_v4.md",
            "src/upgrade/v3_to_v4.md",
        ]
    );
    let summary = file("src/SUMMARY.md");
    assert!(
        summary.contains(
            "  - [v1](releases/v1.md)\n    - [Upgrading to v2](upgrade/v1_to_v2.md)\n    - [Upgrading to v4](upgrade/v1_to_v4.md)\n"
        ),
        "{summary}"
    );
    let v1 = file("src/releases/v1.md");
    assert!(
        v1.contains("[to v2](../upgrade/v1_to_v2.md), [to v4](../upgrade/v1_to_v4.md)."),
        "{v1}"
    );
    assert!(!file("src/releases/v4.md").contains("## Upgrading"));
    let llms = file("llms.txt");
    assert!(
        llms.contains("- [Upgrading from v3 to v4](src/upgrade/v3_to_v4.md)"),
        "{llms}"
    );
    assert!(file("llms-full.txt").contains("src/upgrade/v1_to_v4.md"));
    // The page is the guide, linked.
    let alone = upgrade_guide(&shop, "v1", None).unwrap();
    let page = file("src/upgrade/v1_to_v4.md");
    assert_eq!(shown(page), shown(&alone));
    let paths: std::collections::BTreeSet<&str> =
        files.iter().map(|file| file.path.as_str()).collect();
    let (destinations, tags) = links_and_tags(page);
    let hrefs = tags.iter().filter_map(|tag| {
        tag.strip_prefix("<a href=\"")
            .and_then(|rest| rest.split('"').next())
            .map(str::to_owned)
    });
    let mut checked = 0;
    for destination in destinations.into_iter().chain(hrefs) {
        let target = destination
            .strip_prefix("../")
            .unwrap_or_else(|| panic!("{destination}"));
        assert!(
            paths.contains(format!("src/{target}").as_str()),
            "{destination} in the v1 guide"
        );
        checked += 1;
    }
    assert!(checked > 10, "{checked} links");
}

/// A release's own reference (decision 79) carries no guide: its view
/// leaves out what its release cannot see, so its guides would differ from
/// the current reference's. Its release pages and `llms.txt` link the
/// current reference's guides instead, in its directory beside this one,
/// and `upgrade_guide` refuses the line.
#[test]
fn a_releases_own_reference_links_the_current_references_guides() {
    let current = versioned(&shop_dir());
    let line = crucible_ir::versions::line(&current, "v1").unwrap();
    let line = project(&line, Audience::External).unwrap();
    let files = generate(&line).unwrap();
    assert!(
        !files.iter().any(|file| file.path.contains("upgrade/")),
        "no guide pages"
    );
    let file = |path: &str| {
        files
            .iter()
            .find(|file| file.path == path)
            .unwrap_or_else(|| panic!("no {path}"))
            .contents
            .as_str()
    };
    assert!(!file("src/SUMMARY.md").contains("Upgrading"));
    let v1 = file("src/releases/v1.md");
    assert!(
        v1.contains(
            "[to v2](../../../v4/src/upgrade/v1_to_v2.md), [to v4](../../../v4/src/upgrade/v1_to_v4.md)."
        ),
        "{v1}"
    );
    assert!(v1.contains("in the current release's reference"), "{v1}");
    let llms = file("llms.txt");
    assert!(
        llms.contains("- [Upgrading from v3 to v4](../v4/src/upgrade/v3_to_v4.md)"),
        "{llms}"
    );
    // In one file at the root, the links are rebased onto the root.
    let full = file("llms-full.txt");
    assert!(
        full.contains("[to v4](../v4/src/upgrade/v1_to_v4.md)"),
        "{full}"
    );
    let message = upgrade_guide(&line, "v1", None)
        .expect_err("a line")
        .to_string();
    assert!(message.contains("current model"), "{message}");
}

/// A release name too long for a file name keeps the start of its escaped
/// form and a hash of the name, so a guide's file stays under 255 bytes
/// and two such names do not share one.
#[test]
fn long_release_names_have_short_file_names() {
    let mut service = versioned(&package_dir("version-history"));
    let long = |c: char| format!("{}{}", "r".repeat(150), c);
    service.versions[0].name = long('a');
    service.versions[1].name = long('b');
    let files = generate(&project(&service, Audience::External).unwrap()).unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for file in files.iter().filter(|file| {
        file.path.starts_with("src/upgrade/") || file.path.starts_with("src/releases/")
    }) {
        let name = file.path.rsplit('/').next().unwrap();
        assert!(name.len() <= 255, "{name}");
        assert!(seen.insert(name.to_ascii_lowercase()), "{name} twice");
    }
    assert_eq!(
        seen.len(),
        6,
        "three release pages and three guides: {seen:?}"
    );
}

/// A migration whose only change is an operation retired under an
/// exception is not "no change", and a member a type gained in a later
/// release than the type is listed although the type is new too.
#[test]
fn a_retired_operation_and_a_later_member_are_changes() {
    let retired = versioned(&package_dir("version-retired-operation"));
    let text = guide(&retired, Audience::External, "v1", None);
    assert!(text.contains("### Retired operations"), "{text}");
    let section = |release: &str| {
        let start = text.find(&format!("## Release {release}")).unwrap();
        let rest = &text[start + 3..];
        rest[..rest.find("\n## ").unwrap_or(rest.len())].to_owned()
    };
    for release in ["v2", "v3"] {
        let section = section(release);
        if section.contains("### Retired operations") {
            assert!(!section.contains("No change"), "{section}");
        }
    }
    let shop = versioned(&shop_dir());
    let text = guide(&shop, Audience::External, "v1", None);
    // `Label` is new in v3, its `color` in v4.
    assert!(
        text.contains("- Data type <code>Label</code>: added in v3."),
        "{text}"
    );
    assert!(
        text.contains("- Member <code>Label.color</code>: added in v4."),
        "{text}"
    );
    assert!(!text.contains("<code>Label.title</code>: added"), "{text}");
}

/// Every release's reference in one output (`--all-releases`): the guides
/// are in the current release's directory alone, and every older release's
/// links to them resolve to those files.
#[test]
fn every_releases_reference_links_the_current_ones_guides() {
    let current = versioned(&shop_dir());
    let files = crucible_codegen_docs::generate_releases(
        &current,
        Audience::External,
        &crucible_codegen_docs::Options::default(),
    )
    .unwrap();
    let paths: std::collections::BTreeSet<&str> =
        files.iter().map(|file| file.path.as_str()).collect();
    let guides: Vec<&str> = paths
        .iter()
        .copied()
        .filter(|path| path.contains("/upgrade/"))
        .collect();
    assert_eq!(guides.len(), 5, "{guides:?}");
    assert!(
        guides
            .iter()
            .all(|path| path.starts_with("v4/src/upgrade/"))
    );
    let mut checked = 0;
    for file in files.iter().filter(|file| file.path.ends_with(".md")) {
        let dir = file.path.rsplit_once('/').map_or("", |(dir, _)| dir);
        let (destinations, _) = links_and_tags(&file.contents);
        for destination in destinations.iter().filter(|d| d.contains("upgrade/")) {
            let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
            for part in destination.split('/') {
                match part {
                    ".." => {
                        parts
                            .pop()
                            .unwrap_or_else(|| panic!("{destination} leaves the output"));
                    }
                    part => parts.push(part),
                }
            }
            let target = parts.join("/");
            assert!(
                paths.contains(target.as_str()),
                "{destination} in {}",
                file.path
            );
            checked += 1;
        }
    }
    assert!(checked > 20, "{checked} links");
}
