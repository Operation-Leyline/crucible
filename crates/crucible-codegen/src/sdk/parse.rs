//! Reading and checking `crucible-sdk.toml`. Every problem is collected
//! against its key, so one run reports them all.

use std::collections::BTreeMap;
use std::fmt::Display;
use std::path::Path;

use toml::{Table, Value};

use super::language::LANGUAGE_KEYS;
use super::{
    Channel, Endpoint, Errors, Language, LanguageOptions, Metadata, Provenance, Runtime, Sdk,
    SdkFile,
};

const TOP_KEYS: &[&str] = &["schema_version", "model", "service", "defaults", "sdk"];

/// The common keys: `[defaults]` holds only these, and every SDK takes them.
pub(crate) const COMMON_KEYS: &[&str] = &[
    "description",
    "repository",
    "homepage",
    "documentation",
    "license",
    "license_file",
    "authors",
    "readme",
    "agents",
    "keywords",
    "private",
    "provenance",
];

/// Keys only an SDK takes. `test`, `lockfiles` and `channel` are read only
/// by `crucible sdk`'s release commands.
const SDK_KEYS: &[&str] = &[
    "language",
    "audience",
    "api_version",
    "out",
    "version",
    "test",
    "lockfiles",
    "channel",
];

/// `package.json` keys the generated package owns.
const OWNED_PACKAGE_JSON_KEYS: &[&str] = &[
    "name",
    "version",
    "crucible",
    "files",
    "private",
    "publishConfig",
];

struct Context<'a> {
    file: &'a Path,
    errors: Vec<String>,
}

impl Context<'_> {
    fn error(&mut self, key: &str, message: impl Display) {
        self.errors
            .push(format!("{}: {key}: {message}", self.file.display()));
    }
}

/// The common keys one table sets. `None` is unset, so an SDK inherits
/// `[defaults]` key by key.
#[derive(Default, Clone)]
struct Common {
    description: Option<String>,
    repository: Option<String>,
    homepage: Option<String>,
    documentation: Option<String>,
    /// `license` and `license_file` are one setting: an SDK that sets either
    /// replaces both from `[defaults]`.
    license: Option<(Option<String>, Option<String>)>,
    authors: Option<Vec<String>>,
    readme: Option<String>,
    agents: Option<String>,
    keywords: Option<Vec<String>>,
    private: Option<bool>,
    provenance: Option<Provenance>,
}

