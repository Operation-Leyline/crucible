use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const SERVICE: &str = r#"$version: "2"
namespace example.greeting
use crucible.protocols#json
use crucible.errors#ValidationException

@json
service Greeting {
    version: "1"
    operations: [SayHello]
    errors: [ValidationException]
}

@http(method: "POST", uri: "/hello", code: 200)
operation SayHello {
    input: SayHelloInput
    output: SayHelloOutput
}
"#;

const MODELS: &str = r#"$version: "2"
namespace example.greeting

@input
structure SayHelloInput {
    @required
    @length(min: 1)
    name: String
}

@mixin
structure GreetingFields {
    @required
    message: String
}

@output
structure SayHelloOutput with [GreetingFields] {}
"#;

fn fixture(directory: &Path) -> PathBuf {
    let sources = directory.join("model sources");
    fs::create_dir(&sources).unwrap();
    fs::write(sources.join("service.smithy"), SERVICE).unwrap();
    fs::write(sources.join("models.smithy"), MODELS).unwrap();
    sources
}

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_crucible"));
    // Direct source generation must also work outside a Cargo build script.
    command
        .env_remove("CARGO_MANIFEST_DIR")
        .env_remove("OUT_DIR");
    command
}

fn successful(output: Output) -> Output {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    output
}

