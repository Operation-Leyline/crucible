//! `crucible.sdks/1` (docs/docs.md): each declared SDK's public surface,
//! named by the generator that emits it, with the client-target settings
//! `crucible-sdk.toml` resolves to. A documentation site renders SDK
//! snippets from it beside `describe.json` instead of re-deriving each
//! language's casing, collision suffixes, type mapping, package names,
//! endpoint and default middleware.

use crucible_ir::Audience;
use crucible_ir::ir::Service;
use serde_json::{Value, json};

use crate::sdk::{Endpoint, Errors, Sdk, SdkFile};

/// The contract's `format`.
pub const FORMAT: &str = "crucible.sdks/1";

/// Its file name beside `describe.json` in a documentation output.
pub const FILE_NAME: &str = "sdks.json";

/// The SDK file as a description reads it: [`crate::sdk::load`], then each
/// `endpoint = { env = "NAME" }` whose variable `env` sets read and
/// checked. One whose variable is unset stays unresolved: a description
/// records the variable's name, and generating never needs it.
pub fn load(
    path: &std::path::Path,
    env: impl Fn(&str) -> Option<String>,
) -> Result<SdkFile, Errors> {
    let mut file = crate::sdk::load(path)?;
    let set: Vec<String> = file
        .sdks
        .iter()
        .filter(|(_, sdk)| match &sdk.options.endpoint {
            Some(Endpoint::Env { name, .. }) => env(name).is_some_and(|value| !value.is_empty()),
            _ => false,
        })
        .map(|(name, _)| name.clone())
        .collect();
    if !set.is_empty() {
        file.resolve_endpoints(&set, env)?;
    }
    Ok(file)
}

/// The SDKs a reference for `audience` describes: every SDK for an
/// internal reference, and only external SDKs for an external one, so an
/// external page never names an internal SDK's operations.
pub fn visible_in(file: &SdkFile, audience: Audience) -> Vec<&Sdk> {
    file.sdks
        .values()
        .filter(|sdk| audience == Audience::Internal || sdk.audience == Audience::External)
        .collect()
}

/// The `crucible.sdks/1` document for `selected` SDKs of `file`, generated
/// from `service` (the full lowered model; each SDK projects its own
/// audience). An `endpoint = { env = "NAME" }` whose variable is not
/// resolved is recorded by name, with a null value. Every SDK that cannot
/// be described is reported, in one error.
pub fn describe(file: &SdkFile, service: &Service, selected: &[&Sdk]) -> Result<Value, String> {
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
    Ok(crate::sdk::canonical(&json!({
        "format": FORMAT,
        "service": service.id.to_string(),
        "sdks": sdks,
    })))
}

/// The document as pretty JSON with sorted keys and a final newline.
pub fn render(document: &Value) -> String {
    let mut text = serde_json::to_string_pretty(document).expect("JSON serializes");
    text.push('\n');
    text
}

/// `sdks.json` for a reference at `audience`: [`describe`] of
/// [`visible_in`].
pub fn file(file: &SdkFile, service: &Service, audience: Audience) -> Result<crate::File, String> {
    let document = describe(file, service, &visible_in(file, audience))?;
    Ok(crate::File {
        path: FILE_NAME.into(),
        contents: render(&document),
    })
}

fn entry(file: &SdkFile, service: &Service, sdk: &Sdk) -> Result<Value, String> {
    let mut sdk = sdk.clone();
    let endpoint_env = match &sdk.options.endpoint {
        Some(Endpoint::Env { name, value }) => {
            let name = name.clone();
            // Unresolved, the client is described without a generated-in
            // endpoint: it then reads one from its configuration.
            if value.is_none() {
                sdk.options.endpoint = None;
            }
            Some(name)
        }
        _ => None,
    };
    let client = sdk
        .client(file, None)
        .map_err(|error| format!("sdk.{}: {error}", sdk.name))?;
    let surface = client
        .surface(service, sdk.audience)
        .map_err(|diagnostics| format!("sdk.{}:\n{diagnostics}", sdk.name))?;
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