pub(super) fn parse(path: &Path, text: &str) -> Result<SdkFile, Errors> {
    // The parser's own message quotes the offending source line, which may
    // hold a credential: report its position and message only.
    let table: Table = text.parse().map_err(|error: toml::de::Error| {
        let at = error.span().map_or_else(String::new, |span| {
            let before = &text[..span.start.min(text.len())];
            let line = before.matches('\n').count() + 1;
            let column = before.len() - before.rfind('\n').map_or(0, |index| index + 1) + 1;
            format!(":{line}:{column}")
        });
        Errors(vec![format!("{}{at}: {}", path.display(), error.message())])
    })?;
    let mut context = Context {
        file: path,
        errors: Vec::new(),
    };
    for key in table.keys().filter(|key| !TOP_KEYS.contains(&key.as_str())) {
        context.error(key, "unknown key");
    }
    match table.get("schema_version") {
        Some(Value::Integer(version)) if *version == super::SCHEMA_VERSION => {}
        Some(Value::Integer(version)) => context.error(
            "schema_version",
            format!(
                "{version} is not a schema this crucible reads ({})",
                super::SCHEMA_VERSION
            ),
        ),
        Some(_) => context.error("schema_version", "must be an integer"),
        None => context.error(
            "schema_version",
            format!("missing: set schema_version = {}", super::SCHEMA_VERSION),
        ),
    }
    let model = match table.get("model") {
        Some(value) => path_value(&mut context, "model", value),
        None => {
            context.error("model", "missing: name the .cru package directory");
            None
        }
    };
    let service = table
        .get("service")
        .and_then(|value| string(&mut context, "service", value))
        .filter(|service| match service.parse::<crucible_ir::ShapeId>() {
            Ok(_) => true,
            Err(error) => {
                context.error(
                    "service",
                    format!("{service:?} is not a shape ID such as example#Service: {error}"),
                );
                false
            }
        });
    let defaults = match table.get("defaults") {
        Some(Value::Table(defaults)) => {
            for key in defaults.keys() {
                let at = format!("defaults.{key}");
                if SDK_KEYS.contains(&key.as_str()) || LANGUAGE_KEYS.contains(&key.as_str()) {
                    context.error(
                        &at,
                        format!(
                            "[defaults] holds only the common keys; set `{key}` on each [sdk.<name>]"
                        ),
                    );
                } else if !COMMON_KEYS.contains(&key.as_str()) {
                    context.error(&at, "unknown key");
                }
            }
            common(&mut context, "defaults", defaults)
        }
        Some(_) => {
            context.error("defaults", "must be a table");
            Common::default()
        }
        None => Common::default(),
    };
    let mut sdks = BTreeMap::new();
    match table.get("sdk") {
        Some(Value::Table(declared)) => {
            for (name, value) in declared {
                let at = format!("sdk.{name}");
                let valid = valid_name(name);
                if !valid {
                    context.error(
                        &at,
                        "an SDK name is a lowercase letter followed by lowercase letters, digits or hyphens",
                    );
                }
                let Value::Table(table) = value else {
                    context.error(&at, "must be a table");
                    continue;
                };
                // An invalid name's table is still checked, so one run
                // reports everything.
                if let Some(sdk) = sdk(&mut context, name, table, &defaults)
                    && valid
                {
                    sdks.insert(name.clone(), sdk);
                }
            }
        }
        Some(_) => context.error("sdk", "must hold [sdk.<name>] tables"),
        None => {}
    }
    // Two SDKs writing one directory, or one inside the other, would fight
    // over its files: refused before anything is written.
    let outs: Vec<(String, &str)> = sdks
        .iter()
        .map(|(name, sdk)| (out_key(&sdk.out), name.as_str()))
        .collect();
    for (index, (key, name)) in outs.iter().enumerate() {
        for (other_key, other) in &outs[..index] {
            let nested = |outer: &str, inner: &str| {
                inner == outer || inner.starts_with(&format!("{outer}/"))
            };
            if nested(key, other_key) || nested(other_key, key) {
                context.error(
                    &format!("sdk.{name}.out"),
                    format!(
                        "{:?} overlaps sdk.{other}'s out {:?}; each SDK needs its own directory",
                        sdks[*name].out, sdks[*other].out
                    ),
                );
            }
        }
    }
    if !context.errors.is_empty() {
        return Err(Errors(context.errors));
    }
    Ok(SdkFile {
        path: path.to_path_buf(),
        model: model.expect("checked"),
        service,
        sdks,
    })
}

