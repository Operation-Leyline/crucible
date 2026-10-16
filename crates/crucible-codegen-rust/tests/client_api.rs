use crucible_codegen_rust::{ClientOptions, emit_client_package, generate_client};
mod common;

#[test]
fn sdk_exposes_models_and_exceptions_directly() {
    let service = common::load("weather");
    let code = generate_client(&service).unwrap();
    assert!(code.contains("pub use model::{"));
    assert!(code.contains("pub use errors::{"));
    assert!(!code.contains("pub mod model"));
    assert!(!code.contains("pub mod errors"));
    assert!(code.contains("GetCityInput, GetCityOutput"));
    assert!(code.contains("ConflictError, NotFoundError"));
    assert!(code.contains("pub async fn get_city(&self, input: GetCityInput)"));
    assert!(code.contains(
        "Result<GetCityOutput, ::crucible_client_runtime_rust::__generated::Error<GetCityError>>"
    ));
}

#[test]
fn flattened_names_diagnose_collisions_before_emitting_code() {
    for name in [
        "GetCity",
        "GetCityError",
        "WeatherClient",
        "WeatherException",
        "model",
        "errors",
        "runtime",
        "GET_CITY_OPERATION",
        "__redaction",
    ] {
        let mut service = common::load("weather");
        common::rename(&mut service, &[("example.weather#GetCityInput", name)]);
        let diagnostics = generate_client(&service).unwrap_err().to_string();
        assert!(
            diagnostics.contains(&format!("example.weather#{name}")),
            "{diagnostics}"
        );
        assert!(diagnostics.contains("collides"), "{diagnostics}");
        assert!(emit_client_package(&service, &ClientOptions::default()).is_err());
    }
}

#[test]
fn modeled_errors_colliding_with_operation_errors_are_diagnosed() {
    let mut service = common::load("weather");
    common::rename(
        &mut service,
        &[("example.weather#NotFoundError", "GetCityError")],
    );
    let diagnostics = generate_client(&service).unwrap_err().to_string();
    assert!(
        diagnostics.contains("example.weather#GetCityError"),
        "{diagnostics}"
    );
    assert!(diagnostics.contains("collides"), "{diagnostics}");
}

#[test]
fn flat_exports_are_deterministic() {
    let expected = generate_client(&common::load("weather")).unwrap();
    let service = common::load_with("weather", |package| package.files.reverse());
    assert_eq!(expected, generate_client(&service).unwrap());
}

#[test]
fn sensitive_enum_debug_does_not_rewrite_documentation() {
    let service = common::load_with("weather", |package| {
        for file in &mut package.files {
            file.text = file.text.replace(
                "        location?: Location\n",
                "        location?: Location\n        secretEnum?: SecretEnum\n        secretIntEnum?: SecretIntEnum\n",
            );
        }
        package.files.push(crucible_lang::SourceFile {
            path: "secrets.cru".into(),
            text: r#"
/// Debug, for example #[derive(Debug, Clone)]
sensitive enum SecretEnum {
    /// Debug, member documentation
    Known = "known"
}

/// Debug, for example #[derive(Debug, Clone)]
sensitive enum SecretIntEnum: integer {
    /// Debug, member documentation
    Known = 1
}
"#
            .into(),
        });
    });
    let code = generate_client(&service).unwrap();
    assert_eq!(
        // Model docs are escaped for rustdoc (a bare `[x]` is no link), and
        // otherwise left as written.
        code.matches(r"/// Debug, for example #\[derive(Debug, Clone)]")
            .count(),
        2
    );
    assert_eq!(code.matches("/// Debug, member documentation").count(), 2);
    for name in ["SecretEnum", "SecretIntEnum"] {
        let declaration = format!("pub enum {name} {{");
        let lines: Vec<_> = code.lines().map(str::trim).collect();
        let index = lines.iter().position(|line| *line == declaration).unwrap();
        assert_eq!(lines[index - 1], "#[non_exhaustive]");
        assert!(lines[index - 2].starts_with("#[derive(Clone,"));
        assert!(code.contains(&format!("impl ::std::fmt::Debug for {name}")));
    }
}
