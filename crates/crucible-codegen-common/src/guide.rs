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
//! reads alike. A package's own README, where its options supply one,
//! replaces the generated README; `AGENTS.md` is always generated.

use crucible_ir::ir::Service;

use crate::surface::Surface;

/// The file names, beside the package manifest.
pub const README: &str = "README.md";
pub const AGENTS: &str = "AGENTS.md";

/// One language's facts for its package's README and `AGENTS.md`.
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
    /// Further links: the language guide, the API reference.
    pub links: Vec<(String, String)>,
}

impl Guide {
    pub fn new(language: &str, fence: &str) -> Self {
        Self {
            language: language.to_owned(),
            fence: fence.to_owned(),
            ..Self::default()
        }
    }
}

fn title(service: &Service) -> &str {
    service.title.as_deref().unwrap_or(service.id.name())
}

fn version_line(service: &Service) -> String {
    match &service.api_version {
        Some(version) => format!(
            "API version `{version}`, sent as `Crucible-Version: {version}` on every request"
        ),
        None => format!("model version `{}`", service.version),
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

/// The links a package shows: the service's own documentation first, then
/// the language's.
fn links(service: &Service, guide: &Guide) -> Vec<(String, String)> {
    service
        .notes
        .external_docs
        .iter()
        .map(|(name, url)| (name.clone(), url.clone()))
        .chain(guide.links.iter().cloned())
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
            Some(format!("`{}` (`{member}`)", entry.method))
        })
        .collect()
}

const RETRY_RULES: &str = "Calls are retried with backoff when a retry is safe: a modelled \
throttling error is retried for any operation; transport failures, HTTP 500, 502, 503 and 504 \
and other retryable modelled errors only for operations that are read-only, idempotent or carry \
an idempotency token. An idempotency token the caller leaves unset is generated once per call \
and reused on every attempt; set it yourself to replay one logical request across calls.";

/// The package's imports, then the statements constructing the client:
/// what a caller copies to get a client.
fn construct(surface: &Surface, guide: &Guide) -> String {
    if surface.package.imports.is_empty() {
        // PHP's autoloader needs no import line.
        guide.construct.clone()
    } else {
        format!(
            "{}\n\n{}",
            surface.package.imports.join("\n"),
            guide.construct
        )
    }
}

/// The package's `README.md`.
pub fn readme(service: &Service, surface: &Surface, guide: &Guide) -> String {
    let title = title(service);
    let package = &surface.package.name;
    let mut out = format!(
        "# {package}\n\n<!-- Generated by crucible. Change the model or the SDK configuration and regenerate. -->\n\n\
         The {language} client for the {title} API (`{id}`, {version}), generated by \
         [Crucible](https://github.com/Operation-Leyline/crucible) from the service's model.\n\n",
        language = guide.language,
        id = service.id,
        version = version_line(service),
    );
    out.push_str("## Install\n\n");
    if !guide.install.is_empty() {
        out.push_str(&code("sh", &guide.install.join("\n")));
    }
    if let Some((fence, manifest)) = &guide.manifest {
        if !guide.install.is_empty() {
            out.push_str("\nor declare it in the project's manifest:\n\n");
        }
        out.push_str(&code(fence, manifest));
    }
    out.push_str(&format!(
        "\nThe package is `{package}`{version}; code refers to it as `{import}`.\n\n",
        version = guide
            .version
            .as_ref()
            .map(|v| format!(", version `{v}`"))
            .unwrap_or_default(),
        import = surface.package.import,
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
            "## Call an operation\n\n`{}` calls `{}`. Every operation has a method like it.\n\n",
            entry.method,
            operation.name()
        ));
        out.push_str(&code(&guide.fence, &entry.snippet));
        out.push('\n');
    }
    out.push_str("## Pagination\n\n");
    out.push_str(&guide.pagination);
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
    out.push_str("## Errors\n\n");
    out.push_str(&guide.errors);
    out.push_str("\n\n## Retries and idempotency\n\n");
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
        for (name, url) in links {
            out.push_str(&format!("- [{name}](<{url}>)\n"));
        }
    }
    out
}

/// The package's `AGENTS.md`: what a coding agent needs to call the API
/// correctly with this package, in a page it reads in full.
pub fn agents(service: &Service, surface: &Surface, guide: &Guide) -> String {
    let title = title(service);
    let package = &surface.package;
    let mut out = format!(
        "# {name}: instructions for coding agents\n\n<!-- Generated by crucible. Change the model or the SDK configuration and regenerate. -->\n\n\
         This is the generated {language} client for the {title} API (`{id}`, {version}). \
         The names below are exact: do not guess other package, import or method names.\n\n",
        name = package.name,
        language = guide.language,
        id = service.id,
        version = version_line(service),
    );
    let install = guide
        .install
        .first()
        .map(|line| format!("`{line}`"))
        .or_else(|| {
            // A manifest's first entry, past any `[section]` header.
            let (_, text) = guide.manifest.as_ref()?;
            let entry = text
                .lines()
                .find(|line| !line.trim().is_empty() && !line.starts_with('['))?;
            Some(format!("add `{}` to the project's manifest", entry.trim()))
        })
        .unwrap_or_default();
    out.push_str(&format!(
        "- Install: {install}. The package is `{}`{}.\n",
        package.name,
        guide
            .version
            .as_ref()
            .map(|v| format!(" at version `{v}`"))
            .unwrap_or_default()
    ));
    if !package.imports.is_empty() {
        out.push_str(&format!(
            "- Import: {}.\n",
            package
                .imports
                .iter()
                .map(|line| format!("`{line}`"))
                .collect::<Vec<_>>()
                .join(" then ")
        ));
    }
    out.push_str(&format!(
        "- Client: `{}`. Construct it as below. {}\n",
        surface.client.name, guide.auth
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
            .map(|m| format!("`{}` (adds {})", m.name, m.headers.join(", ")))
            .collect();
        out.push_str(&format!(
            "- Middleware: the service requires {}; the client runs it by default.\n",
            names.join(", ")
        ));
    }
    out.push_str(&format!(
        "- Operations: one method per operation, such as {}. Each takes its input type and returns its output type.\n",
        surface
            .operations
            .values()
            .take(3)
            .map(|op| format!("`{}`", op.method))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    out.push_str(&format!("- Pagination: {}\n", guide.pagination));
    out.push_str(&format!("- Errors: {}\n", guide.errors));
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
            "\n## A complete call: `{}`\n\n{}",
            operation.name(),
            code(&guide.fence, &entry.snippet)
        ));
    }
    let links = links(service, guide);
    if !links.is_empty() {
        out.push_str("\n## Documentation\n\n");
        for (name, url) in links {
            out.push_str(&format!("- [{name}](<{url}>)\n"));
        }
    }
    out
}