fn sdk(context: &mut Context<'_>, name: &str, table: &Table, defaults: &Common) -> Option<Sdk> {
    let at = |key: &str| format!("sdk.{name}.{key}");
    for key in table.keys() {
        let key = key.as_str();
        if !COMMON_KEYS.contains(&key) && !SDK_KEYS.contains(&key) && !LANGUAGE_KEYS.contains(&key)
        {
            context.error(&at(key), "unknown key");
        }
    }
    let named = Language::parse(name);
    let language = match table.get("language") {
        Some(value) => string(context, &at("language"), value).and_then(|spelled| {
            let language = Language::parse(&spelled);
            if language.is_none() {
                context.error(
                    &at("language"),
                    format!(
                        "{spelled:?} is not a client language: {}",
                        Language::ALL.map(Language::name).join(", ")
                    ),
                );
            }
            language
        }),
        None => {
            if named.is_none() {
                context.error(
                    &format!("sdk.{name}"),
                    format!("set `language`: {name:?} is not a language name"),
                );
            }
            named
        }
    };
    // A language's name is reserved for that language's SDK, so a reader
    // never takes `[sdk.rust]` for anything but Rust.
    if let (Some(named), Some(language)) = (named, language)
        && named != language
    {
        context.error(
            &at("language"),
            format!(
                "an SDK named {name:?} is the {} SDK; name a {} SDK otherwise",
                named.display(),
                language.display()
            ),
        );
    }
    if let Some(language) = language {
        for key in LANGUAGE_KEYS {
            if table.contains_key(*key) && !language.keys().contains(key) {
                context.error(
                    &at(key),
                    format!("the {} generator takes no `{key}`", language.display()),
                );
            }
        }
        for key in language.required() {
            if !table.contains_key(*key) {
                context.error(
                    &format!("sdk.{name}"),
                    format!("a {} SDK needs `{key}`", language.display()),
                );
            }
        }
    }
    let audience = match table.get("audience") {
        None => crucible_ir::Audience::External,
        Some(value) => match string(context, &at("audience"), value).as_deref() {
            Some("external") => crucible_ir::Audience::External,
            Some("internal") => crucible_ir::Audience::Internal,
            Some(other) => {
                context.error(
                    &at("audience"),
                    format!("{other:?} is not \"external\" or \"internal\""),
                );
                crucible_ir::Audience::External
            }
            None => crucible_ir::Audience::External,
        },
    };
    // The release this SDK is a line of (decision 76). Whether the model
    // serves it is the generator's to say, against the chain it lowers.
    let api_version = table
        .get("api_version")
        .and_then(|value| string(context, &at("api_version"), value))
        .filter(|release| {
            let valid = !release.is_empty() && release.bytes().all(|byte| byte.is_ascii_graphic());
            if !valid {
                context.error(
                    &at("api_version"),
                    format!(
                        "{release:?} is not a release name: one a client sends in Crucible-Version, of visible ASCII"
                    ),
                );
            }
            valid
        });
    let out = match table.get("out") {
        Some(value) => path_value(context, &at("out"), value),
        None => {
            context.error(
                &format!("sdk.{name}"),
                "set `out`: where `crucible sdk generate` writes the package",
            );
            None
        }
    };
    let version = table
        .get("version")
        .and_then(|value| string(context, &at("version"), value))
        .filter(|version| {
            let plain = version.split('.').count() == 3
                && version
                    .split('.')
                    .all(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit()));
            if !plain {
                context.error(
                    &at("version"),
                    format!("{version:?} is not a version floor such as \"0.2.0\""),
                );
            }
            plain
        });
    let test = match table.get("test") {
        None => Vec::new(),
        Some(Value::Array(commands)) => commands
            .iter()
            .filter_map(|command| {
                let arguments = list(context, &at("test"), command)?;
                if arguments.is_empty() {
                    context.error(&at("test"), "a command needs at least the program");
                    return None;
                }
                Some(arguments)
            })
            .collect(),
        Some(_) => {
            context.error(
                &at("test"),
                "must be an array of commands, each an array of arguments",
            );
            Vec::new()
        }
    };
    let lockfiles = match table.get("lockfiles") {
        None => Vec::new(),
        Some(value) => list(context, &at("lockfiles"), value)
            .unwrap_or_default()
            .into_iter()
            .filter(|path| path_ok(context, &at("lockfiles"), path))
            .collect(),
    };
    let channel = table
        .get("channel")
        .and_then(|value| channel(context, &at("channel"), value, language));
    let options = options(context, name, table);
    let own = common(context, &format!("sdk.{name}"), table);
    let metadata = merge(own.clone(), defaults.clone());
    if let Some(Language::Rust) = language {
        let inherited = own.keywords.is_none() && defaults.keywords.is_some();
        for problem in crucible_codegen_common::metadata::cargo_keyword_problems(&metadata.keywords)
        {
            context.error(
                &at("keywords"),
                if inherited {
                    format!("{problem} (from [defaults])")
                } else {
                    problem
                },
            );
        }
    }
    if metadata.private {
        match &channel {
            None if !table.contains_key("channel") => context.error(
                &at("channel"),
                "a private SDK names its channel, so it never reaches a public registry by default",
            ),
            Some(channel) if channel.is_public() => context.error(
                &at("channel"),
                "a private SDK never goes to a public registry",
            ),
            Some(Channel::Npm {
                access: Some(access),
                ..
            }) if access == "public" => context.error(
                &at("channel.access"),
                "a private SDK is never published with npm access \"public\"",
            ),
            _ => {}
        }
    }
    if audience == crucible_ir::Audience::Internal
        && channel.as_ref().is_some_and(Channel::is_public)
    {
        context.error(
            &at("channel"),
            "an internal-audience SDK never goes to a public registry",
        );
    }
    Some(Sdk {
        name: name.into(),
        language: language?,
        audience,
        api_version,
        out: out?,
        version,
        options,
        metadata,
        test,
        lockfiles,
        channel,
    })
}

