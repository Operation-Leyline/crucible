//! `crucible.sdks/1` (docs/docs.md): each declared SDK's public surface,
//! named by the generator that emits it, with the client-target settings
//! `crucible-sdk.toml` resolves to. A documentation site renders SDK
//! snippets from it beside `describe.json` instead of re-deriving each
//! language's casing, collision suffixes, type mapping, package names,
//! endpoint and default middleware.

use crucible_ir::Audience;
use crucible_ir::ir::Service;
use serde_json::{Value, json};

use crate::sdk::{Endpoint, Sdk, SdkFile};

mod http;

/// The contract's `format`.
pub const FORMAT: &str = "crucible.sdks/1";

/// Its file name beside `describe.json` in a documentation output.
pub const FILE_NAME: &str = "sdks.json";

/// The endpoint a client generated from `endpoint = { env = "NAME" }`
/// is described with. A released SDK always has its endpoint generated in
/// (`crucible sdk generate` and `build` refuse an unset variable), so its
/// snippets construct the client without one; which URL it holds is the
/// deployment's, never the description's, so the environment is not read.
const PLACEHOLDER_ENDPOINT: &str = "https://endpoint.invalid";

/// The SDKs a reference for `audience` describes: every SDK for an
/// internal reference, and only external SDKs for an external one, so an
/// external page never names an internal SDK's operations.
pub fn visible_in(file: &SdkFile, audience: Audience) -> Vec<&Sdk> {
    file.sdks
        .values()
        .filter(|sdk| audience == Audience::Internal || sdk.audience == Audience::External)
        .collect()
}

/// Those of `selected` that are lines of `service`'s release (decision
/// 76): the SDKs whose `api_version` names it, and those without one when
/// it is the current release (`service` may itself be a line). A reference
/// for a release describes these, so its snippets name the types and
/// operations its pages do.
pub fn lines_of<'a>(service: &Service, selected: &[&'a Sdk]) -> Vec<&'a Sdk> {
    let release = crucible_ir::versions::current_name(service);
    let current = crucible_ir::versions::line_current(service);
    let current = crucible_ir::versions::current_name(current.as_ref().unwrap_or(service));
    selected
        .iter()
        .copied()
        .filter(|sdk| sdk.api_version.as_deref().unwrap_or(current) == release)
        .collect()
}

/// The `crucible.sdks/1` document for `selected` SDKs of `file` (read with
/// [`crate::sdk::load`]: no environment), generated from `service` (the
/// full lowered model, or a line of it; each SDK is described from its own
/// release's line and projects its own audience, and the raw request is
/// `service`'s). An
/// `endpoint = { env = "NAME" }` is recorded by its variable's name, with a
/// null `endpoint`. Every SDK that cannot be described is reported, in one
/// error. Deterministic: the same file and model give the same bytes.
pub fn document(file: &SdkFile, service: &Service, selected: &[&Sdk]) -> Result<Value, String> {
    if let Some(declared) = &file.service
        && *declared != service.id.to_string()
    {
        return Err(format!(
            "{} declares SDKs for {declared}, not {}",
            file.path.display(),
            service.id
        ));
    }
    // Each SDK is described from its own line, found from the current
    // model whichever release `service` is.
    let current = crucible_ir::versions::line_current(service);
    let current = current.as_ref().unwrap_or(service);
    let mut sdks = serde_json::Map::new();
    let mut errors = Vec::new();
    for sdk in selected {
        match sdk.model(current).and_then(|line| entry(file, &line, sdk)) {
            Ok(entry) => {
                sdks.insert(sdk.name.clone(), entry);
            }
            Err(error) => errors.push(error),
        }
    }
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    // The raw request beside the SDKs: at the widest audience they
    // describe, with the headers their default middleware adds.
    let audience = if selected
        .iter()
        .any(|sdk| sdk.audience == Audience::Internal)
    {
        Audience::Internal
    } else {
        Audience::External
    };
    let mut middleware: Vec<(String, String)> = sdks
        .values()
        .filter_map(|entry| entry["middleware"].as_array())
        .flatten()
        .flat_map(|m| {
            let name = m["name"].as_str().unwrap_or_default().to_owned();
            m["headers"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(move |header| (name.clone(), header.to_owned()))
        })
        .collect();
    middleware.sort();
    middleware.dedup();
    let http = http::document(service, audience, &middleware)?;
    Ok(crate::sdk::canonical(&json!({
        "format": FORMAT,
        "service": service.id.to_string(),
        "sdks": sdks,
        "http": http,
    })))
}

/// The document as pretty JSON with sorted keys and a final newline.
pub fn render(document: &Value) -> String {
    let mut text = serde_json::to_string_pretty(document).expect("JSON serializes");
    text.push('\n');
    text
}

/// `sdks.json` for a reference at `audience` of `service`'s release: the
/// [`document`] of the SDKs [`visible_in`] it that are [`lines_of`] it.
pub fn file(file: &SdkFile, service: &Service, audience: Audience) -> Result<crate::File, String> {
    let visible = visible_in(file, audience);
    let document = document(file, service, &lines_of(service, &visible))?;
    Ok(crate::File {
        path: FILE_NAME.into(),
        contents: render(&document),
    })
}

fn entry(file: &SdkFile, service: &Service, sdk: &Sdk) -> Result<Value, String> {
    let mut sdk = sdk.clone();
    let endpoint_env = match &sdk.options.endpoint {
        Some(Endpoint::Env { name, .. }) => Some(name.clone()),
        _ => None,
    };
    if endpoint_env.is_some() {
        sdk.options.endpoint = Some(Endpoint::Url(PLACEHOLDER_ENDPOINT.into()));
    }
    // The README and license are package contents, not names: describing
    // needs neither file.
    sdk.metadata.readme = None;
    sdk.metadata.agents = None;
    sdk.metadata.license_file = None;
    let client = sdk
        .client(file, None)
        .map_err(|error| format!("sdk.{}: {error}", sdk.name))?;
    let mut surface = client
        .surface(service, sdk.audience)
        .map_err(|diagnostics| {
            format!(
                "sdk.{}:
{diagnostics}",
                sdk.name
            )
        })?;
    if endpoint_env.is_some() {
        surface.endpoint = None;
    }
    let mut entry = serde_json::to_value(&surface).expect("a surface is JSON data");
    let object = entry.as_object_mut().expect("a surface is an object");
    object.insert("name".into(), json!(sdk.name));
    object.insert(
        "audience".into(),
        json!(match sdk.audience {
            Audience::External => "external",
            Audience::Internal => "internal",
        }),
    );
    object.insert("endpointEnv".into(), json!(endpoint_env));
    Ok(entry)
}
