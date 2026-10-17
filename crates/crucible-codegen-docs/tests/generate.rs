use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crucible_codegen_docs::generate;
use crucible_ir::{Audience, project};
use serde_json::Value;

const WEATHER: &str = "service-docs-weather";
const CATALOG: &str = "service-docs";
const LINKS: &str = "doc-links";

fn package_dir(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
}

/// The package's lowered service, operations tied to their resources.
fn documented(name: &str) -> crucible_ir::ir::Service {
    crucible_lang::load_service(&package_dir(name))
        .unwrap_or_else(|rendered| panic!("{name}:\n{rendered}"))
}

fn files(name: &str, audience: Audience) -> Vec<crucible_codegen_docs::File> {
    generate(&project(&documented(name), audience).unwrap()).unwrap()
}

/// The renderer is identical for every service; `golden/site` holds its single
/// reviewed copy, so a runtime edit shows up once rather than in every fixture.
const SITE_RUNTIME: [&str; 3] = ["index.html", "explorer.js", "explorer.css"];

fn golden(name: &str, package: &str, audience: Audience) {
    golden_files(name, files(package, audience));
}

fn golden_files(name: &str, files: Vec<crucible_codegen_docs::File>) {
    let files: Vec<_> = files
        .into_iter()
        // describe.js repeats describe.json, and llms-full.txt the pages;
        // the site and llms tests pin those.
        .filter(|f| {
            !["describe.js", "llms-full.txt"].contains(&f.path.as_str())
                && !SITE_RUNTIME.contains(&f.path.as_str())
        })
        .collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    let mut expected = BTreeSet::new();
    for file in files {
        let path = root.join(&file.path);
        expected.insert(file.path.clone());
        if std::env::var_os("CRUCIBLE_BLESS").is_some() {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &file.contents).unwrap();
        }
        assert_eq!(
            fs::read_to_string(&path).unwrap().replace("\r\n", "\n"),
            file.contents,
            "{}",
            path.display()
        );
    }
    fn visit(root: &Path, current: &Path, found: &mut BTreeSet<String>) {
        for entry in fs::read_dir(current).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, found);
            } else {
                found.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let mut found = BTreeSet::new();
    visit(&root, &root, &mut found);
    assert_eq!(found, expected, "stale golden files");
}

#[test]
fn weather_external_and_internal_match_reviewed_goldens() {
    golden("weather-external", WEATHER, Audience::External);
    golden("weather-internal", WEATHER, Audience::Internal);
}
#[test]
fn doc_links_match_reviewed_goldens() {
    golden("links-external", LINKS, Audience::External);
    golden("links-internal", LINKS, Audience::Internal);
}

#[test]
fn doc_links_link_pages_and_internal_ones_are_text_externally() {
    let page = |audience, path: &str| {
        files(LINKS, audience)
            .into_iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path}"))
            .contents
    };
    let get = page(Audience::External, "src/operations/API_GetWidget.md");
    assert!(
        get.contains("<a href=\"../shapes/Widget.md\">Widget</a>"),
        "{get}"
    );
    assert!(
        get.contains("<a href=\"../operations/API_CreateWidget.md\">CreateWidget</a>"),
        "{get}"
    );
    // An operation's input member is on the operation's page.
    assert!(
        get.contains("<a href=\"../operations/API_GetWidget.md\">GetWidget.widgetId</a>"),
        "{get}"
    );
    assert!(
        get.contains("<a href=\"../shapes/WidgetError.md\">WidgetError</a>"),
        "{get}"
    );
    let index = page(Audience::External, "src/index.md");
    assert!(
        index.contains("<a href=\"operations/API_GetWidget.md\">the read call</a>"),
        "{index}"
    );
    assert!(
        index.contains("<a href=\"shapes/Color.md\">Color.RED</a>"),
        "{index}"
    );
    assert!(index.contains("<a href=\"index.md\">Gadget</a>"), "{index}");
    assert!(index.contains("Internal: Reveal and Secret."), "{index}");
    assert!(!index.contains("data-crucible-ref"), "{index}");
    let internal = page(Audience::Internal, "src/index.md");
    assert!(
        internal.contains("<a href=\"shapes/Secret.md\">Secret</a>"),
        "{internal}"
    );
    assert!(
        internal.contains("<a href=\"operations/API_Reveal.md\">Reveal</a>"),
        "{internal}"
    );
}

/// A package's service with the older releases it serves, as a build
/// lowers it (`crucible_lang::versions`), at `audience`.
fn versioned_files(dir: &Path, audience: Audience) -> Vec<crucible_codegen_docs::File> {
    let service = crucible_lang::load_versioned_service(dir).unwrap_or_else(|error| {
        panic!(
            "{}:
{error}",
            dir.display()
        )
    });
    generate(&project(&service, audience).unwrap()).unwrap()
}

/// Release history (decisions 77 and 78): every step kind across three
/// releases, two steps on internal API that only the internal reference
/// lists.
#[test]
fn release_history_matches_reviewed_goldens() {
    let dir = package_dir("version-history");
    golden_files(
        "history-external",
        versioned_files(&dir, Audience::External),
    );
    golden_files(
        "history-internal",
        versioned_files(&dir, Audience::Internal),
    );
}

/// The reference of release v1 of the e2e versioned service, an SDK line
/// (decision 79): v1's shapes and routes, and the header every request
/// sends at v1, while the releases still name v4 as current.
#[test]
fn an_older_releases_reference_documents_its_own_release() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../e2e/weather-service/versioned");
    let current = crucible_lang::load_versioned_service(&dir).unwrap_or_else(|e| panic!("{e}"));
    let line = crucible_ir::versions::line(&current, "v1").unwrap();
    let files = generate(&project(&line, Audience::External).unwrap()).unwrap();
    golden_files("versioned-v1-external", files.clone());
    let file = |path: &str| {
        files
            .iter()
            .find(|file| file.path == path)
            .unwrap_or_else(|| panic!("no {path}"))
            .contents
            .clone()
    };
    let index = file("src/index.md");
    assert!(
        index.contains("in a header: <code>Crucible&#45;Version&#58; v1</code>. This reference documents release <code>v1</code>; the current release is <code>v4</code>."),
        "{index}"
    );
    let llms = file("llms.txt");
    assert!(
        llms.contains("Send `Crucible-Version: v1` on every request"),
        "{llms}"
    );
    assert!(
        llms.contains("This reference documents release `v1`, which is supported until `2027-01-01`; the current release is `v4`, which has a reference of its own: [v4](../v4/llms.txt)."),
        "{llms}"
    );
    assert!(!llms.contains("Crucible-Version: v4"), "{llms}");
    let releases = file("src/releases.md");
    assert!(
        releases.contains(
            "The current release is <code>v4</code>. This reference documents release <code>v1</code>."
        ),
        "{releases}"
    );
    // Every request this reference shows names v1, at v1's route.
    for page in files
        .iter()
        .filter(|file| file.path.starts_with("src/operations/"))
    {
        assert!(
            !page.contents.contains("Crucible-Version: v4"),
            "{}: {}",
            page.path,
            page.contents
        );
    }
    let get_item = files
        .iter()
        .find(|file| file.path.starts_with("src/operations/") && file.path.contains("GetItem"))
        .expect("a GetItem page");
    assert!(
        get_item.contents.contains("GET /items/{id}"),
        "{}",
        get_item.contents
    );
    assert!(
        get_item.contents.contains("Crucible-Version: v1"),
        "{}",
        get_item.contents
    );
}

