#![cfg(all(feature = "cdk", feature = "terraform"))]

use std::fs;
use std::path::Path;
use std::process::Command;

use crucible_build::{Artifact, ModelPackage};
use crucible_build::{CdkOptions, Generator, ProvisioningBindings, TerraformOptions};
use serde_json::Value;

const CDK_MODEL: &str = include_str!("../../crucible-codegen-cdk/tests/fixtures/model.json");
const CDK_BINDINGS: &str = include_str!("../../crucible-codegen-cdk/tests/fixtures/bindings.json");
const CDK_CONFIG: &str = include_str!("../../crucible-codegen-cdk/tests/fixtures/options.json");
const TERRAFORM_MODEL: &str =
    include_str!("../../crucible-codegen-terraform/tests/fixtures/inventory.json");
const TERRAFORM_BINDINGS: &str =
    include_str!("../../crucible-codegen-terraform/tests/fixtures/bindings.json");

fn compile(path: &str) -> ModelPackage {
    crucible_build::model(path).compile_with(|_, _| unreachable!())
}

fn bindings(path: &str) -> ProvisioningBindings {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn equivalent(typed: &Artifact, generic: &Artifact, export: &str) {
    assert_eq!(typed.target, generic.target);
    assert_eq!(typed.files, generic.files);
    for path in &typed.files {
        let expected = fs::read(generic.path.join(path)).unwrap();
        assert_eq!(fs::read(typed.path.join(path)).unwrap(), expected, "{path}");
        assert_eq!(
            fs::read(Path::new(export).join(path)).unwrap(),
            expected,
            "{path}"
        );
    }
}

fn cdk_options() -> CdkOptions {
    let mut options: CdkOptions =
        serde_json::from_slice(&fs::read("cdk-options.json").unwrap()).unwrap();
    options.package_name = "@example/configured-constructs".into();
    options.package_version = "2.3.4".into();
    options.author = "Example publisher".into();
    options.repository_url = "https://example.com/constructs.git".into();
    options.license = "Apache-2.0".into();
    options
}

fn generate() {
    let cdk_model = compile("cdk-model.json");
    let defaults = cdk_model
        .cdk("default-constructs")
        .author("Example publisher")
        .repository_url("https://example.com/constructs.git")
        .license("MIT")
        .bindings("cdk-bindings.json")
        .try_generate()
        .unwrap();
    let default_package: Value =
        serde_json::from_slice(&fs::read(defaults.path.join("package.json")).unwrap()).unwrap();
    assert_eq!(default_package["name"], "default-constructs-cdk");
    assert_eq!(default_package["version"], "4.5.6");
    let options = cdk_options();
    let typed = cdk_model
        .cdk("constructs")
        .config("cdk-options.json")
        .package(&options.package_name)
        .version(&options.package_version)
        .author(&options.author)
        .repository_url(&options.repository_url)
        .license(&options.license)
        .targets(options.targets.clone())
        .bindings("cdk-bindings.json")
        .try_generate_into("exports/cdk")
        .unwrap();
    let generic = cdk_model
        .output(
            "constructs-generic",
            Generator::Cdk {
                bindings: bindings("cdk-bindings.json"),
                options,
            },
        )
        .generate();
    equivalent(&typed, &generic, "exports/cdk");
    let package: Value =
        serde_json::from_slice(&fs::read("exports/cdk/package.json").unwrap()).unwrap();
    assert_eq!(package["name"], "@example/configured-constructs");
    assert_eq!(package["version"], "2.3.4");
    assert!(package["jsii"]["targets"]["java"].is_object());
    let advanced = cdk_model
        .cdk("constructs-value")
        .options(cdk_options())
        .bindings_value(bindings("cdk-bindings.json"))
        .generate();
    assert_eq!(advanced.files, generic.files);
    for path in &advanced.files {
        assert_eq!(
            fs::read(advanced.path.join(path)).unwrap(),
            fs::read(generic.path.join(path)).unwrap()
        );
    }

    let mut alternatives: Value = serde_json::from_str(CDK_MODEL).unwrap();
    alternatives["shapes"]["example#Weather"]["traits"]["aws.protocols#restJson1"] =
        serde_json::json!({});
    fs::write(
        "protocol-alternatives.json",
        serde_json::to_vec(&alternatives).unwrap(),
    )
    .unwrap();
    let selected = crucible_build::model("protocol-alternatives.json")
        .protocol("crucible.protocols#json")
        .compile();
    let explicit = selected
        .cdk("selected-protocol")
        .options(cdk_options())
        .bindings_value(bindings("cdk-bindings.json"))
        .try_generate()
        .unwrap();
    for path in &explicit.files {
        assert_eq!(
            fs::read(explicit.path.join(path)).unwrap(),
            fs::read(generic.path.join(path)).unwrap()
        );
    }

    let terraform_model = compile("terraform-model.json");
    terraform_model
        .terraform("default-provider")
        .bindings("terraform-bindings.json")
        .try_generate()
        .unwrap();
    let typed = terraform_model
        .terraform("provider")
        .module_path("example.com/native-widget-provider")
        .provider_name("native_widget")
        .provider_address("registry.example.com/example/native-widget")
        .bindings("terraform-bindings.json")
        .generate_into("exports/terraform");
    let options = || TerraformOptions {
        module_path: "example.com/native-widget-provider".into(),
        provider_name: "native_widget".into(),
        provider_address: "registry.example.com/example/native-widget".into(),
    };
    let generic = terraform_model
        .output(
            "provider-generic",
            Generator::Terraform {
                bindings: bindings("terraform-bindings.json"),
                options: options(),
            },
        )
        .generate();
    equivalent(&typed, &generic, "exports/terraform");
    let advanced = terraform_model
        .terraform("provider-value")
        .options(options())
        .bindings_value(bindings("terraform-bindings.json"))
        .generate();
    for path in &advanced.files {
        assert_eq!(
            fs::read(advanced.path.join(path)).unwrap(),
            fs::read(generic.path.join(path)).unwrap()
        );
    }
    assert!(
        fs::read_to_string("exports/terraform/go.mod")
            .unwrap()
            .contains("example.com/native-widget-provider")
    );
    assert!(
        fs::read_to_string("exports/terraform/main.go")
            .unwrap()
            .contains("registry.example.com/example/native-widget")
    );
}

fn failed_generation_preserves_exports() {
    let old_cdk = fs::read("exports/cdk/package.json").unwrap();
    let old_provider = fs::read("exports/terraform/main.go").unwrap();
    let cdk = compile("cdk-model.json");
    let terraform = compile("terraform-model.json");
    let error = cdk
        .cdk("missing")
        .try_generate_into("exports/cdk")
        .unwrap_err();
    assert!(
        error.contains("explicit provisioning bindings are required"),
        "{error}"
    );
    let error = terraform
        .terraform("missing")
        .try_generate_into("exports/terraform")
        .unwrap_err();
    assert!(
        error.contains("explicit provisioning bindings are required"),
        "{error}"
    );
    let error = terraform
        .terraform("unreadable")
        .bindings("absent.json")
        .try_generate_into("exports/terraform")
        .unwrap_err();
    assert!(
        error.contains("cannot read provisioning bindings"),
        "{error}"
    );
    fs::write("invalid-bindings.json", "{ invalid JSON").unwrap();
    let error = cdk
        .cdk("malformed")
        .options(cdk_options())
        .bindings("invalid-bindings.json")
        .try_generate_into("exports/cdk")
        .unwrap_err();
    assert!(error.contains("invalid provisioning bindings"), "{error}");
    fs::write("invalid-config.json", "{\"unknown\": true}").unwrap();
    let error = match cdk.cdk("invalid-config").try_config("invalid-config.json") {
        Ok(_) => panic!("invalid CDK metadata was accepted"),
        Err(error) => error,
    };
    assert!(error.contains("invalid CDK config"), "{error}");
    let error = cdk
        .cdk("missing-metadata")
        .bindings_value(bindings("cdk-bindings.json"))
        .try_generate_into("exports/cdk")
        .unwrap_err();
    assert!(error.contains("must be nonempty"), "{error}");
    fs::write("empty-bindings.json", "{\"resources\": {}}").unwrap();
    let error = cdk
        .cdk("empty-bindings")
        .options(cdk_options())
        .bindings("empty-bindings.json")
        .try_generate_into("exports/cdk")
        .unwrap_err();
    assert!(error.contains("bindings"), "{error}");
    let mut unsupported: Value = serde_json::from_str(TERRAFORM_BINDINGS).unwrap();
    unsupported["resources"]["example.infra#Widget"]["terraform"]["synchronous"] = false.into();
    fs::write(
        "unsupported-bindings.json",
        serde_json::to_vec(&unsupported).unwrap(),
    )
    .unwrap();
    let error = terraform
        .terraform("unsupported")
        .bindings("unsupported-bindings.json")
        .try_generate_into("exports/terraform")
        .unwrap_err();
    assert!(
        error.contains("synchronous") && error.contains("custom provider"),
        "{error}"
    );
    assert_eq!(fs::read("exports/cdk/package.json").unwrap(), old_cdk);
    assert_eq!(fs::read("exports/terraform/main.go").unwrap(), old_provider);

    let mut malformed: Value = serde_json::from_str(CDK_MODEL).unwrap();
    malformed["shapes"]["example#City"]["identifiers"]["cityId"]["target"] =
        "smithy.api#Integer".into();
    fs::write(
        "malformed-resource.json",
        serde_json::to_vec(&malformed).unwrap(),
    )
    .unwrap();
    let ordinary = compile("malformed-resource.json");
    #[cfg(feature = "go")]
    ordinary
        .go_client("ordinary-go-before")
        .try_generate()
        .unwrap();
    let failed = || {
        ordinary
            .cdk("malformed-resource")
            .options(cdk_options())
            .bindings_value(bindings("cdk-bindings.json"))
            .try_generate_into("exports/cdk")
            .unwrap_err()
    };
    let error = failed();
    assert!(
        error.contains("resource identifier `cityId` must target a defined string shape"),
        "{error}"
    );
    assert_eq!(failed(), error);
    #[cfg(feature = "go")]
    ordinary
        .go_client("ordinary-go-after")
        .try_generate()
        .unwrap();
    assert_eq!(fs::read("exports/cdk/package.json").unwrap(), old_cdk);
    for name in [
        "missing",
        "unreadable",
        "malformed",
        "missing-metadata",
        "empty-bindings",
        "unsupported",
        "malformed-resource",
    ] {
        assert!(
            !Path::new("build-output/outputs").join(name).exists(),
            "{name}"
        );
    }
}

#[test]
fn provisioning_probe() {
    let Ok(mode) = std::env::var("CRUCIBLE_PROVISIONING_PROBE") else {
        return;
    };
    match mode.as_str() {
        "generate" => generate(),
        "failures" => failed_generation_preserves_exports(),
        _ => panic!("unknown provisioning probe"),
    }
}

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for (path, contents) in [
        ("cdk-model.json", CDK_MODEL),
        ("cdk-bindings.json", CDK_BINDINGS),
        ("cdk-options.json", CDK_CONFIG),
        ("terraform-model.json", TERRAFORM_MODEL),
        ("terraform-bindings.json", TERRAFORM_BINDINGS),
    ] {
        fs::write(root.path().join(path), contents).unwrap();
    }
    root
}