fn options(context: &mut Context<'_>, name: &str, table: &Table) -> LanguageOptions {
    let at = |key: &str| format!("sdk.{name}.{key}");
    let mut text = |key: &str| {
        table
            .get(key)
            .and_then(|value| string(context, &at(key), value))
    };
    let mut options = LanguageOptions {
        package: text("package"),
        group_id: text("group_id"),
        artifact_id: text("artifact_id"),
        package_id: text("package_id"),
        module_name: text("module_name"),
        namespace: text("namespace"),
        prefix: text("prefix"),
        module_path: text("module_path"),
        client_name: text("client_name"),
        ..LanguageOptions::default()
    };
    options.endpoint = table.get("endpoint").and_then(|value| match value {
        Value::String(url) => match crucible_codegen_common::metadata::url_problem(url) {
            None => Some(Endpoint::Url(url.clone())),
            Some(problem) => {
                context.error(&at("endpoint"), problem);
                None
            }
        },
        Value::Table(table) if table.len() == 1 && table.contains_key("env") => {
            let variable = string(context, &at("endpoint"), &table["env"])?;
            let valid = variable
                .bytes()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
                && variable
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_');
            if !valid {
                context.error(
                    &at("endpoint"),
                    format!("{variable:?} is not an environment variable name"),
                );
                return None;
            }
            Some(Endpoint::Env {
                name: variable,
                value: None,
            })
        }
        _ => {
            context.error(&at("endpoint"), "must be a URL or { env = \"NAME\" }");
            None
        }
    });
    options.middleware = table
        .get("middleware")
        .and_then(|value| list(context, &at("middleware"), value))
        .unwrap_or_default()
        .into_iter()
        .filter(|middleware| {
            let known = middleware == "content-sha256";
            if !known {
                context.error(
                    &at("middleware"),
                    format!("{middleware:?} is not a middleware: \"content-sha256\""),
                );
            }
            known
        })
        .collect();
    options.package_json = match table.get("package_json") {
        None => None,
        Some(Value::Table(fragment)) => {
            for key in OWNED_PACKAGE_JSON_KEYS {
                if fragment.contains_key(*key) {
                    context.error(
                        &at("package_json"),
                        format!("cannot set `{key}`: the generated package.json owns it"),
                    );
                }
            }
            Some(serde_json::to_value(fragment).expect("TOML is JSON-shaped"))
        }
        Some(_) => {
            context.error(&at("package_json"), "must be a table");
            None
        }
    };
    options.runtime = match table.get("runtime") {
        None => None,
        Some(Value::Table(runtime)) => {
            for key in runtime.keys() {
                if !matches!(key.as_str(), "git" | "rev" | "version") {
                    context.error(&at(&format!("runtime.{key}")), "unknown key");
                }
            }
            let mut field = |key: &str| match runtime.get(key) {
                Some(value) => string(context, &at(&format!("runtime.{key}")), value),
                None => {
                    context.error(&at("runtime"), format!("needs `{key}`"));
                    None
                }
            };
            let (git, rev, version) = (field("git"), field("rev"), field("version"));
            if let Some(git) = &git
                && let Some(problem) = crucible_codegen_common::metadata::credential_problem(git)
            {
                context.error(&at("runtime.git"), problem);
            }
            match (git, rev, version) {
                (Some(git), Some(rev), Some(version)) => Some(Runtime { git, rev, version }),
                _ => None,
            }
        }
        Some(_) => {
            context.error(
                &at("runtime"),
                "must be { git = \"..\", rev = \"..\", version = \"..\" }",
            );
            None
        }
    };
    options.runtime_path = table
        .get("runtime_path")
        .and_then(|value| path_value(context, &at("runtime_path"), value));
    if table.contains_key("runtime") && table.contains_key("runtime_path") {
        context.error(
            &at("runtime_path"),
            "set `runtime` or `runtime_path`, not both",
        );
    }
    options
}

