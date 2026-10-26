//! The `README.md` and `AGENTS.md` every generated SDK package carries.
//!
//! The README is for a person installing the package: the install line,
//! constructing the client, one complete call, pagination, errors, retries
//! and links. `AGENTS.md` says the same things tersely for a coding agent,
//! which reads it whenever it works in a project that depends on the
//! package: the exact package and import names (agents otherwise invent
//! plausible ones), how the client is built and authenticated, and the
//! rules for pagination, errors and idempotency.
//!
//! Each client generator fills a [`Guide`] with its language's facts and
//! its surface's snippets; the layout is shared so every language's package
//! reads alike. The package's own README or `AGENTS.md`, where its metadata
//! supplies one, replaces the generated file.
//!
//! Model text (the service's title, `@externalDocs` names and URLs, version
//! strings) reaches these files only through [`markdown_text`],
//! [`code_span`] and [`markdown_url`]: a coding agent reads `AGENTS.md` as
//! instructions, so a title must not be able to add a heading, a paragraph,
//! a link or an HTML comment to it.

use crucible_ir::ir::Service;

use crate::Registry;
use crate::metadata::PackageMetadata;
use crate::surface::Surface;

/// The file names, beside the package manifest.
pub const README: &str = "README.md";
pub const AGENTS: &str = "AGENTS.md";

/// One language's facts for its package's README and `AGENTS.md`.
///
/// `pagination` and `errors` may name `{paginator}` (the first paginated
/// operation's paginator), `{showcase}` (the README's operation's method),
/// `{error}` (a modelled error's shape name) and `{operation}` (the
/// operation declaring it): the guide fills them from the model, so every
/// name it shows is one the package declares.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Guide {
    /// The language as a reader names it: `TypeScript`, `C#`.
    pub language: String,
    /// The Markdown code fence's language: `ts`, `csharp`.
    pub fence: String,
    /// The package's version, when it has one to install.
    pub version: Option<String>,
    /// Shell lines that add the package to a project, version included.
    pub install: Vec<String>,
    /// A manifest entry to add instead of, or beside, `install`, with its
    /// fence: a `Cargo.toml` dependency line, a Maven `<dependency>`.
    pub manifest: Option<(String, String)>,
    /// `install` and `manifest` fetch the package by name from the
    /// ecosystem's public registry (npm, PyPI, RubyGems, Packagist, NuGet,
    /// pub.dev, CRAN, crates.io). A private package, or one released by Git
    /// tag, shows where it is released instead: installing its name from
    /// the public registry could fetch someone else's package.
    pub public_registry: bool,
    /// Shown, in place of any install line, where the package has none.
    pub install_note: Option<String>,
    /// How code names the package, where that is not `package.import`
    /// (Go's package alias).
    pub refers_to: Option<String>,
    /// A statement sequence constructing the client with its token, as a
    /// snippet does.
    pub construct: String,
    /// How a caller authenticates: one or two sentences.
    pub auth: String,
    /// How a caller pages through a paginated operation.
    pub pagination: String,
    /// How a caller tells the modelled errors apart.
    pub errors: String,
    /// The language's retry configuration, after the shared retry rules.
    pub retries: String,
    /// Further links: the language guide.
    pub links: Vec<(String, String)>,
    /// The package's common metadata: a configured README or `AGENTS.md`
    /// replaces the generated one, its documentation, home page and
    /// repository are linked, and `private` and its release channel decide
    /// the install line.
    pub metadata: PackageMetadata,
}

impl Guide {
    pub fn new(language: &str, fence: &str) -> Self {
        Self {
            language: language.to_owned(),
            fence: fence.to_owned(),
            ..Self::default()
        }
    }

    /// Whether the package is on its ecosystem's public registry: not
    /// private, and not released by Git tag or to a private npm registry.
    pub fn published(&self) -> bool {
        !self.metadata.private
            && match &self.metadata.registry {
                None => true,
                Some(Registry::Npm { url, .. }) => crate::metadata::is_public_npm_registry(url),
                Some(_) => false,
            }
    }

