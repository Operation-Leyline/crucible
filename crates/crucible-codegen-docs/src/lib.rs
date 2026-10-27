//! API reference output: mdBook pages and a static single-page site, both
//! rendered from the shared versioned description.
mod markdown;

use crucible_codegen_common::describe;
use crucible_ir::ir::Service;
use crucible_ir::{Diagnostics, ShapeId};

/// One generated artifact; publication is owned by `crucible-codegen`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct File {
    pub path: String,
    pub contents: String,
}

/// Whether `id` names a built-in type, which has no page of its own.
pub(crate) fn builtin(id: &str) -> bool {
    id.split_once('#')
        .is_some_and(|(namespace, _)| namespace == crucible_ir::PRELUDE)
}

pub use crucible_codegen_common::describe::Schemas;

/// Reference settings. Start from `Options::default()` and set fields.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct Options {
    /// Where `describe.json` carries the operations' JSON Schemas; inline
    /// by default.
    pub schemas: Schemas,
}

/// Generate all files before publication. The caller projects the audience first.
pub fn generate(service: &Service) -> Result<Vec<File>, Diagnostics> {
    generate_with(service, &Options::default())
}

/// [`generate`] with `options`.
pub fn generate_with(service: &Service, options: &Options) -> Result<Vec<File>, Diagnostics> {
    generate_cached(
        service,
        options,
        &crucible_ir::versions::HistoryCache::default(),
    )
}

