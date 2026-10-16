use crucible_codegen_c::{File, Options, emit};
use crucible_ir::ir::Service;
use std::{fs, path::Path};

fn options() -> Options {
    Options {
        metadata: Default::default(),
        package_name: "weather_client".into(),
        prefix: "weather".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
    }
}
/// Compiles one `.cru` source as a package of its own.
fn lower(source: &str) -> Service {
    let package = crucible_lang::Package::new(
        "test",
        vec![crucible_lang::SourceFile {
            path: "test.cru".to_owned(),
            text: source.to_owned(),
        }],
    );
    let compiled = crucible_lang::compile(&package, None);
    let rendered = compiled.render();
    compiled.service.unwrap_or_else(|| {
        panic!(
            "the model compiles:
{rendered}"
        )
    })
}
fn weather_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/weather")
}
fn weather() -> Service {
    crucible_lang::load_service(&weather_dir()).unwrap()
}
fn write(files: &[File], directory: &Path) {
    for file in files {
        let path = directory.join(&file.path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &file.contents).unwrap();
    }
}
/// A service whose one `GET /` operation has `output`, beside `declarations`.
fn tiny(output: &str, declarations: &str) -> String {
    format!(
        "service Service {{
    version \"1\"
    protocol json
    operations [Get]
}}

readonly operation Get {{
    http GET \"/\"
    output {{ {output} }}
}}

{declarations}
"
    )
}

#[test]
fn weather_output_is_deterministic_and_native_fixture() {
    let service = weather();
    let first = emit(&service, &options()).unwrap();
    let second = emit(&service, &options()).unwrap();
    assert_eq!(
        first
            .iter()
            .map(|f| (&f.path, &f.contents))
            .collect::<Vec<_>>(),
        second
            .iter()
            .map(|f| (&f.path, &f.contents))
            .collect::<Vec<_>>()
    );
    let header = &first
        .iter()
        .find(|f| f.path.ends_with("/models.h"))
        .unwrap()
        .contents;
    assert!(header.contains(
        "struct { weather_presence state; const weather_models_city_summary * value; } city;"
    ));
    assert!(header.contains("weather_models_sky_known_clear"));
    let snapshot = first
        .iter()
        .filter(|file| {
            matches!(
                file.path.as_str(),
                "src/models.c"
                    | "src/client.c"
                    | "CMakeLists.txt"
                    | "cmake/Config.cmake.in"
                    | "README.md"
                    | "AGENTS.md"
                    | "Doxyfile"
                    | "crucible.json"
                    | "schema.json"
            ) || file.path.ends_with("/models.h")
                || file.path.ends_with("/client.h")
        })
        .map(|file| format!("=== {} ===\n{}\n", file.path, file.contents))
        .collect::<String>();
    let snapshot = format!("{}\n", snapshot.trim_end());
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/package.txt");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::create_dir_all(golden.parent().unwrap()).unwrap();
        fs::write(&golden, &snapshot).unwrap();
    }
    assert_eq!(
        snapshot,
        fs::read_to_string(golden).expect("package golden")
    );
    if let Some(path) = std::env::var_os("C_MODEL_TEST_OUTPUT") {
        write(&first, Path::new(&path));
    }
}

#[test]
fn empty_model_golden() {
    let model = "service Service {
    version \"1\"
    protocol json
}
";
    let files = emit(&lower(model), &options()).unwrap();
    let source = &files
        .iter()
        .find(|f| f.path == "src/models.c")
        .unwrap()
        .contents;
    assert_eq!(
        source,
        "#include \"weather_client/models.h\"\n#include <stddef.h>\n\nconst weather_shape *weather_shape_lookup(weather_string id) {\n    (void)id;\n    return NULL;\n}\n"
    );
}

#[test]
fn recursive_collections_and_structures_use_pointers() {
    let model = tiny(
        "node?: Node; choice?: Choice; loop?: Loop; document: document",
        r#"
structure Node {
    next?: Node
    children?: Nodes
    lookup?: NodeMap
}
list Nodes: Node
map NodeMap: string -> Node
list Loop: Node
union Choice {
    node: Node
    choice: Choice
    none
}
"#,
    );
    let files = emit(&lower(&model), &options()).unwrap();
    let header = &files
        .iter()
        .find(|f| f.path.ends_with("/models.h"))
        .unwrap()
        .contents;
    assert!(header.contains("const weather_models_node * const *items"));
    assert!(header.contains("const weather_models_node * value; } next"));
    if let Some(path) = std::env::var_os("C_MODEL_RECURSIVE_OUTPUT") {
        write(&files, Path::new(&path));
    }
}