fn files(path: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn collect(root: &Path, path: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                collect(root, &entry.path(), result);
            } else {
                result.insert(
                    entry.path().strip_prefix(root).unwrap().to_owned(),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    if path.is_file() {
        BTreeMap::from([(PathBuf::from("client.rs"), fs::read(path).unwrap())])
    } else {
        let mut result = BTreeMap::new();
        collect(path, path, &mut result);
        result
    }
}

#[test]
fn every_client_route_matches_compiled_ast_from_split_smithy_sources() {
    let directory = tempfile::tempdir().unwrap();
    let sources = fixture(directory.path());
    let ast = crucible_build::compile_smithy(&sources, directory.path().join("assembly")).unwrap();
    let assembled: serde_json::Value = serde_json::from_slice(&fs::read(&ast).unwrap()).unwrap();
    assert!(
        assembled["shapes"]["example.greeting#SayHelloOutput"]["members"]
            .get("message")
            .is_some()
    );
    assert!(
        assembled["shapes"]
            .get("example.greeting#GreetingFields")
            .is_none()
    );

    let targets: [(&str, &[&str]); 14] = [
        ("rust", &["--client"]),
        ("ts", &["--package-name", "@example/greeting"]),
        (
            "python",
            &[
                "--package-name",
                "greeting-sdk",
                "--module-name",
                "greeting_sdk",
            ],
        ),
        (
            "go",
            &[
                "--module-path",
                "example.com/greeting",
                "--package-name",
                "greeting",
            ],
        ),
        (
            "java",
            &[
                "--group-id",
                "example",
                "--artifact-id",
                "greeting-sdk",
                "--package-name",
                "example.greeting",
            ],
        ),
        (
            "kotlin",
            &[
                "--group-id",
                "example",
                "--artifact-id",
                "greeting-sdk",
                "--package-name",
                "example.greeting",
            ],
        ),
        (
            "csharp",
            &[
                "--namespace",
                "Example.Greeting",
                "--package-id",
                "Greeting.Sdk",
            ],
        ),
        (
            "swift",
            &[
                "--package-name",
                "GreetingSDK",
                "--module-name",
                "GreetingSDK",
            ],
        ),
        ("dart", &["--package-name", "greeting_sdk"]),
        (
            "php",
            &[
                "--package-name",
                "example/greeting-sdk",
                "--namespace",
                "Example\\Greeting",
            ],
        ),
        (
            "ruby",
            &[
                "--package-name",
                "greeting_sdk",
                "--module-name",
                "GreetingSDK",
            ],
        ),
        (
            "cpp",
            &["--package-name", "greeting_sdk", "--namespace", "greeting"],
        ),
        (
            "c",
            &["--package-name", "greeting_sdk", "--prefix", "greeting"],
        ),
        ("r", &["--package-name", "greetingSDK"]),
    ];
    for (target, options) in targets {
        let from_sources = directory.path().join(format!("{target} direct"));
        let from_ast = directory.path().join(format!("{target} ast"));
        for (input, output) in [(&sources, &from_sources), (&ast, &from_ast)] {
            successful(
                cli()
                    .args(["generate", target])
                    .arg(input)
                    .args(options)
                    .args(["--service", "example.greeting#Greeting"])
                    .arg("--out")
                    .arg(output)
                    .output()
                    .unwrap(),
            );
        }
        let actual = files(&from_sources);
        assert!(!actual.is_empty(), "{target} must emit client artifacts");
        assert_eq!(
            actual,
            files(&from_ast),
            "{target} direct sources must preserve AST generation semantics"
        );
    }
    assert_eq!(
        fs::read(sources.join("service.smithy")).unwrap(),
        SERVICE.as_bytes()
    );
    assert_eq!(
        fs::read(sources.join("models.smithy")).unwrap(),
        MODELS.as_bytes()
    );
    assert_eq!(
        fs::read_dir(&sources).unwrap().count(),
        2,
        "compilation must not write project files"
    );
}

#[test]
fn a_single_source_file_supports_check_and_generation_without_server_errors() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("single source.smithy");
    let models = MODELS.lines().skip(2).collect::<Vec<_>>().join("\n");
    fs::write(
        &source,
        format!(
            "{}\n{models}",
            SERVICE.replace("errors: [ValidationException]", "errors: []")
        ),
    )
    .unwrap();
    let checked = successful(
        cli()
            .arg("check")
            .arg(&source)
            .args(["--target", "rust"])
            .output()
            .unwrap(),
    );
    assert!(String::from_utf8_lossy(&checked.stdout).contains("example.greeting#Greeting"));
    let output = directory.path().join("client-sdk");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../crucible-client-runtime-rust")
        .canonicalize()
        .unwrap();
    successful(
        cli()
            .args(["generate", "rust"])
            .arg(&source)
            .arg("--client")
            .args([
                "--package-name",
                "greeting-client",
                "--package-version",
                "1.2.3",
            ])
            .arg("--runtime-path")
            .arg(&runtime)
            .arg("--out")
            .arg(&output)
            .output()
            .unwrap(),
    );
    assert!(
        fs::read_to_string(output.join("src/lib.rs"))
            .unwrap()
            .contains("GreetingClient")
    );
    let manifest = fs::read_to_string(output.join("Cargo.toml")).unwrap();
    assert!(manifest.contains("crucible-client-runtime-rust"));
    assert!(manifest.contains("[workspace]"));
    assert!(manifest.contains("name = \"greeting-client\""));
    assert!(manifest.contains("version = \"1.2.3\""));
    assert!(manifest.contains("path = "));
    assert!(!output.join("build.rs").exists());
}

#[test]
fn compiler_errors_preserve_existing_generated_packages() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("invalid.smithy");
    fs::write(
        &source,
        SERVICE.replace("input: SayHelloInput", "input: DoesNotExist"),
    )
    .unwrap();
    let output = directory.path().join("sdk");
    fs::create_dir(&output).unwrap();
    fs::write(output.join("consumer.txt"), "keep existing output").unwrap();
    let result = cli()
        .args(["generate", "ts"])
        .arg(&source)
        .args(["--package-name", "@example/greeting", "--out"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("DoesNotExist"));
    assert_eq!(
        files(&output),
        BTreeMap::from([(
            PathBuf::from("consumer.txt"),
            b"keep existing output".to_vec()
        )])
    );
}

#[test]
fn source_models_preserve_service_and_protocol_selection() {
    let directory = tempfile::tempdir().unwrap();
    let sources = fixture(directory.path());
    fs::write(
        sources.join("service.smithy"),
        format!("{SERVICE}\n@json\nservice Other {{ version: \"1\", operations: [SayHello] }}"),
    )
    .unwrap();
    let ambiguous = cli().arg("check").arg(&sources).output().unwrap();
    assert!(!ambiguous.status.success());
    assert!(String::from_utf8_lossy(&ambiguous.stderr).contains("service"));
    let selected = successful(
        cli()
            .arg("check")
            .arg(&sources)
            .args(["--service", "example.greeting#Greeting"])
            .output()
            .unwrap(),
    );
    assert!(String::from_utf8_lossy(&selected.stdout).contains("example.greeting#Greeting"));
}

#[test]
fn reusable_assembly_reports_errors_and_keeps_unchanged_ast_stable() {
    let directory = tempfile::tempdir().unwrap();
    let sources = fixture(directory.path());
    let output = directory.path().join("assembly");
    let compiler = |config: &Path, compiled: &Path| -> io::Result<()> {
        let configuration: serde_json::Value = serde_json::from_slice(&fs::read(config)?).unwrap();
        assert_eq!(
            configuration["projections"]["crucible"]["transforms"][0]["name"],
            "flattenAndRemoveMixins"
        );
        assert_eq!(configuration["imports"].as_array().unwrap().len(), 3);
        fs::create_dir_all(compiled.join("crucible/model"))?;
        fs::write(
            compiled.join("crucible/model/model.json"),
            b"{\"smithy\":\"2.0\",\"shapes\":{}}",
        )
    };
    let model = crucible_build::compile_smithy_with(&sources, &output, compiler).unwrap();
    let before = (
        fs::read(&model).unwrap(),
        fs::metadata(&model).unwrap().modified().unwrap(),
    );
    assert_eq!(
        crucible_build::compile_smithy_with(&sources, &output, compiler).unwrap(),
        model
    );
    assert_eq!(
        before,
        (
            fs::read(&model).unwrap(),
            fs::metadata(&model).unwrap().modified().unwrap()
        )
    );
    let error = crucible_build::compile_smithy_with(&sources, &output, |_, _| {
        Err(io::Error::other("compiler validation diagnostic"))
    })
    .unwrap_err();
    assert_eq!(error.to_string(), "compiler validation diagnostic");
    assert_eq!(before.0, fs::read(&model).unwrap());
    let nested = sources.join("generated");
    let error = crucible_build::compile_smithy_with(&sources, &nested, compiler).unwrap_err();
    assert!(error.to_string().contains("outside the source directory"));
    assert!(!nested.exists());
    let relative_output = PathBuf::from(format!("source-output-guard-{}", std::process::id()));
    assert!(!relative_output.exists());
    let error = crucible_build::compile_smithy_with(
        std::env::current_dir().unwrap(),
        &relative_output,
        compiler,
    )
    .unwrap_err();
    assert!(error.to_string().contains("outside the source directory"));
    assert!(!relative_output.exists());
    let missing_output = directory.path().join("unused output");
    assert!(
        crucible_build::compile_smithy_with(
            directory.path().join("missing"),
            &missing_output,
            compiler
        )
        .is_err()
    );
    assert!(!missing_output.exists());
}