/// The e2e versioned service's directory, and a line of one of its
/// releases.
fn shop() -> crucible_ir::ir::Service {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../e2e/weather-service/versioned");
    crucible_lang::load_versioned_service(&dir).unwrap_or_else(|e| panic!("{e}"))
}

fn shop_line(release: &str) -> Vec<crucible_codegen_docs::File> {
    let line = crucible_ir::versions::line(&shop(), release).unwrap();
    generate(&project(&line, Audience::External).unwrap()).unwrap()
}

/// The reference of release v2 of the e2e versioned service: no sunset,
/// v2's names, CountItems restored, and v1's renames behind it.
#[test]
fn a_middle_releases_reference_matches_reviewed_goldens() {
    golden_files("versioned-v2-external", shop_line("v2"));
}

/// The notice on every page of an older release's reference (decision
/// 79): which release it documents, its sunset when it has one, and a link
/// to the current release's reference beside it, from each page's depth.
/// The current release's reference has none.
#[test]
fn every_page_of_an_older_releases_reference_names_its_release() {
    for (release, sunset) in [
        (
            "v1",
            ", which is supported until <code>2027&#45;01&#45;01</code>",
        ),
        ("v2", ""),
        ("v3", ""),
    ] {
        let files = shop_line(release);
        let mut pages = 0;
        for file in files
            .iter()
            .filter(|f| f.path.starts_with("src/") && f.path != "src/SUMMARY.md")
        {
            let depth = file.path.matches('/').count();
            let banner = format!(
                "> **An older release.** This reference documents release <code>{release}</code>{sunset}. The current release is <code>v4</code>: see [its reference]({}v4/index.html).\n\n",
                "../".repeat(depth + 1)
            );
            // Under the page's title.
            let (title, rest) = file.contents.split_once("\n\n").unwrap();
            assert!(title.starts_with("# "), "{}", file.path);
            assert!(
                rest.starts_with(&banner),
                "{}:\n{}",
                file.path,
                file.contents
            );
            pages += 1;
        }
        assert!(pages > 20, "{pages} pages");
        let summary = &files.iter().find(|f| f.path == "src/SUMMARY.md").unwrap();
        assert!(!summary.contents.contains("An older release"));
        // In one file at the root, the link is rebased onto the root.
        let full = &files.iter().find(|f| f.path == "llms-full.txt").unwrap();
        assert!(
            full.contents
                .contains("see [its reference](../v4/index.html)."),
            "{}",
            full.contents
        );
        assert!(!full.contents.contains("(v4/index.html)"));
    }
    for file in shop_line("v4")
        .iter()
        .filter(|f| !SITE_RUNTIME.contains(&f.path.as_str()))
    {
        assert!(!file.contents.contains("An older release"), "{}", file.path);
    }
}

/// A release's directory: the explorer computes the same names to link the
/// current reference (`tests/site`, "a release's directory is named as the
/// generator names it", holds the same table).
#[test]
fn a_releases_directory_is_its_escaped_name() {
    for (name, directory) in [
        ("v2", "v2"),
        ("2026-09-04", "2026-09-04"),
        ("1.0+build", "1.0+build"),
        (
            "2026<img/src=x>?#q:*|\"_",
            "2026_3Cimg_2Fsrc_3Dx_3E_3F_23q_3A_2A_7C_22_5F",
        ),
        ("con", "_63on"),
        ("Com1.x", "_43om1.x"),
        (".hidden.", "_2Ehidden_2E"),
        (
            "x](https://example.com)",
            "x_5D_28https_3A_2F_2Fexample.com_29",
        ),
        ("v3/../../evil`*", "v3_2F.._2F.._2Fevil_60_2A"),
    ] {
        assert_eq!(
            crucible_codegen_docs::release_directory(name),
            directory,
            "{name}"
        );
    }
}

/// The older release `service.versions[index]` renamed `name`: the entry,
/// and its projection's own `version`, which a line names itself by.
fn rename_release(service: &mut crucible_ir::ir::Service, index: usize, name: &str) {
    let version = &mut service.versions[index];
    version.name = name.into();
    version.service.version = name.into();
    version.service.api_version = Some(name.into());
}

/// A release name and a sunset date are model text: in the notice they
/// read back as written, open no tag or link, and the link to the current
/// reference is to its escaped directory beside this one.
#[test]
fn model_text_in_the_release_notice_is_inert() {
    let dir = package_dir("version-history");
    let mut current = crucible_lang::load_versioned_service(&dir).unwrap();
    let older = "<img src=x onerror=alert(1)>[v1](https://example.com)";
    let newest = "v3/../../evil`*";
    let sunset = "<b>2027</b>](javascript:alert(1))";
    rename_release(&mut current, 0, older);
    current.versions[0].sunset = Some(sunset.into());
    current.version = newest.into();
    current.api_version = Some(newest.into());
    let line = crucible_ir::versions::line(&current, older).unwrap();
    let files = generate(&project(&line, Audience::External).unwrap()).unwrap();
    let escaped_newest = crucible_codegen_docs::release_directory(newest);
    assert_eq!(escaped_newest, "v3_2F.._2F.._2Fevil_60_2A");
    let index = &files.iter().find(|f| f.path == "src/index.md").unwrap();
    let shown_index = shown(&index.contents);
    assert!(
        shown_index.contains(&format!(
            "This reference documents release {older}, which is supported until {sunset}. The current release is {newest}: see its reference."
        )),
        "{shown_index}"
    );
    for file in files
        .iter()
        .filter(|f| f.path.ends_with(".md") || f.path.ends_with(".txt"))
    {
        // Raw HTML a reader would run: only the generator's own tags. A
        // request's code block shows the name as typed, inert.
        for event in pulldown_cmark::Parser::new(&file.contents) {
            if let pulldown_cmark::Event::Html(html) | pulldown_cmark::Event::InlineHtml(html) =
                event
            {
                assert!(
                    !html.contains("<img") && !html.contains("<b>"),
                    "{html} in {}",
                    file.path
                );
            }
        }
        for destination in destinations(&file.contents) {
            assert!(
                !destination.contains("://") && !destination.starts_with("javascript"),
                "{destination} in {}",
                file.path
            );
            assert!(!destination.contains("evil`"), "{destination}");
        }
    }
    assert!(
        destinations(&index.contents).contains(&format!("../../{escaped_newest}/index.html")),
        "{}",
        index.contents
    );
    let llms = &files
        .iter()
        .find(|f| f.path == "llms.txt")
        .unwrap()
        .contents;
    assert!(
        llms.contains(&format!("(../{escaped_newest}/llms.txt)")),
        "{llms}"
    );
    // In code spans, as typed: inert.
    assert!(
        llms.contains("This reference documents release `<img src=x onerror=alert(1)>[v1](https://example.com)`, which is supported until `<b>2027</b>](javascript:alert(1))`;"),
        "{llms}"
    );
}