fn common(context: &mut Context<'_>, at: &str, table: &Table) -> Common {
    let key = |name: &str| format!("{at}.{name}");
    let mut text = |name: &str| {
        table
            .get(name)
            .and_then(|value| string(context, &key(name), value))
    };
    let mut common = Common {
        description: text("description"),
        repository: text("repository"),
        homepage: text("homepage"),
        documentation: text("documentation"),
        ..Common::default()
    };
    for (name, value) in [
        ("repository", &common.repository),
        ("homepage", &common.homepage),
        ("documentation", &common.documentation),
    ] {
        if let Some(value) = value
            && let Some(problem) = crucible_codegen_common::metadata::url_problem(value)
        {
            context.error(&key(name), problem);
        }
    }
    let license = table
        .get("license")
        .and_then(|value| string(context, &key("license"), value));
    let license_file = table
        .get("license_file")
        .and_then(|value| path_value(context, &key("license_file"), value));
    if table.contains_key("license") && table.contains_key("license_file") {
        context.error(
            &key("license_file"),
            "set `license` (an SPDX expression) or `license_file`, not both",
        );
    }
    if table.contains_key("license") || table.contains_key("license_file") {
        common.license = Some((license, license_file));
    }
    common.authors = table
        .get("authors")
        .and_then(|value| list(context, &key("authors"), value));
    common.readme = table
        .get("readme")
        .and_then(|value| path_value(context, &key("readme"), value));
    common.agents = table
        .get("agents")
        .and_then(|value| path_value(context, &key("agents"), value));
    common.keywords = table
        .get("keywords")
        .and_then(|value| list(context, &key("keywords"), value));
    common.private = table
        .get("private")
        .and_then(|value| boolean(context, &key("private"), value));
    common.provenance = match table.get("provenance") {
        None => None,
        Some(value) => match string(context, &key("provenance"), value).as_deref() {
            Some("full") => Some(Provenance::Full),
            Some("minimal") => {
                context.error(
                    &key("provenance"),
                    "\"minimal\" provenance is not built yet; use \"full\"",
                );
                None
            }
            Some(other) => {
                context.error(&key("provenance"), format!("{other:?} is not \"full\""));
                None
            }
            None => None,
        },
    };
    common
}

fn merge(own: Common, defaults: Common) -> Metadata {
    let (license, license_file) = own.license.or(defaults.license).unwrap_or_default();
    Metadata {
        description: own.description.or(defaults.description),
        repository: own.repository.or(defaults.repository),
        homepage: own.homepage.or(defaults.homepage),
        documentation: own.documentation.or(defaults.documentation),
        license,
        license_file,
        authors: own.authors.or(defaults.authors).unwrap_or_default(),
        readme: own.readme.or(defaults.readme),
        agents: own.agents.or(defaults.agents),
        keywords: own.keywords.or(defaults.keywords).unwrap_or_default(),
        private: own.private.or(defaults.private).unwrap_or(false),
        provenance: own
            .provenance
            .or(defaults.provenance)
            .unwrap_or(Provenance::Full),
    }
}

