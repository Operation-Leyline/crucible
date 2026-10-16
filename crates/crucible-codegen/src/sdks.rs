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

/// The `crucible.sdks/1` document for `selected` SDKs of `file` (read with
/// [`crate::sdk::load`]: no environment), generated from `service` (the
/// full lowered model; each SDK projects its own audience). An
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
    let mut sdks = serde_json::Map::new();
    let mut errors = Vec::new();
    for sdk in selected {
        match entry(file, service, sdk) {
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

/// `sdks.json` for a reference at `audience`: the [`document`] of
/// [`visible_in`].
pub fn file(file: &SdkFile, service: &Service, audience: Audience) -> Result<crate::File, String> {
    let document = document(file, service, &visible_in(file, audience))?;
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