/// Every supported release's reference in one output (decision 79): each
/// release's directory holds exactly the reference generated for that
/// release alone, and the root an index of them, whatever release the
/// model given is a line of.
#[test]
fn every_releases_reference_sits_in_its_own_directory() {
    let current = shop();
    let options = crucible_codegen_docs::Options::default();
    let all =
        crucible_codegen_docs::generate_releases(&current, Audience::External, &options).unwrap();
    let mut expected = Vec::new();
    for release in ["v1", "v2", "v3", "v4"] {
        for file in shop_line(release) {
            expected.push((format!("{release}/{}", file.path), file.contents));
        }
    }
    let index = |path: &str| {
        all.iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path}"))
            .contents
            .clone()
    };
    let html = index("index.html");
    let llms = index("llms.txt");
    expected.push(("index.html".into(), html.clone()));
    expected.push(("llms.txt".into(), llms.clone()));
    expected.sort();
    let found: Vec<(String, String)> = all
        .iter()
        .map(|f| (f.path.clone(), f.contents.clone()))
        .collect();
    assert_eq!(found, expected);
    for release in ["v1", "v2", "v3", "v4"] {
        assert!(
            html.contains(&format!(
                "<li><a href=\"{release}/index.html\"><code>{release}</code></a>: "
            )),
            "{html}"
        );
        assert!(
            llms.contains(&format!(
                "- [{release}]({release}/llms.txt): release `{release}`, "
            )),
            "{llms}"
        );
    }
    assert!(html.contains("<code>v1</code></a>: supported until <code>2027-01-01</code></li>"));
    assert!(html.contains("<code>v4</code></a>: the current release</li>"));
    assert!(html.contains("<link rel=\"stylesheet\" href=\"v4/explorer.css\">"));
    // No script: it works under the reference's own CSP.
    assert!(!html.contains("<script"), "{html}");
    // A line gives the same output: its current model's releases.
    let line = crucible_ir::versions::line(&current, "v2").unwrap();
    let from_line =
        crucible_codegen_docs::generate_releases(&line, Audience::External, &options).unwrap();
    assert_eq!(
        from_line
            .iter()
            .map(|f| (&f.path, &f.contents))
            .collect::<Vec<_>>(),
        all.iter()
            .map(|f| (&f.path, &f.contents))
            .collect::<Vec<_>>()
    );
    // A service with one release is one directory.
    let single = crucible_codegen_docs::generate_releases(
        &documented(WEATHER),
        Audience::External,
        &options,
    )
    .unwrap();
    let roots: BTreeSet<&str> = single
        .iter()
        .map(|f| f.path.split('/').next().unwrap())
        .collect();
    assert_eq!(roots.len(), 3, "{roots:?}");
    assert!(roots.contains("index.html") && roots.contains("llms.txt"));
}

/// The index of every release escapes model text: the service's title and
/// release names in HTML text and in `llms.txt`, and a release's directory
/// in its links; releases whose directories case folds would merge are
/// refused before anything is written.
#[test]
fn the_index_of_every_release_escapes_model_text() {
    let dir = package_dir("version-history");
    let mut current = crucible_lang::load_versioned_service(&dir).unwrap();
    current.title = Some("<script>alert(1)</script> & [x](https://example.com)".into());
    rename_release(&mut current, 0, "<b>one</b>");
    current.versions[0].sunset = Some("\"><img>".into());
    let options = crucible_codegen_docs::Options::default();
    let all =
        crucible_codegen_docs::generate_releases(&current, Audience::Internal, &options).unwrap();
    let file = |path: &str| {
        all.iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path}"))
            .contents
            .clone()
    };
    let html = file("index.html");
    assert!(!html.contains("<script>alert"), "{html}");
    assert!(!html.contains("<b>one"), "{html}");
    assert!(!html.contains("\"><img>"), "{html}");
    assert!(html.contains("<title>&lt;script&gt;alert(1)&lt;/script&gt; &amp; [x](https://example.com) API reference</title>"), "{html}");
    assert!(
        html.contains("<li><a href=\"_3Cb_3Eone_3C_2Fb_3E/index.html\"><code>&lt;b&gt;one&lt;/b&gt;</code></a>: supported until <code>&quot;&gt;&lt;img&gt;</code></li>"),
        "{html}"
    );
    let llms = file("llms.txt");
    assert!(
        llms.starts_with(
            r"# &lt;script&gt;alert\(1\)&lt;\/script&gt; &amp; \[x\]\(https\:\/\/example\.com\)"
        ),
        "{llms}"
    );
    assert!(
        llms.contains(r#"- [&lt;b&gt;one&lt;\/b&gt;](_3Cb_3Eone_3C_2Fb_3E/llms.txt): release `<b>one</b>`, supported until `"><img>`"#),
        "{llms}"
    );
    for destination in destinations(&llms) {
        assert!(!destination.contains("://"), "{destination}");
    }
    assert!(
        all.iter()
            .any(|f| f.path.starts_with("_3Cb_3Eone_3C_2Fb_3E/src/"))
    );
    // `V3` and `v3` would share a directory where case folds.
    let mut folded = crucible_lang::load_versioned_service(&dir).unwrap();
    rename_release(&mut folded, 0, "V3");
    let error = crucible_codegen_docs::generate_releases(&folded, Audience::External, &options)
        .unwrap_err()
        .to_string();
    assert!(error.contains("docs.filename"), "{error}");
}

/// The e2e versioned service's reference: renames, a reshape, a removal
/// and a sunset, as its build serves them.
#[test]
fn the_versioned_service_reference_matches_reviewed_goldens() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../e2e/weather-service/versioned");
    golden_files(
        "versioned-external",
        versioned_files(&dir, Audience::External),
    );
}

#[test]
fn release_history_leaves_internal_steps_out_of_the_external_reference() {
    let dir = package_dir("version-history");
    let changes = |audience| -> Value {
        let files = versioned_files(&dir, audience);
        let file = files.iter().find(|f| f.path == "changes.json").unwrap();
        serde_json::from_str(&file.contents).unwrap()
    };
    let kinds = |document: &Value| -> Vec<Vec<String>> {
        document["versions"]["releases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|release| {
                release["changes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|change| change["kind"].as_str().unwrap().to_owned())
                    .collect()
            })
            .collect()
    };
    let external = changes(Audience::External);
    let internal = changes(Audience::Internal);
    assert_eq!(external["format"], "crucible.changes/1");
    assert_eq!(
        kinds(&external),
        [
            vec![],
            vec![
                "rename",
                "rename",
                "require",
                "addEnumValue",
                "reshape",
                "retype",
                "behavior"
            ],
            vec![
                "move",
                "addError",
                "addError",
                "require",
                "retype",
                "removeOperation"
            ],
        ]
    );
    assert_eq!(kinds(&internal)[1].len(), 10);
    assert_eq!(kinds(&internal)[2].len(), 9);
    // No external file names internal API: not the internal operation, the
    // internal value or error a step added, the internal member a reshape
    // replaced, nor the members internal in v1 that a rename and a retype
    // made public (externally they arrived in v2).
    let files = versioned_files(&dir, Audience::External);
    // The renderer is the same for every service, and says "placeholder".
    for file in files
        .iter()
        .filter(|f| !SITE_RUNTIME.contains(&f.path.as_str()))
    {
        let text = file.contents.to_lowercase();
        for internal in [
            "audit",
            "secret",
            "conflict",
            "depth",
            "hint",
            "holder",
            "priority</code></a> to",
        ] {
            assert!(!text.contains(internal), "{internal} in {}", file.path);
        }
    }
    let internal_files = versioned_files(&dir, Audience::Internal);
    assert!(internal_files.iter().any(|f| f.contents.contains("SECRET")));
    // changes.json holds describe.json's `versions`.
    let describe: Value = serde_json::from_str(
        &files
            .iter()
            .find(|f| f.path == "describe.json")
            .unwrap()
            .contents,
    )
    .unwrap();
    assert_eq!(describe["versions"], external["versions"]);
}