fn channel(
    context: &mut Context<'_>,
    at: &str,
    value: &Value,
    language: Option<Language>,
) -> Option<Channel> {
    let Value::Table(table) = value else {
        context.error(
            at,
            "must be a table such as { git = \"..\" } or { npm = \"..\" }",
        );
        return None;
    };
    let kinds: Vec<&str> = ["npm", "git", "cargo", "pypi"]
        .into_iter()
        .filter(|kind| table.contains_key(*kind))
        .collect();
    let [kind] = kinds.as_slice() else {
        context.error(at, "names exactly one of `npm` or `git`");
        return None;
    };
    let allowed: &[&str] = match *kind {
        "npm" => &["npm", "access", "dist_tag"],
        "git" => &["git", "branch", "path", "tag", "adopt"],
        _ => {
            context.error(
                &format!("{at}.{kind}"),
                format!("a {kind} channel is not built yet; use a git channel"),
            );
            return None;
        }
    };
    for key in table.keys().filter(|key| !allowed.contains(&key.as_str())) {
        context.error(
            &format!("{at}.{key}"),
            format!("a {kind} channel takes no `{key}`"),
        );
    }
    let optional = |context: &mut Context<'_>, key: &str| {
        table
            .get(key)
            .and_then(|value| string(context, &format!("{at}.{key}"), value))
    };
    // Every problem is reported before giving up on the channel.
    let mut valid = true;
    let url = string(context, &format!("{at}.{kind}"), &table[*kind]);
    if *kind == "npm" {
        if let Some(language) = language
            && language != Language::Ts
        {
            context.error(
                &format!("{at}.npm"),
                format!(
                    "an npm channel publishes TypeScript SDKs, not {}",
                    language.display()
                ),
            );
        }
        // A registry is an http(s) URL without credentials.
        if let Some(problem) = url
            .as_deref()
            .and_then(crucible_codegen_common::metadata::url_problem)
        {
            context.error(&format!("{at}.npm"), problem);
            valid = false;
        }
        let access = optional(context, "access");
        if let Some(access) = &access
            && !matches!(access.as_str(), "public" | "restricted")
        {
            context.error(
                &format!("{at}.access"),
                format!("{access:?} is not \"public\" or \"restricted\""),
            );
        }
        let dist_tag = optional(context, "dist_tag").unwrap_or_else(|| "latest".into());
        let url = url?;
        return valid.then_some(Channel::Npm {
            url,
            access,
            dist_tag,
        });
    }
    if let Some(problem) = url
        .as_deref()
        .and_then(crucible_codegen_common::metadata::credential_problem)
    {
        context.error(&format!("{at}.git"), problem);
        valid = false;
    }
    // The distribution repository's root is `.`; anything else stays inside it.
    let path = match table.get("path") {
        Some(Value::String(path)) if path == "." => Some(".".into()),
        Some(value) => path_value(context, &format!("{at}.path"), value),
        None => Some(".".into()),
    };
    let tag = optional(context, "tag").unwrap_or_else(|| "v{version}".into());
    if !tag.contains("{version}") {
        context.error(
            &format!("{at}.tag"),
            format!("{tag:?} has no {{version}}, so every release would take the same tag"),
        );
    }
    let adopt = table
        .get("adopt")
        .and_then(|value| list(context, &format!("{at}.adopt"), value))
        .unwrap_or_default()
        .into_iter()
        .filter(|path| path_ok(context, &format!("{at}.adopt"), path))
        .collect();
    let branch = optional(context, "branch").unwrap_or_else(|| "main".into());
    let (url, path) = (url?, path?);
    valid.then_some(Channel::Git {
        url,
        branch,
        path,
        tag,
        adopt,
    })
}

