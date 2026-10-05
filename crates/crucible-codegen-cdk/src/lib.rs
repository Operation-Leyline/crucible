//! One jsii-compatible construct library from a resolved provisioning plan.

mod source;

use crucible_codegen_infra::Plan;
use crucible_smithy::{Diagnostics, ir::Service};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Publishing metadata is explicit: generation does not choose package ownership.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    pub package_name: String,
    pub package_version: String,
    pub author: String,
    pub repository_url: String,
    pub license: String,
    #[serde(default)]
    pub targets: Targets,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Targets {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub python: Option<Python>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub java: Option<Java>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dotnet: Option<DotNet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub go: Option<Go>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Python {
    pub dist_name: String,
    pub module: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Java {
    pub package: String,
    pub maven: Maven,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Maven {
    pub group_id: String,
    pub artifact_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DotNet {
    pub namespace: String,
    pub package_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Go {
    pub module_name: String,
    pub package_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub path: String,
    pub contents: String,
}

pub fn emit(service: &Service, plan: &Plan, options: &Options) -> Result<Vec<File>, Diagnostics> {
    let mut diagnostics = Diagnostics::default();
    for (field, value) in [
        ("package_name", &options.package_name),
        ("package_version", &options.package_version),
        ("author", &options.author),
        ("repository_url", &options.repository_url),
        ("license", &options.license),
    ] {
        if value.trim().is_empty() || value.chars().any(char::is_control) {
            diagnostics.push(
                &service.id,
                format!("CDK {field} must be nonempty and contain no control characters"),
            );
        }
    }
    let targets = serde_json::to_value(&options.targets).expect("metadata serializes");
    validate_metadata(&targets, "targets", service, &mut diagnostics);
    let source = source::emit(service, plan, &mut diagnostics);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let package = json!({
        "name": options.package_name,
        "version": options.package_version,
        "description": format!("CDK resources for {}", service.id),
        "author": { "name": options.author },
        "repository": { "type": "git", "url": options.repository_url },
        "license": options.license,
        "main": "lib/index.js", "types": "lib/index.d.ts",
        "files": ["lib", ".jsii"],
        "scripts": { "build": "jsii --strict", "package": "jsii-pacmak" },
        "peerDependencies": { "aws-cdk-lib": "^2.272.0", "constructs": "^10.8.1" },
        "devDependencies": { "aws-cdk-lib": "2.272.0", "constructs": "10.8.1", "jsii": "~5.9.0", "jsii-pacmak": "^1.0.0", "@types/node": "^22.0.0" },
        "jsii": { "outdir": "dist", "targets": targets, "tsc": { "rootDir": "src", "outDir": "lib" } },
    });
    Ok(vec![
        File {
            path: "src/index.ts".into(),
            contents: source,
        },
        File {
            path: "src/validation.ts".into(),
            contents: include_str!("../runtime/validation.ts").replace("\r\n", "\n"),
        },
        File {
            path: "package.json".into(),
            contents: format!("{}\n", serde_json::to_string_pretty(&package).unwrap()),
        },
        File {
            path: "README.md".into(),
            contents: format!(
                "# {}\n\nGenerated CDK constructs for `{}`. Supply the service token of an existing provider that implements the configured resource contract. This package does not create a provider or implement resource lifecycle operations.\n\nRun `npm install` and `npm run build` to compile the jsii assembly. Run `npm run package` to package configured language targets. Node.js is required by jsii consumers.\n\nModel constraints are checked for concrete values. Deployment-time tokens are checked by the provider. Replacement and deletion behavior belong to that provider; immutable documentation does not enforce replacement.\n",
                options.package_name, service.id
            ),
        },
    ])
}

fn validate_metadata(
    value: &serde_json::Value,
    path: &str,
    service: &Service,
    diagnostics: &mut Diagnostics,
) {
    match value {
        serde_json::Value::String(value)
            if value.trim().is_empty() || value.chars().any(char::is_control) =>
        {
            diagnostics.push(
                &service.id,
                format!("CDK {path} must be nonempty and contain no control characters"),
            )
        }
        serde_json::Value::Object(object) => {
            for (name, value) in object {
                validate_metadata(value, &format!("{path}.{name}"), service, diagnostics);
            }
        }
        _ => {}
    }
}