#[test]
fn member_and_generated_symbol_collisions_are_diagnostics() {
    for model in [
        tiny("fooBar?: string; foo_bar?: string", ""),
        tiny("other?: GetOutputEncode", "structure GetOutputEncode {}"),
        tiny(
            "value?: Kind",
            "enum Kind {
    FOO = \"a\"
    Foo = \"b\"
}",
        ),
        tiny(
            "choice?: Choice",
            "union Choice { value: ChoiceTag }
structure ChoiceTag {}",
        ),
    ] {
        assert!(emit(&lower(&model), &options()).is_err());
    }
    for prefix in ["_bad", "a__b", "int", "bad-name"] {
        let mut config = options();
        config.prefix = prefix.into();
        assert!(emit(&weather(), &config).is_err());
    }
}

#[test]
fn enum_literals_preserve_nuls_backslashes_and_utf8() {
    let model = tiny(
        "kind?: Kind",
        r#"enum Kind { CONTROL = "\u{0}\\u0001\u{e9}??=" }"#,
    );
    let files = emit(&lower(&model), &options()).unwrap();
    let header = &files
        .iter()
        .find(|f| f.path.ends_with("/models.h"))
        .unwrap()
        .contents;
    assert!(header.contains("\\000\\\\u0001\\303\\251\\077\\077="));
    assert!(header.contains(", 12}"));
}

#[test]
fn client_symbols_head_bodies_and_package_options_are_diagnosed() {
    // Lowering refuses a HEAD request with a body, so the generator's own
    // check is reached by changing the method of a lowered POST.
    let model = r#"
service Service {
    version "1"
    protocol json
    operations [Get]
}

operation Get {
    http POST "/"
    input { body: string }
}
"#;
    let mut service = lower(model);
    service.operations[0].method = "HEAD".to_owned();
    assert!(emit(&service, &options()).is_err());
    for value in ["", "1..2", "1.2.3.4.5", "1.0-beta", "1\n0"] {
        let mut config = options();
        config.package_version = value.into();
        assert!(emit(&weather(), &config).is_err());
    }
    let mut config = options();
    config.package_name = "install".into();
    assert!(emit(&weather(), &config).is_err());
    let model = tiny(
        "",
        "readonly operation GetResult {\n    http GET \"/result\"\n    output { }\n}",
    )
    .replace("operations [Get]", "operations [Get, GetResult]");
    assert!(emit(&lower(&model), &options()).is_err());
}

/// yyjson documents a 40-byte minimum for the buffer `yyjson_write_number` fills.
#[test]
fn yyjson_number_buffers_meet_the_documented_minimum() {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("runtime");
    let mut checked = 0;
    for entry in fs::read_dir(runtime).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|extension| extension != "c") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        if !text.contains("yyjson_write_number") {
            continue;
        }
        for declaration in text.split("char ").skip(1) {
            let Some((name, rest)) = declaration.split_once('[') else {
                continue;
            };
            if name != "native" {
                continue;
            }
            let size: usize = rest.split(']').next().unwrap().parse().unwrap();
            assert!(size >= 40, "{}: char {name}[{size}]", path.display());
            checked += 1;
        }
    }
    assert!(checked >= 2, "found {checked} number buffers");
}

#[test]
fn documentation_and_sensitive_debug_metadata_are_emitted_from_ir() {
    let mut package = crucible_lang::load_package(&weather_dir()).unwrap();
    for file in &mut package.files {
        file.text = file.text.replace(
            "expectedRevision?: long",
            "expectedRevision?: long\n        debugModel?: DebugModel",
        );
    }
    package.files.push(crucible_lang::SourceFile {
        path: "native-docs.cru".to_owned(),
        text: include_str!("../../../tests/conformance/native-docs.cru").to_owned(),
    });
    let compiled = crucible_lang::compile(&package, None);
    let rendered = compiled.render();
    let service = compiled
        .service
        .unwrap_or_else(|| panic!("the model compiles:\n{rendered}"));
    let files = emit(&service, &options()).unwrap();
    let text = files
        .iter()
        .map(|file| file.contents.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("A **documented** value."));
    assert!(text.contains("A sensitive member."));
    assert!(text.contains("Known secret value."));
    assert!(text.contains("*&#47;") && text.contains("&#64;param"));
    assert!(text.contains("weather_models_debug_model_debug"));
    assert!(text.contains(".sensitive = true"));
}

#[test]
fn debug_helper_collisions_use_the_existing_collected_symbol_diagnostic() {
    let model = tiny("value?: GetOutputDebug", "structure GetOutputDebug {}");
    let diagnostics = emit(&lower(&model), &options()).err().unwrap().to_string();
    assert!(diagnostics.contains("C generated symbol collision"));
}

#[test]
fn client_config_docs_attach_once_to_the_correct_fields() {
    let header = include_str!("../runtime/client_runtime.h").replace("\r\n", "\n");
    let config = header
        .split("} crucible_generated_client_config;")
        .next()
        .unwrap();
    assert!(!config.contains("*/\n    /**"));
    assert!(config.contains("Retry-After can override it. */\n    double max_delay;"));
    assert!(config.contains("/** Total call deadline in seconds. */\n    double timeout;"));
    assert!(config.contains("/** Default request headers applied to every attempt. */\n    crucible_generated_headers headers;"));
}