    /// Where an unpublished package is released, as a sentence.
    pub fn release_note(&self, package: &str) -> String {
        let package = code_span(package);
        match (&self.metadata.git, &self.metadata.registry) {
            (Some(git), _) => format!(
                "{package} is released as Git tag {} of {}, not to a public registry: depend on it from there.",
                code_span(&git.tag),
                markdown_url(&git.url)
                    .map_or_else(|| code_span(&git.url), |url| format!("<{url}>")),
            ),
            (None, Some(Registry::Npm { url, .. })) => format!(
                "{package} is published to the npm registry {}, not to the public one.",
                code_span(url)
            ),
            (None, Some(Registry::Git)) => format!(
                "{package} is a development export with no assigned Git release tag. \
                 Use the generated package locally. For a published SDK release, \
                 ask the SDK publisher for its distribution repository and release tag; \
                 installing {package} from the public registry could fetch someone else's package."
            ),
            _ => format!(
                "{package} is private: it is not published to a public registry. Install it from \
                 where your organisation releases it; installing {package} from the public \
                 registry could fetch someone else's package."
            ),
        }
    }
}

/// Model text as one line of Markdown prose: line breaks become spaces,
/// controls and bidirectional characters are dropped, and every character
/// Markdown or HTML gives a meaning (emphasis, links, headings, tables,
/// code, raw HTML, entities) is backslash-escaped, so the text stays text.
pub fn markdown_text(text: &str) -> String {
    let line: Vec<char> = crate::docs::comment_line(text).chars().collect();
    let mut out = String::with_capacity(line.len());
    for (at, &c) in line.iter().enumerate() {
        // An underscore inside a word (`weather_client`) is never emphasis,
        // and a name a reader copies keeps it bare.
        let intraword = c == '_'
            && at > 0
            && line[at - 1].is_alphanumeric()
            && line.get(at + 1).is_some_and(|next| next.is_alphanumeric());
        if !intraword
            && matches!(
                c,
                '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '!' | '#' | '|' | '~' | '&'
            )
        {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Text as one Markdown code span: on one line, without controls, fenced
/// by more backticks than any run it holds, so it cannot close the span.
pub fn code_span(text: &str) -> String {
    let line = crate::docs::comment_line(text);
    let longest = line.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let ticks = "`".repeat(longest + 1);
    // CommonMark strips one space from each end, so a span that starts or
    // ends with a backtick (or a space) keeps it.
    if line.starts_with(['`', ' ']) || line.ends_with(['`', ' ']) {
        format!("{ticks} {line} {ticks}")
    } else {
        format!("{ticks}{line}{ticks}")
    }
}

/// A link destination for `<...>`: `http`, `https` or `mailto` only, with
/// `<`, `>`, `(`, `)` and spaces percent-encoded; `None` for another
/// scheme or a URL holding a control, a line break or a bidirectional
/// character, which no link should carry.
pub fn markdown_url(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    if !["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
    {
        return None;
    }
    if url.chars().any(|c| {
        c.is_control() || crate::bidi(c) || matches!(c, '\u{2028}' | '\u{2029}' | '\u{feff}')
    }) {
        return None;
    }
    let mut out = String::with_capacity(url.len());
    for c in url.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            c => out.push(c),
        }
    }
    Some(out)
}

fn title(service: &Service) -> String {
    markdown_text(service.title.as_deref().unwrap_or(service.id.name()))
}

fn version_line(service: &Service) -> String {
    match &service.api_version {
        Some(version) => format!(
            "API version {}, sent as {} on every request",
            code_span(version),
            code_span(&format!("Crucible-Version: {version}"))
        ),
        None => format!("model version {}", code_span(&service.version)),
    }
}

fn code(fence: &str, text: &str) -> String {
    let text = text.trim_end_matches('\n');
    // A fence longer than any backtick run in the text cannot be closed by it.
    let longest = text
        .split(|c| c != '`')
        .map(str::len)
        .max()
        .unwrap_or(0)
        .max(2);
    let ticks = "`".repeat(longest + 1);
    format!("{ticks}{fence}\n{text}\n{ticks}\n")
}

/// The links a package shows: the service's own documentation, the
/// package's, then the language's. A link with an unsafe URL is left out.
fn links(service: &Service, guide: &Guide) -> Vec<String> {
    let metadata = &guide.metadata;
    // The model's names are model text; the generator's own labels are not.
    service
        .notes
        .external_docs
        .iter()
        .map(|(name, url)| (markdown_text(name), url.clone()))
        .chain(
            [
                ("Documentation", &metadata.documentation),
                ("Home page", &metadata.homepage),
                ("Repository", &metadata.repository),
            ]
            .into_iter()
            .filter_map(|(name, url)| Some((name.to_owned(), url.clone()?))),
        )
        .chain(guide.links.iter().cloned())
        .filter_map(|(name, url)| Some(format!("- [{name}](<{}>)\n", markdown_url(&url)?)))
        .collect()
}

fn idempotent_operations(service: &Service, surface: &Surface) -> Vec<String> {
    service
        .operations
        .iter()
        .filter_map(|op| {
            let token = op.idempotency_token.as_ref()?;
            let entry = surface.operations.get(&op.id.to_string())?;
            let member = entry
                .input
                .as_ref()
                .and_then(|input| input.members.iter().find(|m| &m.member == token))
                .map_or(token.as_str(), |m| m.name.as_str());
            Some(format!(
                "{} ({})",
                code_span(&entry.method),
                code_span(member)
            ))
        })
        .collect()
}

/// `template` with its placeholders filled from the model; `None` when it
/// names one the model has no value for (`{paginator}` without a paginated
/// operation), or is empty.
fn fill(template: &str, service: &Service, surface: &Surface) -> Option<String> {
    if template.is_empty() {
        return None;
    }
    let mut text = template.to_owned();
    if text.contains("{paginator}") {
        let operation = crate::snippet::paginated(service)?;
        let paginator = surface
            .operations
            .get(&operation.id.to_string())?
            .paginators
            .as_ref()?
            .first()?;
        text = text.replace("{paginator}", paginator);
    }
    if text.contains("{showcase}") {
        let operation = crate::snippet::showcase(service)?;
        let method = &surface.operations.get(&operation.id.to_string())?.method;
        text = text.replace("{showcase}", method);
    }
    if text.contains("{error}") || text.contains("{operation}") {
        // The showcase's error, as its snippet handles it, else any
        // operation's.
        let (operation, error) = crate::snippet::showcase(service)
            .into_iter()
            .chain(&service.operations)
            .find_map(|op| Some((op, crate::snippet::call(service, op).error?.id)))?;
        text = text
            .replace("{error}", service.name_of(error))
            .replace("{operation}", operation.name());
    }
    Some(text)
}

/// Whether the service declares any modelled error a caller can catch.
fn has_errors(service: &Service) -> bool {
    service.operations.iter().any(|op| !op.errors.is_empty())
}

const RETRY_RULES: &str = "Calls are retried with backoff when a retry is safe: a modelled \
throttling error is retried for any operation; transport failures, HTTP 500, 502, 503 and 504 \
and other retryable modelled errors only for operations that are read-only, idempotent or carry \
an idempotency token. For operations that declare an idempotency token, an unset token is generated once per call \
and reused on every attempt; set it yourself to replay one logical request across calls.";

/// The package's imports, then the statements constructing the client:
/// what a caller copies to get a client. Each import is shown once: a line
/// of the statements that repeats one of the package's imports is dropped,
/// so statements that open with the import do not print it twice.
fn construct(surface: &Surface, guide: &Guide) -> String {
    let imports = &surface.package.imports;
    if imports.is_empty() {
        // PHP's autoloader needs no import line.
        return guide.construct.clone();
    }
    let mut statements: Vec<&str> = Vec::new();
    let mut dropped = false;
    for line in guide.construct.lines() {
        if imports.iter().any(|import| import == line.trim_end()) {
            dropped = true;
            continue;
        }
        let blank = line.trim().is_empty();
        // A dropped import leaves the blank lines around it: keep one
        // between statements, and none at the top.
        if blank && dropped && statements.last().is_none_or(|last| last.trim().is_empty()) {
            continue;
        }
        if !blank {
            dropped = false;
        }
        statements.push(line);
    }
    format!("{}\n\n{}", imports.join("\n"), statements.join("\n"))
}

/// What the package's install section shows: shell lines, a manifest
/// entry, or a note naming where an unpublished package is released.
struct Install {
    lines: Vec<String>,
    manifest: Option<(String, String)>,
    note: Option<String>,
}

fn install(surface: &Surface, guide: &Guide) -> Install {
    if guide.public_registry && !guide.published() {
        return Install {
            lines: Vec::new(),
            manifest: None,
            note: Some(guide.release_note(&surface.package.name)),
        };
    }
    Install {
        lines: guide.install.clone(),
        manifest: guide.manifest.clone(),
        note: guide.install_note.clone(),
    }
}

/// The package's `README.md`: the configured one, else generated.
pub fn readme(service: &Service, surface: &Surface, guide: &Guide) -> String {
    if let Some(readme) = &guide.metadata.readme {
        return readme.clone();
    }
    let title = title(service);
    let package = &surface.package.name;
    let mut out = format!(
        "# {heading}\n\n<!-- Generated by crucible. Change the model or the SDK configuration and regenerate. -->\n\n\
         The {language} client for the {title} API ({id}, {version}), generated by \
         [Crucible](https://github.com/Operation-Leyline/crucible) from the service's model.\n\n",
        heading = markdown_text(package),
        language = guide.language,
        id = code_span(&service.id.to_string()),
        version = version_line(service),
    );
    out.push_str("## Install\n\n");
    let install = install(surface, guide);
    if !install.lines.is_empty() {
        out.push_str(&code("sh", &install.lines.join("\n")));
    }
    if let Some((fence, manifest)) = &install.manifest {
        if !install.lines.is_empty() {
            out.push_str("\nor declare it in the project's manifest:\n\n");
        }
        out.push_str(&code(fence, manifest));
    }
    if let Some(note) = &install.note {
        if !install.lines.is_empty() || install.manifest.is_some() {
            out.push('\n');
        }
        out.push_str(note);
        out.push('\n');
    }
    out.push_str(&format!(
        "\nThe package is {}{version}; code refers to it as {}.\n\n",
        code_span(package),
        code_span(
            guide
                .refers_to
                .as_deref()
                .unwrap_or(&surface.package.import)
        ),
        version = guide
            .version
            .as_ref()
            .map(|v| format!(", version {}", code_span(v)))
            .unwrap_or_default(),
    ));
    out.push_str("## Create a client\n\n");
    out.push_str(&code(&guide.fence, &construct(surface, guide)));
    out.push('\n');
    out.push_str(&guide.auth);
    if let Some(sentence) = crate::snippet::added_headers_markdown(service, &surface.middleware) {
        out.push(' ');
        out.push_str(&sentence);
    }
    out.push_str("\n\n");
    if let Some(operation) = crate::snippet::showcase(service)
        && let Some(entry) = surface.operations.get(&operation.id.to_string())
        && !entry.snippet.is_empty()
    {
        out.push_str(&format!(
            "## Call an operation\n\n{} calls {}. Every operation has a method like it.\n\n",
            code_span(&entry.method),
            code_span(operation.name())
        ));
        out.push_str(&code(&guide.fence, &entry.snippet));
        out.push('\n');
    }
    if let Some(pagination) = fill(&guide.pagination, service, surface) {
        out.push_str("## Pagination\n\n");
        out.push_str(&pagination);
        out.push_str("\n\n");
        if let Some(operation) = crate::snippet::paginated(service)
            && let Some(snippet) = surface
                .operations
                .get(&operation.id.to_string())
                .and_then(|entry| entry.paginated_snippet.as_ref())
        {
            out.push_str(&code(&guide.fence, snippet));
            out.push('\n');
        }
    }
    if has_errors(service)
        && let Some(errors) = fill(&guide.errors, service, surface)
    {
        out.push_str("## Errors\n\n");
        out.push_str(&errors);
        out.push_str("\n\n");
    }
    out.push_str("## Retries and idempotency\n\n");
    out.push_str(RETRY_RULES);
    let idempotent = idempotent_operations(service, surface);
    if !idempotent.is_empty() {
        out.push_str(&format!(
            " These operations fill a token: {}.",
            idempotent.join(", ")
        ));
    }
    if !guide.retries.is_empty() {
        out.push(' ');
        out.push_str(&guide.retries);
    }
    out.push('\n');
    let links = links(service, guide);
    if !links.is_empty() {
        out.push_str("\n## Links\n\n");
        out.push_str(&links.concat());
    }
    out
}

/// The package's `AGENTS.md`: what a coding agent needs to call the API
/// correctly with this package, in a page it reads in full. The configured
/// one, else generated.
pub fn agents(service: &Service, surface: &Surface, guide: &Guide) -> String {
    if let Some(agents) = &guide.metadata.agents {
        return agents.clone();
    }
    let title = title(service);
    let package = &surface.package;
    let mut out = format!(
        "# {name}: instructions for coding agents\n\n<!-- Generated by crucible. Change the model or the SDK configuration and regenerate. -->\n\n\
         This is the generated {language} client for the {title} API ({id}, {version}). \
         The names below are exact: do not guess other package, import or method names.\n\n",
        name = markdown_text(&package.name),
        language = guide.language,
        id = code_span(&service.id.to_string()),
        version = version_line(service),
    );
    let install = install(surface, guide);
    // A `# ...` line is a shell comment (what to install first): prose here.
    let (comments, commands): (Vec<&String>, Vec<&String>) =
        install.lines.iter().partition(|line| line.starts_with('#'));
    let mut steps: Vec<String> = commands.into_iter().map(|line| code_span(line)).collect();
    if let Some((_, text)) = &install.manifest {
        // A manifest's entries, past any `[section]` header.
        let entries: Vec<String> = text
            .lines()
            .filter(|line| !line.trim().is_empty() && !line.starts_with('['))
            .map(|line| code_span(line.trim()))
            .collect();
        if !entries.is_empty() {
            steps.push(format!("in the manifest, {}", entries.join(" and ")));
        }
    }
    let prerequisites: String = comments
        .iter()
        .map(|line| format!(" {}", markdown_text(line.trim_start_matches('#').trim())))
        .collect();
    out.push_str(&format!(
        "- Install: {}.{prerequisites} The package is {}{}.\n",
        if steps.is_empty() {
            install.note.clone().unwrap_or_default()
        } else {
            let mut text = steps.join(", then ");
            if let Some(note) = &install.note {
                text.push_str(&format!(" ({note})"));
            }
            text
        }
        .trim_end_matches('.'),
        code_span(&package.name),
        guide
            .version
            .as_ref()
            .map(|v| format!(" at version {}", code_span(v)))
            .unwrap_or_default()
    ));
    if !package.imports.is_empty() {
        out.push_str(&format!(
            "- Import: {}.\n",
            package
                .imports
                .iter()
                .map(|line| code_span(line))
                .collect::<Vec<_>>()
                .join(" then ")
        ));
    }
    out.push_str(&format!(
        "- Client: {}. Construct it as below. {}\n",
        code_span(&surface.client.name),
        guide.auth
    ));
    out.push_str(&format!(
        "\n{}\n",
        code(&guide.fence, &construct(surface, guide))
    ));
    if let Some(sentence) = crate::snippet::added_headers_markdown(service, &surface.middleware) {
        out.push_str(&format!(
            "- Headers: {sentence} Do not set them yourself.\n"
        ));
    }
    if !surface.middleware.is_empty() {
        let names: Vec<String> = surface
            .middleware
            .iter()
            .map(|m| format!("{} (adds {})", code_span(&m.name), m.headers.join(", ")))
            .collect();
        out.push_str(&format!(
            "- Middleware: the service requires {}; the client runs it by default.\n",
            names.join(", ")
        ));
    }
    out.push_str(&format!(
        "- Operations: one method per operation, such as {}. {}\n",
        surface
            .operations
            .values()
            .take(3)
            .map(|op| code_span(&op.method))
            .collect::<Vec<_>>()
            .join(", "),
        if surface.operations.values().any(|op| op.output.as_ref().is_some_and(|output| output.style == "async-iterator")) {
            "Each takes its input type. Buffered methods return their output type; streaming methods resolve to a ResponseStream event iterable."
        } else {
            "Each takes its input type and returns its output type."
        }
    ));
    if let Some(pagination) = fill(&guide.pagination, service, surface) {
        out.push_str(&format!("- Pagination: {pagination}\n"));
    }
    if has_errors(service)
        && let Some(errors) = fill(&guide.errors, service, surface)
    {
        out.push_str(&format!("- Errors: {errors}\n"));
    }
    out.push_str(&format!(
        "- Retries: automatic for safe calls; do not wrap calls in your own retry loop.{}\n",
        if guide.retries.is_empty() {
            String::new()
        } else {
            format!(" {}", guide.retries)
        }
    ));
    let idempotent = idempotent_operations(service, surface);
    if !idempotent.is_empty() {
        out.push_str(&format!(
            "- Idempotency: these calls fill an idempotency token when it is unset: {}. Set it yourself to replay one logical request.\n",
            idempotent.join(", ")
        ));
    }
    out.push_str(
        "- Generated code: do not edit files in this package; they are replaced when it is regenerated.\n",
    );
    if let Some(operation) = crate::snippet::showcase(service)
        && let Some(entry) = surface.operations.get(&operation.id.to_string())
        && !entry.snippet.is_empty()
    {
        out.push_str(&format!(
            "\n## A complete call: {}\n\n{}",
            code_span(operation.name()),
            code(&guide.fence, &entry.snippet)
        ));
    }
    let links = links(service, guide);
    if !links.is_empty() {
        out.push_str("\n## Documentation\n\n");
        out.push_str(&links.concat());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Guide, code_span, construct, markdown_text, markdown_url};
    use crate::surface::{ClientType, Package, Surface};

    #[test]
    fn the_client_block_shows_each_import_once() {
        let mut package = Package::new("cargo", "weather-sdk", "weather_sdk");
        package.imports = vec!["use weather_sdk::*;".to_owned()];
        let surface = Surface::new("rust", package, ClientType::new("WeatherClient", ""));
        let mut guide = Guide::new("Rust", "rust");
        let expected = "use weather_sdk::*;\n\nlet client = WeatherClient::new(config);";
        // Statements that open with the package's import show it once.
        guide.construct = "use weather_sdk::*;\n\nlet client = WeatherClient::new(config);".into();
        assert_eq!(construct(&surface, &guide), expected);
        guide.construct = "let client = WeatherClient::new(config);".into();
        assert_eq!(construct(&surface, &guide), expected);
        // A repeat between statements leaves one blank line, not two.
        guide.construct = "let a = 1;\n\nuse weather_sdk::*;\n\nlet b = 2;".into();
        assert_eq!(
            construct(&surface, &guide),
            "use weather_sdk::*;\n\nlet a = 1;\n\nlet b = 2;"
        );
        // An import of something else stays.
        guide.construct = "use std::env;\n\nlet client = WeatherClient::new(config);".into();
        assert_eq!(
            construct(&surface, &guide),
            "use weather_sdk::*;\n\nuse std::env;\n\nlet client = WeatherClient::new(config);"
        );
    }

    #[test]
    fn model_text_stays_one_line_of_text() {
        assert_eq!(
            markdown_text("Evil\n\n## SYSTEM <!-- x --> [a](b) *e* `c`"),
            r"Evil  \#\# SYSTEM \<\!-- x --\> \[a\](b) \*e\* \`c\`"
        );
        assert_eq!(markdown_text("a\u{1b}[201~b\u{202e}c"), r"a\[201\~bc");
    }

    #[test]
    fn code_spans_cannot_be_closed_from_inside() {
        assert_eq!(code_span("a"), "`a`");
        assert_eq!(code_span("a`b"), "``a`b``");
        assert_eq!(code_span("`x`"), "`` `x` ``");
        assert_eq!(code_span("a\nb"), "`a b`");
    }

    #[test]
    fn urls_are_web_links_without_breaks() {
        assert_eq!(
            markdown_url("https://x.example/a>b) c").as_deref(),
            Some("https://x.example/a%3Eb%29%20c")
        );
        assert_eq!(markdown_url("javascript:alert(1)"), None);
        assert_eq!(markdown_url("https://x.example/\na"), None);
        assert_eq!(
            markdown_url("mailto:a@example.com").as_deref(),
            Some("mailto:a@example.com")
        );
    }
}