fn valid_name(name: &str) -> bool {
    name.bytes().next().is_some_and(|c| c.is_ascii_lowercase())
        && name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

fn string(context: &mut Context<'_>, at: &str, value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.trim().is_empty() => Some(text.clone()),
        Value::String(_) => {
            context.error(at, "is empty");
            None
        }
        _ => {
            context.error(at, "must be a string");
            None
        }
    }
}

fn boolean(context: &mut Context<'_>, at: &str, value: &Value) -> Option<bool> {
    let flag = value.as_bool();
    if flag.is_none() {
        context.error(at, "must be true or false");
    }
    flag
}

/// An array of strings. Every bad element is reported, not only the first.
fn list(context: &mut Context<'_>, at: &str, value: &Value) -> Option<Vec<String>> {
    let Value::Array(items) = value else {
        context.error(at, "must be an array of strings");
        return None;
    };
    let strings: Vec<Option<String>> = items.iter().map(|item| string(context, at, item)).collect();
    strings.into_iter().collect()
}

fn path_value(context: &mut Context<'_>, at: &str, value: &Value) -> Option<String> {
    let path = string(context, at, value)?;
    path_ok(context, at, &path).then_some(path)
}

/// Paths are relative to the SDK file, written with `/`, and stay inside its
/// directory: no empty, `.` or `..` segment (a trailing `/` is fine). So the
/// file and its configuration digest mean the same on every machine, and
/// nothing outside the service is written, copied or adopted.
fn path_ok(context: &mut Context<'_>, at: &str, path: &str) -> bool {
    let trimmed = path.strip_suffix('/').unwrap_or(path);
    let problem = if path.contains('\\') {
        Some("uses `\\`; write paths with `/`".to_owned())
    } else if path.starts_with('/') || path.as_bytes().get(1) == Some(&b':') {
        Some("is absolute; paths are relative to the SDK file".to_owned())
    } else if trimmed
        .split('/')
        .any(|segment| matches!(segment, "" | "." | ".."))
    {
        Some("leaves the SDK file's directory or has an empty, `.` or `..` segment".to_owned())
    } else {
        trimmed
            .split('/')
            .find_map(segment_problem)
            .map(|problem| format!("has a segment that {problem}"))
    };
    if let Some(problem) = &problem {
        context.error(at, format!("{path:?} {problem}"));
    }
    problem.is_none()
}

/// Why one path segment is not a plain file name on every platform, if it
/// is not. A drive prefix (`C:x`) would replace the path it is joined to on
/// Windows, a trailing `.` or space names the same file as without it, and
/// a device name is no file at all.
fn segment_problem(segment: &str) -> Option<&'static str> {
    if segment
        .chars()
        .any(|c| c.is_control() || matches!(c, ':' | '<' | '>' | '"' | '|' | '?' | '*'))
    {
        return Some("holds `:`, a control character or one of `<>\"|?*`");
    }
    if segment.ends_with(['.', ' ']) {
        return Some("ends in `.` or a space");
    }
    let stem = segment
        .split('.')
        .next()
        .unwrap_or(segment)
        .trim_end()
        .to_ascii_uppercase();
    let device = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ((stem.starts_with("COM") || stem.starts_with("LPT"))
        && stem.len() == 4
        && stem.as_bytes()[3].is_ascii_digit()
        && stem.as_bytes()[3] != b'0');
    if device {
        return Some("is a reserved device name");
    }
    let mut components = std::path::Path::new(segment).components();
    match (components.next(), components.next()) {
        (Some(std::path::Component::Normal(_)), None) => None,
        _ => Some("is not a plain file name"),
    }
}

/// `out` as a comparable key: no trailing `/`, and upper-cased, since a
/// case-insensitive filesystem (Windows, macOS) writes `Gen` and `gen` to
/// one directory, and NTFS folds by uppercase (`ς` and `σ` are both `Σ`).
/// Aliases a name cannot show (8.3 short names, links) are caught when
/// generating, by [`SdkFile::check_outs`](super::SdkFile::check_outs).
pub(super) fn out_key(out: &str) -> String {
    out.strip_suffix('/').unwrap_or(out).to_uppercase()
}