/// [`generate_with`], dating elements through `cache`, which every
/// release's reference of one model at one audience shares.
fn generate_cached(
    service: &Service,
    options: &Options,
    cache: &crucible_ir::versions::HistoryCache,
) -> Result<Vec<File>, Diagnostics> {
    let mut unsupported = Diagnostics::default();
    for op in &service.operations {
        if op.response_mode == crucible_ir::ir::ResponseMode::ServerStream {
            unsupported.push(&op.id, "API reference rendering does not yet support server-response streams; use the OpenAPI stream description");
        }
    }
    if !unsupported.is_empty() {
        return Err(unsupported);
    }
    let mut description = describe::describe_cached(service, cache)?;
    description.schemas(options.schemas);
    let mut diagnostics = Diagnostics::default();
    let mut paths = std::collections::BTreeSet::new();
    for (id, item) in description.operations.iter().chain(&description.shapes) {
        if builtin(id) {
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
    // Release pages too: their files are escaped release names, distinct
    // byte for byte, but `V1` and `v1` would share one where case folds.
    let releases = description.versions["releases"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    if releases.len() > 1 {
        for release in releases {
            let name = release["name"].as_str().unwrap_or_default();
            let file = markdown::release_file(name).to_ascii_lowercase();
            if !paths.insert(format!("releases/{file}")) {
                diagnostics.push(
                    &service.id,
                    format!("docs.filename: release {name:?}'s page collides with another release's on a case-insensitive filesystem"),
                );
            }
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let mut files = markdown::render(service, &description);
    let json = description.to_json();
    files.extend(site(&json));
    files.push(File {
        path: "changes.json".into(),
        contents: changes(&description),
    });
    files.push(File {
        path: "describe.json".into(),
        contents: json,
    });
    files.push(File { path:"book.toml".into(), contents:"[book]\nlanguage = \"en\"\nsrc = \"src\"\n\n[output.html]\n\"smart-punctuation\" = false\n".into() });
    for file in &mut files {
        file.contents.truncate(file.contents.trim_end().len());
        file.contents.push('\n');
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// The directory [`generate_releases`] writes the reference of `release`
/// into: the release's name with every byte a file name or a link could
/// misread escaped, as its changelog page's file is (docs/docs.md, "Output
/// layout"). An older release's reference links the current one at
/// `../<directory>/`.
pub fn release_directory(release: &str) -> String {
    markdown::release_file(release)
}

/// Every supported release's reference in one output (decision 79;
/// docs/docs.md, "Every release's reference"): the reference of each
/// release, oldest first, as [`generate_with`] writes it for that
/// release's line at `audience`, each in its own [`release_directory`],
/// and at the root an `index.html` and an `llms.txt` that list them, so a
/// documentation site can serve them side by side and switch between
/// them. `service` is the current model as lowered, or a line of it, whose
/// current model is used; it is never projected to an audience first, since
/// a release is selected before an audience (release first, audience
/// second). Each reference is self-contained; an older one links the
/// current one beside it. Every release's problems are reported together,
/// and a release whose directory would collide with another's where case
/// folds, with a file at the root (`index.html`, `llms.txt`), or with a
/// directory the output writer refuses (`dist`, `target`) is refused.
pub fn generate_releases(
    service: &Service,
    audience: crucible_ir::Audience,
    options: &Options,
) -> Result<Vec<File>, Diagnostics> {
    let current = crucible_ir::versions::line_current(service).unwrap_or_else(|| service.clone());
    let releases: Vec<String> = crucible_ir::versions::releases(&current)
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut diagnostics = Diagnostics::default();
    let mut files = Vec::new();
    let mut directories = std::collections::BTreeSet::new();
    // Every reference reads the same older releases' histories.
    let cache = crucible_ir::versions::HistoryCache::default();
    for release in &releases {
        let directory = release_directory(release);
        // The root's own files, and the first segments the output writer
        // keeps for build tools, are no release's directory.
        if ["index.html", "llms.txt", "dist", "target"]
            .contains(&directory.to_ascii_lowercase().as_str())
        {
            diagnostics.push(
                &current.id,
                format!("docs.filename: release {release:?}'s directory would be {directory:?}, which every release's reference keeps for itself; rename the release"),
            );
            continue;
        }
        // A release's page already refuses this inside every reference
        // with more than one release; a lone release has no sibling.
        if !directories.insert(directory.to_ascii_lowercase()) {
            diagnostics.push(
                &current.id,
                format!("docs.filename: release {release:?}'s directory collides with another release's on a case-insensitive filesystem"),
            );
            continue;
        }
        let reference = crucible_ir::versions::line(&current, release)
            .and_then(|line| crucible_ir::project(&line, audience))
            .and_then(|line| generate_cached(&line, options, &cache));
        match reference {
            Ok(reference) => files.extend(reference.into_iter().map(|file| File {
                path: format!("{directory}/{}", file.path),
                contents: file.contents,
            })),
            Err(problems) => {
                for problem in problems.iter() {
                    diagnostics.push(
                        &problem.shape,
                        format!("release {release:?}: {}", problem.message),
                    );
                }
            }
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    files.extend(releases_index(&current, &releases));
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// The root of [`generate_releases`]: `index.html`, a page linking each
/// release's reference, with no script so it works under the reference's
/// own `Content-Security-Policy`, and `llms.txt`, linking each release's
/// `llms.txt` for an agent. Model text is escaped as everywhere else: HTML
/// text in the page, inert Markdown in `llms.txt`.
fn releases_index(current: &Service, releases: &[String]) -> [File; 2] {
    let title = current
        .title
        .as_deref()
        .unwrap_or_else(|| current.id.name());
    let current_name = crucible_ir::versions::current_name(current);
    let sunset = |release: &str| {
        current
            .versions
            .iter()
            .find(|version| version.name == release)
            .and_then(|version| version.sunset.as_deref())
    };
    let html = |text: &str| {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    let mut page = format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{} API reference</title>\n<link rel=\"stylesheet\" href=\"{}/explorer.css\">\n</head>\n<body>\n<main class=\"cx-main\">\n<h1>{}</h1>\n<p>The API reference of every supported release, oldest first. The current release is <code>{}</code>.</p>\n<ul class=\"cx-list\">\n",
        html(title),
        release_directory(current_name),
        html(title),
        html(current_name)
    );
    let mut llms = format!(
        "# {}\n\n> The API reference of every supported release, one directory each, oldest first.\n\nEach release's reference is complete on its own: its `llms.txt` indexes its Markdown pages, and every request it documents names that release in `Crucible-Version`. The current release is {}.\n\n## Releases\n\n",
        markdown::plain(title),
        markdown::code_span(current_name)
    );
    for release in releases {
        let directory = release_directory(release);
        let (html_fact, llms_fact) = if release == current_name {
            (
                "the current release".to_owned(),
                "the current release".to_owned(),
            )
        } else {
            match sunset(release) {
                Some(date) => (
                    format!("supported until <code>{}</code>", html(date)),
                    format!("supported until {}", markdown::code_span(date)),
                ),
                None => ("an older release".to_owned(), "an older release".to_owned()),
            }
        };
        page.push_str(&format!(
            "<li><a href=\"{directory}/index.html\"><code>{}</code></a>: {html_fact}</li>\n",
            html(release)
        ));
        llms.push_str(&format!(
            "- [{}]({directory}/llms.txt): release {}, {llms_fact}\n",
            markdown::plain(release),
            markdown::code_span(release)
        ));
    }
    page.push_str("</ul>\n<p>For agents: <a href=\"llms.txt\">llms.txt</a> links each release's Markdown index.</p>\n</main>\n</body>\n</html>\n");
    [
        File {
            path: "index.html".into(),
            contents: page,
        },
        File {
            path: "llms.txt".into(),
            contents: llms,
        },
    ]
}

/// `changes.json` (`crucible.changes/1`, docs/docs.md): the release chain
/// alone, the same data as `describe.json`'s `versions`, for a site that
/// builds a changelog or a feed without reading the whole description.
fn changes(description: &describe::Description) -> String {
    let mut value = serde_json::json!({
        "format": "crucible.changes/1",
        "service": description.service["id"],
        "versions": description.versions,
    });
    describe::sort_keys(&mut value);
    // `generate` ends every file with one newline.
    serde_json::to_string_pretty(&value).expect("JSON data")
}

/// The static reference site: a fixed renderer plus this service's description.
/// `describe.js` carries the same JSON as a classic script so `index.html` also
/// works from `file://`, where the browser refuses to fetch `describe.json`.
fn site(description: &str) -> [File; 4] {
    // Sources may be checked out with CRLF; generated output never is.
    let runtime = |path: &str, source: &str| File {
        path: path.into(),
        contents: source.replace("\r\n", "\n"),
    };
    [
        runtime("index.html", crucible_explorer_assets::INDEX_HTML),
        runtime("explorer.js", crucible_explorer_assets::EXPLORER_JS),
        runtime("explorer.css", crucible_explorer_assets::EXPLORER_CSS),
        File {
            path: "describe.js".into(),
            contents: format!(
                "globalThis.CRUCIBLE_DESCRIPTION = JSON.parse({});\n",
                serde_json::to_string(description.trim_end()).expect("JSON text")
            ),
        },
    ]
}

/// The upgrade guide from release `from` to release `to` of `service`, or
/// to its current release when `to` is `None` (`crucible upgrade-guide`;
/// docs/api-versions-clients.md, 5d): Markdown listing each migration
/// between them, oldest first, with its documentation, every change in
/// model terms and what a client of the release before it sees instead,
/// the operations it retired, the sunsets of the releases a client passes
/// through, and what was added since. It reads the same `versions` block
/// the reference and `changes.json` carry, so the caller projects the
/// audience first, as for [`generate`]: an external guide names no
/// internal step or element. It stands on its own, so it links no
/// reference page, and a doc link is its text. A reference generated with
/// more than one release carries the same guides as pages
/// (`src/upgrade/<from>_to_<to>.md`), linked from its release pages; a
/// release's own reference links those of the current release's.
/// `service` is the current model: a release's line is refused, since its
/// view leaves out what its release cannot see. Refused too, naming the
/// supported releases, unless both are supported releases and `from` is
/// older than `to`.
pub fn upgrade_guide(
    service: &Service,
    from: &str,
    to: Option<&str>,
) -> Result<String, Diagnostics> {
    if crucible_ir::versions::is_line(service) {
        let mut diagnostics = Diagnostics::default();
        diagnostics.push(
            &service.id,
            format!(
                "upgrade guide: this is release {:?}'s view of the model; a guide is written from the current model",
                service.version
            ),
        );
        return Err(diagnostics);
    }
    let description = describe::describe(service)?;
    let (older, newer) = markdown::upgrade::select(&description, from, to).map_err(|message| {
        let mut diagnostics = Diagnostics::default();
        diagnostics.push(&service.id, message);
        diagnostics
    })?;
    let mut guide = markdown::upgrade::standalone(&description, older, newer);
    guide.truncate(guide.trim_end().len());
    guide.push('\n');
    Ok(guide)
}