/// A release name is any visible ASCII; its page's file is escaped so it
/// is one file name everywhere and a link carries it unchanged.
#[test]
fn release_names_become_safe_file_names() {
    let dir = package_dir("version-history");
    let mut service = crucible_lang::load_versioned_service(&dir).unwrap();
    let hostile = "2026<img/src=x>?#q:*|\"_";
    service.version = hostile.into();
    service.api_version = Some(hostile.into());
    service.versions[0].name = "con".into();
    let files = generate(&project(&service, Audience::External).unwrap()).unwrap();
    let pages: Vec<&str> = files
        .iter()
        .map(|f| f.path.as_str())
        .filter(|p| p.starts_with("src/releases/"))
        .collect();
    assert_eq!(
        pages,
        [
            "src/releases/2026_3Cimg_2Fsrc_3Dx_3E_3F_23q_3A_2A_7C_22_5F.md",
            "src/releases/_63on.md",
            "src/releases/v2.md",
        ]
    );
    for file in &files {
        assert!(
            file.path
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-+/".contains(&b)),
            "{}",
            file.path
        );
    }
    let summary = &files
        .iter()
        .find(|f| f.path == "src/SUMMARY.md")
        .unwrap()
        .contents;
    assert!(summary.contains("(releases/_63on.md)"), "{summary}");
    // A reader sees the name's `<img ..>` as text, never as HTML.
    assert!(
        !pulldown_cmark::Parser::new(summary).any(|event| matches!(
            event,
            pulldown_cmark::Event::InlineHtml(_) | pulldown_cmark::Event::Html(_)
        )),
        "{summary}"
    );
    assert!(shown(summary).contains(hostile), "{summary}");
}

#[test]
fn release_pages_that_share_a_file_where_case_folds_are_refused() {
    let dir = package_dir("version-history");
    let mut service = crucible_lang::load_versioned_service(&dir).unwrap();
    service.versions[0].name = "V3".into();
    let error = generate(&project(&service, Audience::External).unwrap()).unwrap_err();
    assert!(
        error.to_string().contains("docs.filename: release \"v3\""),
        "{error}"
    );
}

/// One release has no changelog to show: no release pages, but the JSON
/// still carries `versions`.
#[test]
fn a_single_release_has_no_release_pages() {
    let files = files(WEATHER, Audience::External);
    assert!(!files.iter().any(|f| f.path.contains("releases")));
    let describe: Value = serde_json::from_str(
        &files
            .iter()
            .find(|f| f.path == "describe.json")
            .unwrap()
            .contents,
    )
    .unwrap();
    assert_eq!(
        describe["versions"]["releases"].as_array().unwrap().len(),
        1
    );
}

#[test]
fn rpc_metadata_examples_and_shapes_match_reviewed_goldens() {
    golden("catalog-external", CATALOG, Audience::External);
    golden("catalog-internal", CATALOG, Audience::Internal);
}

/// The typed permissions fixture, a `.cru` package; `edit` rewrites its
/// source first.
fn leyline_with(edit: impl Fn(String) -> String) -> crucible_ir::ir::Service {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/permissions");
    let mut package = crucible_lang::load_package(&dir).unwrap();
    for file in &mut package.files {
        file.text = edit(std::mem::take(&mut file.text));
    }
    let compiled = crucible_lang::compile(&package, None);
    let rendered = compiled.render();
    compiled
        .service
        .unwrap_or_else(|| panic!("the fixture compiles:\n{rendered}"))
}

fn leyline() -> crucible_ir::ir::Service {
    leyline_with(|text| text)
}

/// Typed permissions (docs/permissions.md §6.3): a Permissions page with a
/// table per tier, an Authorization section per operation, and the
/// registry and requirements in `describe.json`. Internal permissions and
/// pending reasons only in the server audience.
#[test]
fn permissions_match_reviewed_goldens() {
    let service = leyline();
    let generated = |audience| generate(&project(&service, audience).unwrap()).unwrap();
    golden_files("permissions-external", generated(Audience::External));
    golden_files("permissions-internal", generated(Audience::Internal));
    let external = generated(Audience::External);
    for file in &external {
        assert!(
            !file.contents.contains("organization:read"),
            "{}",
            file.path
        );
        assert!(
            !file.contents.contains("history module moves"),
            "{}",
            file.path
        );
    }
    let internal = generated(Audience::Internal);
    let page = internal
        .iter()
        .find(|f| f.path == "src/permissions.md")
        .unwrap();
    assert!(page.contents.contains("### Internal"), "{}", page.contents);
}

#[test]
fn site_files_match_reviewed_goldens() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/site");
    let generated = files(WEATHER, Audience::External);
    let mut found = BTreeSet::new();
    for file in generated
        .iter()
        .filter(|f| SITE_RUNTIME.contains(&f.path.as_str()))
    {
        found.insert(file.path.clone());
        let path = root.join(&file.path);
        if std::env::var_os("CRUCIBLE_BLESS").is_some() {
            fs::create_dir_all(&root).unwrap();
            fs::write(&path, &file.contents).unwrap();
        }
        assert_eq!(
            fs::read_to_string(&path).unwrap().replace("\r\n", "\n"),
            file.contents,
            "{}",
            path.display()
        );
    }
    assert_eq!(found.len(), SITE_RUNTIME.len());
    let on_disk: BTreeSet<_> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(on_disk, found, "stale golden files");
}

#[test]
fn site_is_identical_across_services_and_loads_its_own_description() {
    let weather = files(WEATHER, Audience::External);
    let catalog = files(CATALOG, Audience::Internal);
    for name in SITE_RUNTIME {
        let pick = |files: &[crucible_codegen_docs::File]| {
            files
                .iter()
                .find(|f| f.path == name)
                .unwrap()
                .contents
                .clone()
        };
        assert_eq!(
            pick(&weather),
            pick(&catalog),
            "{name} must not be baked per service"
        );
    }
    for generated in [&weather, &catalog] {
        let get = |name: &str| &generated.iter().find(|f| f.path == name).unwrap().contents;
        // Parse JSON data so file:// and fetch preserve arbitrary map keys alike.
        assert_eq!(
            get("describe.js"),
            &format!(
                "globalThis.CRUCIBLE_DESCRIPTION = JSON.parse({});\n",
                serde_json::to_string(get("describe.json").trim_end()).unwrap()
            )
        );
        // A strict CSP (script-src 'self'; style-src 'self') forbids inline code.
        let html = get("index.html");
        assert!(
            !html.contains("style=") && !html.contains("<style"),
            "inline style"
        );
        assert!(
            !html.contains("javascript:") && !html.contains(" on"),
            "inline handler"
        );
        let scripts = html.matches("<script").count();
        assert_eq!(
            scripts,
            html.matches("<script src=").count(),
            "inline script"
        );
        assert_eq!(scripts, 2);
        assert!(get("explorer.js").len() + get("explorer.css").len() < 100 * 1024);
    }
}