fn probe(root: &Path, mode: &str) -> String {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "provisioning_probe", "--nocapture"])
        .current_dir(root)
        .env("CRUCIBLE_PROVISIONING_PROBE", mode)
        .env("CARGO_MANIFEST_DIR", root)
        .env("OUT_DIR", root.join("build-output"))
        .env("CARGO_PKG_VERSION", "4.5.6")
        .env("CARGO_PKG_AUTHORS", "")
        .env("CARGO_PKG_REPOSITORY", "")
        .env("CARGO_PKG_LICENSE", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn typed_provisioning_outputs_match_native_emitters_and_track_configuration() {
    let root = fixture();
    let log = probe(root.path(), "generate");
    for path in [
        "cdk-options.json",
        "cdk-bindings.json",
        "terraform-bindings.json",
    ] {
        assert!(
            log.contains(&format!(
                "cargo::rerun-if-changed={}",
                root.path().join(path).display()
            )),
            "{log}"
        );
    }
    assert!(log.contains("cargo::metadata=artifact_constructs="));
    assert!(log.contains("cargo::metadata=artifact_provider="));
    assert!(root.path().join("exports/cdk/src/index.ts").is_file());
    assert!(root.path().join("exports/terraform/main.go").is_file());

    let mut config: Value = serde_json::from_str(CDK_CONFIG).unwrap();
    config["targets"]["python"]["distName"] = "updated-constructs".into();
    fs::write(
        root.path().join("cdk-options.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let mut bindings: Value = serde_json::from_str(CDK_BINDINGS).unwrap();
    bindings["resources"]["example#City"]["name"] = "UpdatedConstruct".into();
    fs::write(
        root.path().join("cdk-bindings.json"),
        serde_json::to_vec(&bindings).unwrap(),
    )
    .unwrap();
    probe(root.path(), "generate");
    let package: Value =
        serde_json::from_slice(&fs::read(root.path().join("exports/cdk/package.json")).unwrap())
            .unwrap();
    assert_eq!(
        package["jsii"]["targets"]["python"]["distName"],
        "updated-constructs"
    );
    assert_eq!(package["version"], "2.3.4");
    assert!(
        fs::read_to_string(root.path().join("exports/cdk/src/index.ts"))
            .unwrap()
            .contains("UpdatedConstruct")
    );
}

#[test]
fn invalid_provisioning_configuration_preserves_existing_native_exports() {
    let root = fixture();
    probe(root.path(), "generate");
    probe(root.path(), "failures");
}