#[test]
fn audience_filters_values_and_badges_are_visible_at_every_level() {
    let external = files(WEATHER, Audience::External);
    let internal = files(WEATHER, Audience::Internal);
    assert!(
        !external
            .iter()
            .any(|f| f.contents.contains("INTERNAL_EXAMPLE_SECRET"))
    );
    assert!(
        internal
            .iter()
            .any(|f| f.contents.contains("INTERNAL_EXAMPLE_SECRET"))
    );
    assert!(
        internal
            .iter()
            .any(|f| f.path.ends_with("API_Status.md") && f.contents.contains("Internal API"))
    );
    assert!(
        internal.iter().any(|f| f.path.ends_with("CitySummary.md")
            && f.contents.contains("<strong>Internal</strong>"))
    );
    let files = files(CATALOG, Audience::External);
    let describe: Value = serde_json::from_str(
        &files
            .iter()
            .find(|f| f.path == "describe.json")
            .unwrap()
            .contents,
    )
    .unwrap();
    assert_eq!(
        describe["operations"]["example.docs#WriteItem"]["examples"][0]["input"]["body"]["item"]["amount"]
            .to_string(),
        "1234567890123456789"
    );
    assert!(
        describe["operations"]["example.docs#WriteItem"]["inputSchema"]["$defs"]["crucible.api.BigInteger"]
            ["type"]
            == "integer"
    );
    assert!(
        describe["service"]["documentation"]["html"]
            .as_str()
            .unwrap()
            .contains("&lt;script&gt;")
    );
}

#[test]
fn all_links_point_to_generated_pages() {
    let leyline = generate(&project(&leyline(), Audience::Internal).unwrap()).unwrap();
    for files in [
        files(WEATHER, Audience::Internal),
        files(CATALOG, Audience::Internal),
        files(LINKS, Audience::External),
        files(LINKS, Audience::Internal),
        leyline,
    ] {
        let paths: BTreeSet<_> = files.iter().map(|f| f.path.as_str()).collect();
        for file in files
            .iter()
            .filter(|f| f.path.ends_with(".md") || f.path.ends_with(".txt"))
        {
            // Markdown links, and the anchors doc links became.
            let markdown = file.contents.split("](").skip(1).map(|s| (s, ')'));
            let anchors = file.contents.split("<a href=\"").skip(1).map(|s| (s, '"'));
            for (segment, end) in markdown.chain(anchors) {
                let Some((target, _)) = segment.split_once(end) else {
                    continue;
                };
                if !target.ends_with(".md") {
                    continue;
                }
                let mut components: Vec<_> = file.path.split('/').collect();
                components.pop();
                for component in target.split('/') {
                    if component == ".." {
                        components.pop();
                    } else {
                        components.push(component);
                    }
                }
                assert!(
                    paths.contains(components.join("/").as_str()),
                    "{} -> {target}",
                    file.path
                );
            }
        }
    }
}

#[test]
fn raw_payload_syntax_uses_the_resolved_content_type_once() {
    for file in files(WEATHER, Audience::External) {
        if file.path.ends_with("API_GetCityPhoto.md") || file.path.ends_with("API_PutCityPhoto.md")
        {
            // Once in the syntax, and once in the generated sample.
            let (syntax, sample) = file.contents.split_once("## Generated sample").unwrap();
            assert_eq!(syntax.matches("Content-Type: image/jpeg").count(), 1);
            assert_eq!(sample.matches("Content-Type: image/jpeg").count(), 1);
        }
    }
}

#[test]
fn model_file_order_does_not_change_description_or_pages() {
    let reordered = {
        let mut package = crucible_lang::load_package(&package_dir(CATALOG)).unwrap();
        package.files.reverse();
        let compiled = crucible_lang::compile(&package, None);
        let service = compiled.service.expect("the package compiles");
        generate(&project(&service, Audience::External).unwrap()).unwrap()
    };
    let original = files(CATALOG, Audience::External);
    assert_eq!(
        original
            .iter()
            .map(|f| (&f.path, &f.contents))
            .collect::<Vec<_>>(),
        reordered
            .iter()
            .map(|f| (&f.path, &f.contents))
            .collect::<Vec<_>>()
    );
}

#[test]
fn missing_targets_are_collected() {
    let mut service = documented(CATALOG);
    service
        .shapes
        .remove(&"example.docs#Content".parse().unwrap());
    let error = generate(&service).unwrap_err().to_string();
    assert!(error.matches("describe.target").count() >= 2, "{error}");
}

#[test]
fn documentation_requires_resource_metadata_and_detects_filename_collisions() {
    // An operation names its resource, so the reference needs the
    // resource's metadata, which the lowering carries.
    let mut weather = documented(WEATHER);
    assert!(generate(&weather).is_ok());
    weather.resources.clear();
    assert!(
        generate(&weather)
            .unwrap_err()
            .to_string()
            .contains("describe.resources")
    );
    let mut service = documented(CATALOG);
    let mut duplicate = service.operations[0].clone();
    duplicate.id = "example.docs#listitems".parse().unwrap();
    duplicate.resource = None;
    service.operations.push(duplicate);
    assert!(
        generate(&service)
            .unwrap_err()
            .to_string()
            .contains("docs.filename")
    );
}

#[test]
fn file_description_preserves_arbitrary_example_keys() {
    let mut package = crucible_lang::load_package(&package_dir(WEATHER)).unwrap();
    for file in &mut package.files {
        file.text = file
            .text
            .replace("\r\n", "\n")
            .replace(
                "        location?: Location\n        @token",
                "        location?: Location\n        metadata?: document\n        @token",
            )
            .replace(
                "input { name: \"Lisbon\", location:",
                "input { name: \"Lisbon\", metadata: { \"__proto__\": { example: true } }, location:",
            );
    }
    let compiled = crucible_lang::compile(&package, None);
    let rendered = compiled.render();
    let service = compiled
        .service
        .unwrap_or_else(|| panic!("the package compiles:\n{rendered}"));
    let generated = generate(&project(&service, Audience::Internal).unwrap()).unwrap();
    let script = &generated
        .iter()
        .find(|f| f.path == "describe.js")
        .unwrap()
        .contents;
    let json = &generated
        .iter()
        .find(|f| f.path == "describe.json")
        .unwrap()
        .contents;
    assert!(json.contains("__proto__"));
    let encoded = script
        .strip_prefix("globalThis.CRUCIBLE_DESCRIPTION = JSON.parse(")
        .unwrap()
        .strip_suffix(");\n")
        .unwrap();
    let decoded: String = serde_json::from_str(encoded).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&decoded).unwrap(),
        serde_json::from_str::<Value>(json).unwrap()
    );
}

/// Generated samples, with the placeholders a sample cannot avoid named on
/// their page as the static reference names them: `tests/models/samples`.
#[test]
fn samples_match_reviewed_goldens() {
    golden("samples-external", "samples", Audience::External);
    let page = files("samples", Audience::External)
        .into_iter()
        .find(|f| f.path == "src/operations/API_Check.md")
        .expect("the Check page");
    assert!(
        page.contents.contains(
            "Placeholders that do not meet their constraints: <code>input.bounded</code>."
        )
    );
}

/// Every link destination a generated Markdown page has, as a Markdown
/// reader finds them.
fn destinations(markdown: &str) -> Vec<String> {
    pulldown_cmark::Parser::new(markdown)
        .filter_map(|event| match event {
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link { dest_url, .. }) => {
                Some(dest_url.to_string())
            }
            _ => None,
        })
        .collect()
}

/// A release name is model text: Markdown link syntax in it stays the text
/// of its own link, in the sidebar and on every page, never a link of its
/// own.
#[test]
fn a_release_name_cannot_redirect_a_link() {
    let dir = package_dir("version-history");
    let mut service = crucible_lang::load_versioned_service(&dir).unwrap();
    let hostile = "x](https://example.com)";
    service.version = hostile.into();
    service.api_version = Some(hostile.into());
    service.versions[1].name = "[y](https://example.org)`\\".into();
    let files = generate(&project(&service, Audience::External).unwrap()).unwrap();
    let summary = &files
        .iter()
        .find(|f| f.path == "src/SUMMARY.md")
        .unwrap()
        .contents;
    assert!(
        summary.contains(
            r"[x\]\(https\:\/\/example\.com\)](releases/x_5D_28https_3A_2F_2Fexample.com_29.md)"
        ),
        "{summary}"
    );
    let mut checked = 0;
    for file in files
        .iter()
        .filter(|f| f.path.ends_with(".md") || f.path.ends_with(".txt"))
    {
        for destination in destinations(&file.contents) {
            assert!(
                !destination.contains("://"),
                "{destination} in {}",
                file.path
            );
            checked += 1;
        }
    }
    assert!(checked > 50, "{checked} links");
    // The release pages still link each other and the pages they name.
    let page = &files
        .iter()
        .find(|f| f.path == "src/releases/x_5D_28https_3A_2F_2Fexample.com_29.md")
        .unwrap()
        .contents;
    assert!(
        destinations(page)
            .contains(&"../releases/_5By_5D_28https_3A_2F_2Fexample.org_29_60_5C.md".to_owned()),
        "{page}"
    );
}

/// An older operation the reference shows (a removed one) names no member
/// its release's external view left out, in its bindings or anywhere else:
/// a public member whose target is internal goes, and with it every
/// binding that named it.
#[test]
fn an_older_operation_names_no_member_its_release_hid() {
    use crucible_ir::ShapeId;
    use crucible_ir::ir::Shape;
    let dir = package_dir("version-history");
    let mut service = crucible_lang::load_versioned_service(&dir).unwrap();
    let v2 = &mut service.versions[1].release;
    let output: ShapeId = "example.history#CountItemsOutput".parse().unwrap();
    let ledger: ShapeId = "example.history#Ledger".parse().unwrap();
    let mut hidden = v2.shapes[&output].clone();
    if let Shape::Structure(s) = &mut hidden {
        s.internal = true;
    }
    v2.shapes.insert(ledger.clone(), hidden);
    let Some(Shape::Structure(s)) = v2.shapes.get_mut(&output) else {
        panic!("CountItemsOutput");
    };
    let mut member = s.members[0].clone();
    member.name = "ledger".into();
    member.target = ledger;
    member.required = false;
    s.members.push(member);
    let op = v2
        .operations
        .iter_mut()
        .find(|op| op.name() == "CountItems")
        .unwrap();
    op.response.body.push("ledger".into());
    let external = generate(&project(&service, Audience::External).unwrap()).unwrap();
    for file in &external {
        assert!(
            !file.contents.to_lowercase().contains("ledger"),
            "ledger in {}",
            file.path
        );
    }
    let internal = generate(&project(&service, Audience::Internal).unwrap()).unwrap();
    let changes = &internal
        .iter()
        .find(|f| f.path == "changes.json")
        .unwrap()
        .contents;
    assert!(changes.contains("ledger"), "the internal view keeps it");
}

/// What a Markdown reader shows of a page: its text, code and raw-HTML
/// content, character references read.
fn shown(markdown: &str) -> String {
    pulldown_cmark::Parser::new(markdown)
        .filter_map(|event| match event {
            pulldown_cmark::Event::Text(text) | pulldown_cmark::Event::Code(text) => {
                Some(text.to_string())
            }
            _ => None,
        })
        .collect()
}

/// Release names with Markdown syntax in them (emphasis, brackets, a
/// backslash) read back exactly as written wherever the book names them,
/// in link labels and in code, and no link leaves the book.
#[test]
fn release_names_read_back_as_written() {
    let dir = package_dir("version-history");
    // Each name is also the middle release once, which the current page
    // names in code ("Clients of <code>..</code>").
    for names in [
        ["*beta*", "[beta]", "_x_"],
        ["[beta]", "*beta*", r"a\b"],
        [r"a\b", "_x_", "*beta*"],
        ["_x_", r"a\b", "[beta](https://example.com)"],
        ["v1", "[y](https://example.org)", "<b>&amp;"],
        ["`c`", "<b>&amp;", "v3"],
    ] {
        let mut service = crucible_lang::load_versioned_service(&dir).unwrap();
        service.versions[0].name = names[0].into();
        service.versions[1].name = names[1].into();
        service.version = names[2].into();
        service.api_version = Some(names[2].into());
        let files = generate(&project(&service, Audience::External).unwrap()).unwrap();
        let page = |path: &str| {
            &files
                .iter()
                .find(|f| f.path == path)
                .unwrap_or_else(|| panic!("{path}"))
                .contents
        };
        for path in ["src/SUMMARY.md", "src/releases.md"] {
            let text = shown(page(path));
            for name in names {
                assert!(text.contains(name), "{name} in {path}: {text}");
            }
        }
        // The current release's page names the release before it in a
        // link, and in code where it says what its clients see.
        let current = files
            .iter()
            .find(|f| {
                f.path.starts_with("src/releases/")
                    && shown(&f.contents).contains(&format!("Release {}", names[2]))
            })
            .unwrap_or_else(|| panic!("the page of {}", names[2]));
        let text = shown(&current.contents);
        assert!(text.contains(&format!("Follows {}.", names[1])), "{text}");
        assert!(
            text.contains(&format!("Clients of {} ", names[1])),
            "{text}"
        );
        assert!(shown(page("src/index.md")).contains(names[2]));
        // Dates on a data type's page and in its table cells.
        let size = shown(page("src/shapes/Size.md"));
        assert!(
            size.contains(&format!("Changed in {}.", names[1])),
            "{size}"
        );
        assert!(
            size.contains(&format!("Clients of {} do not see it.", names[0])),
            "{size}"
        );
        for file in files
            .iter()
            .filter(|f| f.path.ends_with(".md") || f.path.ends_with(".txt"))
        {
            for destination in destinations(&file.contents) {
                assert!(
                    !destination.contains("://"),
                    "{destination} in {}",
                    file.path
                );
            }
        }
    }
}

/// `llms.txt` links every page, each to a generated file, and
/// `llms-full.txt` holds every page, each under a comment naming it.
#[test]
fn llms_txt_indexes_every_page_and_llms_full_holds_them() {
    let cases = [
        files(WEATHER, Audience::External),
        files(WEATHER, Audience::Internal),
        files(CATALOG, Audience::Internal),
        generate(&project(&leyline(), Audience::External).unwrap()).unwrap(),
        versioned_files(&package_dir("version-history"), Audience::Internal),
    ];
    for files in cases {
        let file = |path: &str| {
            files
                .iter()
                .find(|f| f.path == path)
                .unwrap_or_else(|| panic!("{path}"))
                .contents
                .clone()
        };
        let index = file("llms.txt");
        assert!(index.starts_with("# "), "{index}");
        assert!(index.contains("\n\n> "), "a summary: {index}");
        let linked: BTreeSet<String> = destinations(&index).into_iter().collect();
        for destination in &linked {
            assert!(
                files.iter().any(|f| &f.path == destination),
                "{destination} is not generated: {index}"
            );
        }
        let full = file("llms-full.txt");
        let sections = llms_full_pages(&full);
        let pages: Vec<&crucible_codegen_docs::File> = files
            .iter()
            .filter(|f| f.path.starts_with("src/") && f.path != "src/SUMMARY.md")
            .collect();
        assert_eq!(sections.len(), pages.len(), "one section per page");
        for page in pages {
            assert!(linked.contains(&page.path), "llms.txt links {}", page.path);
            let (_, section) = sections
                .iter()
                .find(|(path, _)| path == &page.path)
                .unwrap_or_else(|| panic!("llms-full.txt holds {}", page.path));
            // The same text, links aside.
            assert_eq!(
                section.lines().next(),
                page.contents.lines().next(),
                "{}",
                page.path
            );
            assert_eq!(
                destinations(section).len(),
                destinations(&page.contents).len(),
                "{}",
                page.path
            );
        }
        // Every page's link resolves from the root, where llms-full.txt is.
        for destination in destinations(&full) {
            let path = destination.split('#').next().unwrap();
            assert!(
                destination.contains(':') || files.iter().any(|f| f.path == path),
                "{destination} from llms-full.txt"
            );
        }
        for href in hrefs(&full) {
            assert!(
                href.contains(':') || files.iter().any(|f| f.path == href),
                "{href} from llms-full.txt"
            );
        }
    }
}

/// llms-full.txt's pages, by the boundary comment before each: a run of
/// `=` longer than any in the pages, the file, and the run again.
fn llms_full_pages(full: &str) -> Vec<(String, String)> {
    let first = full.lines().next().unwrap();
    let rule = first
        .strip_prefix("<!-- ")
        .and_then(|rest| rest.split(' ').next())
        .unwrap();
    assert!(rule.len() > 2 && rule.chars().all(|c| c == '='), "{first}");
    let mut out: Vec<(String, String)> = Vec::new();
    for line in full.lines() {
        if let Some(path) = line
            .strip_prefix(&format!("<!-- {rule} "))
            .and_then(|rest| rest.strip_suffix(&format!(" {rule} -->")))
        {
            out.push((path.to_owned(), String::new()));
        } else if let Some((_, text)) = out.last_mut() {
            text.push_str(line);
            text.push('\n');
        }
    }
    for (_, text) in &mut out {
        *text = text.trim_start().to_owned();
    }
    out
}

/// The `href`s of raw HTML anchors in Markdown.
fn hrefs(markdown: &str) -> Vec<String> {
    pulldown_cmark::Parser::new(markdown)
        .filter_map(|event| match event {
            pulldown_cmark::Event::Html(text) | pulldown_cmark::Event::InlineHtml(text) => {
                Some(text.to_string())
            }
            _ => None,
        })
        .flat_map(|html| {
            html.match_indices("href=\"")
                .filter_map(|(at, _)| {
                    let rest = &html[at + 6..];
                    rest.find('"').map(|end| rest[..end].to_owned())
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Every operation page says how to call it on its own: the credential
/// and release headers in the request syntax, and each error's code.
#[test]
fn operation_pages_carry_the_headers_and_error_codes_a_caller_sends_and_reads() {
    let files = files(WEATHER, Audience::External);
    let page = |path: &str| {
        files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path}"))
            .contents
            .clone()
    };
    let get = page("src/operations/API_GetCity.md");
    assert!(
        get.contains(
            "GET /cities/{cityId}?at={at} HTTP/1.1\nAuthorization: Bearer {token}\nCrucible-Version: 2026-10-01\n"
        ),
        "{get}"
    );
    assert!(
        get.contains("| <code>NotFoundError</code> | 404 |"),
        "{get}"
    );
    assert!(
        get.contains("<code>X-Crucible-Error</code> header"),
        "{get}"
    );
    let create = page("src/operations/API_CreateCity.md");
    assert!(
        create.contains("Content-Type: application/json\nAuthorization: Bearer {token}\n"),
        "{create}"
    );
}

/// An operation's schema rebuilt from a shared description: its root
/// definition with every definition it reaches, as `explorer.js` does.
fn rebuilt(description: &Value, schema: &Value) -> Value {
    let Some(name) = schema["$ref"]
        .as_str()
        .and_then(|r| r.strip_prefix("#/$defs/"))
    else {
        return schema.clone();
    };
    let defs = &description["schemas"]["$defs"];
    let mut reached = BTreeSet::new();
    let mut pending = vec![name.to_owned()];
    fn refs(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::Object(object) => {
                for (key, value) in object {
                    match (key.as_str(), value.as_str()) {
                        ("$ref", Some(target)) => {
                            if let Some(name) = target.strip_prefix("#/$defs/") {
                                out.push(name.to_owned());
                            }
                        }
                        _ => refs(value, out),
                    }
                }
            }
            Value::Array(values) => values.iter().for_each(|v| refs(v, out)),
            _ => {}
        }
    }
    while let Some(name) = pending.pop() {
        if reached.insert(name.clone()) {
            refs(&defs[&name], &mut pending);
        }
    }
    let mut out = defs[name].clone();
    out["$defs"] = Value::Object(
        reached
            .into_iter()
            .map(|name| (name.clone(), defs[&name].clone()))
            .collect(),
    );
    out
}

/// An inline schema with only the definitions its root (named by the
/// shared form's `$ref`) references, directly or through another: what a
/// shared schema rebuilds to. An inline schema also lists definitions a
/// member's own constraints replaced, which nothing references.
fn referenced(inline: &Value, shared: &Value) -> Value {
    let Some(name) = shared["$ref"]
        .as_str()
        .and_then(|r| r.strip_prefix("#/$defs/"))
    else {
        return inline.clone();
    };
    let description = serde_json::json!({ "schemas": { "$defs": inline["$defs"] } });
    let mut root = inline.clone();
    root.as_object_mut().unwrap().remove("$defs");
    assert_eq!(
        root, inline["$defs"][name],
        "the root is its shape's definition"
    );
    rebuilt(&description, shared)
}

/// `Schemas::Shared` and `Schemas::Omit` change only where the operation
/// schemas are: shared, each rebuilds to the inline schema exactly, and
/// every other file is the same in every mode.
#[test]
fn shared_and_omitted_schemas_keep_the_contract_and_rebuild_the_inline_ones() {
    use crucible_codegen_docs::{Options, Schemas, generate_with};
    let services = [
        (
            "weather",
            project(&documented(WEATHER), Audience::Internal).unwrap(),
        ),
        (
            "catalog",
            project(&documented(CATALOG), Audience::Internal).unwrap(),
        ),
        (
            "permissions",
            project(&leyline(), Audience::External).unwrap(),
        ),
    ];
    for (name, service) in services {
        let with = |schemas| {
            let mut options = Options::default();
            options.schemas = schemas;
            generate_with(&service, &options).unwrap()
        };
        let describe = |files: &[crucible_codegen_docs::File]| -> Value {
            let file = files.iter().find(|f| f.path == "describe.json").unwrap();
            serde_json::from_str(&file.contents).unwrap()
        };
        let inline_files = with(Schemas::Inline);
        let inline = describe(&inline_files);
        assert!(
            inline.get("schemas").is_none(),
            "{name}: inline is the absent mode"
        );
        for mode in [Schemas::Shared, Schemas::Omit] {
            let files = with(mode);
            // Only the description (and its script) differ.
            for (a, b) in inline_files.iter().zip(&files) {
                assert_eq!(a.path, b.path);
                if !["describe.json", "describe.js"].contains(&a.path.as_str()) {
                    assert_eq!(a.contents, b.contents, "{name}: {}", a.path);
                }
            }
            let mut description = describe(&files);
            for (id, op) in inline["operations"].as_object().unwrap() {
                for key in ["inputSchema", "outputSchema"] {
                    let schema = &description["operations"][id][key];
                    match mode {
                        Schemas::Shared => {
                            assert_eq!(
                                rebuilt(&description, schema),
                                referenced(&op[key], schema),
                                "{name}: {id} {key}"
                            );
                        }
                        _ => assert!(schema.is_null(), "{name}: {id} {key}"),
                    }
                }
            }
            // The mode is explicit.
            match mode {
                Schemas::Omit => {
                    assert_eq!(
                        description["schemas"],
                        serde_json::json!({"mode": "omit"}),
                        "{name}"
                    )
                }
                _ => assert_eq!(description["schemas"]["mode"], "shared", "{name}"),
            }
            // Everything else is unchanged.
            description.as_object_mut().unwrap().remove("schemas");
            let mut expected = inline.clone();
            for value in [&mut description, &mut expected] {
                for op in value["operations"].as_object_mut().unwrap().values_mut() {
                    let op = op.as_object_mut().unwrap();
                    op.remove("inputSchema");
                    op.remove("outputSchema");
                }
            }
            assert_eq!(description, expected, "{name}");
        }
    }
}

/// The shared and omitted descriptions the renderer's tests read.
#[test]
fn shared_and_omitted_descriptions_match_reviewed_goldens() {
    use crucible_codegen_docs::{Options, Schemas, generate_with};
    let service = project(&documented(WEATHER), Audience::Internal).unwrap();
    for (mode, name) in [
        (Schemas::Shared, "weather-shared.json"),
        (Schemas::Omit, "weather-omit.json"),
    ] {
        let mut options = Options::default();
        options.schemas = mode;
        let files = generate_with(&service, &options).unwrap();
        let file = files
            .into_iter()
            .find(|f| f.path == "describe.json")
            .unwrap();
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden/schemas")
            .join(name);
        if std::env::var_os("CRUCIBLE_BLESS").is_some() {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &file.contents).unwrap();
        }
        assert_eq!(
            fs::read_to_string(&path).unwrap().replace("\r\n", "\n"),
            file.contents,
            "{}",
            path.display()
        );
    }
}

/// Sender-signed operations (decision 81): the reference says who calls
/// each and which header carries its signature, and asks no credential or
/// release header of it.
#[test]
fn sender_signed_operations_match_reviewed_goldens() {
    golden("sender-external", "sender-signed", Audience::External);
    let files = files("sender-signed", Audience::External);
    let page = |path: &str| {
        files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path}"))
            .contents
            .clone()
    };
    let payment = page("src/operations/API_RecordPaymentEvent.md");
    assert!(
        payment.contains(r"Called by Payments Inc\., not by clients of this API. It takes no credential: Payments Inc\. signs each request, and the service verifies the signature in the <code>Payment-Signature</code> header."),
        "{payment}"
    );
    assert!(!payment.contains("Authorization: Bearer"), "{payment}");
    assert!(!payment.contains("Crucible-Version"), "{payment}");
    assert!(page("src/operations/API_GetAccount.md").contains("Crucible-Version: 2026-10-01"));
    assert!(page("llms.txt").contains(
        "- [ReportLeakedKeys](src/operations/API_ReportLeakedKeys.md): `POST /leaked-keys`. Called by CodeHost, which signs it in the `Scanner-Signature` header."
    ));
    let describe: Value = serde_json::from_str(&page("describe.json")).unwrap();
    assert_eq!(
        describe["operations"]["example.hooks#RecordPaymentEvent"]["sender"],
        serde_json::json!({"name": "Payments Inc.", "member": "signature", "header": "Payment-Signature"})
    );
    assert!(
        describe["operations"]["example.hooks#GetAccount"]
            .get("sender")
            .is_none()
    );
}

/// Model text reaches llms.txt as text: a title, a summary or a sender's
/// name opens no link, tag or comment, and llms-full.txt carries the
/// pages' own markup only, with a link only where the author wrote one.
#[test]
fn model_text_cannot_inject_markup_into_llms_txt() {
    let hostile = r"Hostile [live](https://live.example) `[code](https://code.example)` \[escaped](https://escaped.example) <script>alert(1)</script> <img src=x onerror=y> https://bare.example <!-- unclosed";
    let mut service = documented("hostile-text");
    service.title = Some("Title [t](https://title.example) <b>bold</b> # x".into());
    service.docs = Some(format!("{hostile}. Second sentence."));
    for op in &mut service.operations {
        op.docs = Some(hostile.into());
        op.public = true;
        op.auth.clear();
        op.sender = Some(crucible_ir::ir::Sender {
            name: hostile.into(),
            signature: "text".into(),
        });
    }
    for shape in service.shapes.values_mut() {
        if let crucible_ir::ir::Shape::Structure(s) = shape {
            s.docs = Some(hostile.into());
        }
    }
    let files = generate(&project(&service, Audience::External).unwrap()).unwrap();
    let file = |path: &str| {
        files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path}"))
            .contents
            .clone()
    };
    let generated = |destination: &str| files.iter().any(|f| f.path == destination);
    let index = file("llms.txt");
    assert!(index.contains("Hostile"), "{index}");
    for destination in destinations(&index) {
        assert!(
            generated(&destination),
            "{destination} in llms.txt:\n{index}"
        );
    }
    let markup: Vec<String> = pulldown_cmark::Parser::new(&index)
        .filter_map(|event| match event {
            pulldown_cmark::Event::Html(t) | pulldown_cmark::Event::InlineHtml(t) => {
                Some(t.to_string())
            }
            _ => None,
        })
        .collect();
    assert!(markup.is_empty(), "{markup:?} in llms.txt:\n{index}");
    assert!(
        index.starts_with(r"# Title \[t\]\(https\:\/\/title\.example\) &lt;b&gt;"),
        "the title is one heading's text:\n{index}"
    );
    let full = file("llms-full.txt");
    for destination in destinations(&full).into_iter().chain(hrefs(&full)) {
        let path = destination.split('#').next().unwrap();
        assert!(
            // The author's links: one in the documentation, and the
            // model's `@externalDocs` URL, percent-encoded where it would
            // end the link.
            generated(path)
                || destination == "https://live.example"
                || destination == "https://x.example/a%3Eb%29%3Cc%5D%28https://evil.example",
            "{destination} in llms-full.txt"
        );
    }
    let rule = full
        .lines()
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .to_owned();
    for event in pulldown_cmark::Parser::new(&full) {
        if let pulldown_cmark::Event::Html(t) | pulldown_cmark::Event::InlineHtml(t) = event {
            let t = t.to_string();
            assert!(!t.contains("<script") && !t.contains("<img"), "{t}");
            if t.starts_with("<!--") {
                assert!(t.starts_with(&format!("<!-- {rule} src/")), "{t}");
            }
        }
    }
}

/// An error whose body is its payload carries its code in the header
/// alone, and the error table says so.
#[test]
fn a_payload_error_s_code_is_in_its_header_alone() {
    let files = files("error-payloads", Audience::External);
    let pages: String = files
        .iter()
        .filter(|f| f.path.starts_with("src/operations/"))
        .map(|f| f.contents.as_str())
        .collect();
    assert!(
        pages.contains(
            "An error response names its code in the <code>X-Crucible-Error</code> header.\n"
        ),
        "{pages}"
    );
    assert!(!pages.contains("and in its JSON body"), "{pages}");
}
