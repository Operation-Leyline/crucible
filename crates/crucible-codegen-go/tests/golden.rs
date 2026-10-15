use crucible_codegen_go::{Options, emit, emit_without_permissions};
use crucible_smithy::Model;

#[test]
fn service_exception_is_exported_with_go_type_spelling() {
    let mut service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    service.id = crucible_smithy::ShapeId::new("example.weather", "weather_api");
    let files = emit(
        &service,
        &Options {
            module_path: "example.com/weather".into(),
            package_name: "weather".into(),
            endpoint: None,
        },
    )
    .unwrap();
    let runtime = &files
        .iter()
        .find(|file| file.path == "runtime.go")
        .unwrap()
        .contents;
    assert!(runtime.contains("type WeatherApiException interface"));
}
use std::{fs, path::Path};

#[test]
fn service_client_and_constructor_names_cannot_collide_with_models() {
    for name in ["WeatherClient", "NewWeatherClient"] {
        let mut model: serde_json::Value = serde_json::from_str(include_str!(
            "../../crucible-smithy/tests/fixtures/weather.json"
        ))
        .unwrap();
        model["shapes"]["example.weather#Weather"]["rename"] =
            serde_json::json!({"example.weather#CitySummary":name});
        let service = Model::from_json(&model.to_string())
            .unwrap()
            .lower(None)
            .unwrap();
        let diagnostics = emit(
            &service,
            &Options {
                module_path: "example.com/weather".into(),
                package_name: "weather".into(),
                endpoint: None,
            },
        )
        .err()
        .unwrap();
        assert!(
            diagnostics
                .to_string()
                .contains("collides with another declaration")
        );
    }
}

#[test]
fn weather_output_is_deterministic() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let options = Options {
        module_path: "example.com/weather".into(),
        package_name: "weather".into(),
        endpoint: Some("https://weather.example".into()),
    };
    let first = emit(&service, &options).unwrap();
    let second = emit(&service, &options).unwrap();
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
}

#[test]
fn invalid_package_configuration_is_collected() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let errors = emit(
        &service,
        &Options {
            module_path: "bad\nmodule".into(),
            package_name: "type".into(),
            endpoint: None,
        },
    )
    .err()
    .unwrap();
    assert_eq!(errors.len(), 2, "{errors}");
}

#[test]
fn accessor_names_do_not_collide_with_fields_or_other_accessors() {
    let mut model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap();
    let members = model["shapes"]["example.weather#CreateCityInput"]["members"]
        .as_object_mut()
        .unwrap();
    members.insert(
        "setName".into(),
        serde_json::json!({"target":"smithy.api#String"}),
    );
    members.insert(
        "getName".into(),
        serde_json::json!({"target":"smithy.api#String"}),
    );
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let files = emit(
        &service,
        &Options {
            module_path: "example.com/weather".into(),
            package_name: "weather".into(),
            endpoint: None,
        },
    )
    .unwrap();
    let models = &files
        .iter()
        .find(|f| f.path == "models.go")
        .unwrap()
        .contents;
    assert!(models.contains("func (m *CreateCityInput) SetName2(value CityName)"));
    assert!(models.contains("func (m *CreateCityInput) GetName2() CityName"));
    assert!(models.contains("func (m *CreateCityInput) SetSetName(value string)"));
}

#[test]
fn resource_only_types_do_not_change_sdk_files() {
    let model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap();
    let generated = |model: &serde_json::Value| {
        let service = Model::from_json(&model.to_string())
            .unwrap()
            .lower(None)
            .unwrap();
        emit(
            &service,
            &Options {
                module_path: "example.com/weather".into(),
                package_name: "weather".into(),
                endpoint: None,
            },
        )
        .unwrap()
        .into_iter()
        .map(|file| (file.path, file.contents))
        .collect::<std::collections::BTreeMap<_, _>>()
    };
    let baseline = generated(&model);
    for resource_only_shape in [
        serde_json::json!({"type":"structure","members":{"value":{"target":"smithy.api#String"}}}),
        serde_json::json!({"type":"bigDecimal"}),
        serde_json::json!({"type":"intEnum","members":{"ZERO":{"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":0}}}}),
        serde_json::json!({"type":"set","member":{"target":"smithy.api#String"}}),
    ] {
        let mut extended = model.clone();
        extended["shapes"]["example.weather#ResourceOnlyIdentifier"] =
            serde_json::json!({"type":"string"});
        extended["shapes"]["example.weather#City"]["identifiers"]["deploymentId"] =
            serde_json::json!({"target":"example.weather#ResourceOnlyIdentifier"});
        extended["shapes"]["example.weather#ResourceOnly"] = resource_only_shape.clone();
        extended["shapes"]["example.weather#City"]["properties"] =
            serde_json::json!({"deploymentState":{"target":"example.weather#ResourceOnly"}});
        assert_eq!(
            generated(&extended),
            baseline,
            "resource-only {} must not affect any SDK file",
            resource_only_shape["type"]
        );
    }
}

#[test]
fn weather_matches_goldens() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let files = emit(
        &service,
        &Options {
            module_path: "example.com/weather".into(),
            package_name: "weather".into(),
            endpoint: Some("https://weather.example".into()),
        },
    )
    .unwrap();
    for file in files {
        if matches!(
            file.path.as_str(),
            "runtime.go" | "auth.go" | "policy.go" | "telemetry.go"
        ) {
            continue;
        }
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(&file.path);
        if std::env::var_os("CRUCIBLE_BLESS").is_some() {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &file.contents).unwrap();
        }
        assert_eq!(
            fs::read_to_string(path)
                .unwrap_or_default()
                .replace("\r\n", "\n"),
            file.contents,
            "{}",
            file.path
        );
    }
}

fn policy_options() -> Options {
    Options {
        module_path: "example.com/policy".into(),
        package_name: "policy".into(),
        endpoint: None,
    }
}

#[test]
fn policy_models_generate_with_the_policy_runtime() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/runtime-policy.json"
    ))
    .unwrap()
    .lower_with_capabilities(None, &crucible_codegen_go::capabilities())
    .unwrap();
    let files = emit(
        &service,
        &Options {
            module_path: "example.com/policy".into(),
            package_name: "policy".into(),
            endpoint: None,
        },
    )
    .unwrap();
    for runtime in ["policy.go", "telemetry.go"] {
        assert!(files.iter().any(|f| f.path == runtime), "{runtime}");
    }
}

#[test]
fn runtime_policy_names_cannot_collide_with_models() {
    for name in [
        "RetryMode",
        "TelemetryHook",
        "AttemptFinishedEvent",
        "Clock",
    ] {
        let mut model: serde_json::Value = serde_json::from_str(include_str!(
            "../../crucible-smithy/tests/fixtures/weather.json"
        ))
        .unwrap();
        model["shapes"]["example.weather#Weather"]["rename"] =
            serde_json::json!({"example.weather#CitySummary":name});
        let service = Model::from_json(&model.to_string())
            .unwrap()
            .lower(None)
            .unwrap();
        let diagnostics = emit(&service, &policy_options()).err().unwrap();
        assert!(
            diagnostics
                .to_string()
                .contains("collides with another declaration"),
            "{name}"
        );
    }
}

#[test]
fn documentation_and_formatting_use_the_lowered_contract() {
    let service = Model::from_json(include_str!("../../../tests/fixtures/client-docs.json"))
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_go::capabilities())
        .unwrap();
    let files = emit(
        &service,
        &Options {
            module_path: "example.com/docs".into(),
            package_name: "docs".into(),
            endpoint: None,
        },
    )
    .unwrap();
    let models = &files
        .iter()
        .find(|f| f.path.ends_with("models.go"))
        .unwrap()
        .contents;
    let client = &files
        .iter()
        .find(|f| f.path.ends_with("client.go"))
        .unwrap()
        .contents;
    assert!(models.contains("https://example.com"));
    assert!(models.contains("line two"));
    assert!(models.contains("Label marker."));
    assert!(client.contains("Modeled errors: Problem"));
    assert!(client.contains("Construct") || client.contains("Construct with"));
    assert!(models.contains("String() string"));
}

#[test]
fn representation_method_collisions_are_diagnosed() {
    let service = Model::from_json(include_str!(
        "../../../tests/fixtures/client-docs-go-collisions.json"
    ))
    .unwrap()
    .lower_with_capabilities(None, &crucible_codegen_go::capabilities())
    .unwrap();
    let diagnostics = emit(
        &service,
        &Options {
            module_path: "example.com/docs".into(),
            package_name: "docs".into(),
            endpoint: None,
        },
    )
    .err()
    .unwrap();
    assert!(
        diagnostics
            .to_string()
            .contains("member collides with a Go model field or method")
    );
}

/// The typed permissions fixture (`tests/fixtures/permissions`) as a client
/// sees it (docs/permissions.md §6.3), hostile documentation included: every
/// generated file that names a permission, checked in under
/// `tests/golden/permissions/`. No file names an internal permission, and a
/// checked-in file no generated one matches any more is reported as stale
/// (blessing removes it).
#[test]
fn permissions_match_goldens() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/permissions");
    let compiled = crucible_lang::compile(&crucible_lang::load_package(&dir).unwrap(), None);
    let rendered = compiled.render();
    let full = compiled
        .service
        .unwrap_or_else(|| panic!("the fixture compiles:\n{rendered}"));
    // Every internal value, an alias of an internal permission's included.
    let internal: Vec<String> = full
        .permissions
        .values()
        .flat_map(|resource| {
            resource.permissions.iter().filter_map(move |p| {
                let primary = p
                    .alias_of
                    .as_deref()
                    .and_then(|symbol| resource.permission(symbol))
                    .unwrap_or(p);
                (primary.tier == crucible_smithy::ir::Tier::Internal).then(|| p.value.clone())
            })
        })
        .collect();
    assert!(
        !internal.is_empty(),
        "the fixture declares internal permissions"
    );
    let service = crucible_smithy::project(&full, crucible_smithy::Audience::External).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/permissions");
    let bless = std::env::var_os("CRUCIBLE_BLESS").is_some();
    let mut expected = std::collections::BTreeSet::new();
    for file in emit(
        &service,
        &Options {
            module_path: "example.com/leyline".into(),
            package_name: "leyline".into(),
            endpoint: None,
        },
    )
    .unwrap()
    {
        for value in &internal {
            assert!(
                !file.contents.contains(value.as_str()),
                "{} names {value}",
                file.path
            );
        }
        if !file.contents.contains("graph:read") {
            continue;
        }
        expected.insert(file.path.clone());
        let path = root.join(&file.path);
        if bless {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &file.contents).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(&path)
                .unwrap_or_default()
                .replace("\r\n", "\n"),
            file.contents,
            "{}; rerun with CRUCIBLE_BLESS=1 and read the diff",
            file.path
        );
    }
    assert!(!expected.is_empty(), "no generated file names a permission");
    fn checked_in(dir: &std::path::Path, prefix: &str, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
            if entry.path().is_dir() {
                checked_in(&entry.path(), &format!("{name}/"), out);
            } else {
                out.push(name);
            }
        }
    }
    let mut files = Vec::new();
    checked_in(&root, "", &mut files);
    let stale: Vec<String> = files
        .into_iter()
        .filter(|path| !expected.contains(path))
        .collect();
    if bless {
        for path in &stale {
            std::fs::remove_file(root.join(path)).unwrap();
        }
    } else {
        assert!(
            stale.is_empty(),
            "stale permission goldens {stale:?}; rerun with CRUCIBLE_BLESS=1"
        );
    }
}

/// The typed permissions fixture (`tests/fixtures/permissions`) projected for
/// a client, as `crucible generate` lowers it.
fn permissions_service() -> crucible_smithy::ir::Service {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/permissions");
    let compiled = crucible_lang::compile(&crucible_lang::load_package(&dir).unwrap(), None);
    let rendered = compiled.render();
    let full = compiled
        .service
        .unwrap_or_else(|| panic!("the fixture compiles:\n{rendered}"));
    crucible_smithy::project(&full, crucible_smithy::Audience::External).unwrap()
}

/// The fixture with a permission `{symbol}` added to `graph`, whose name
/// the language spells like an existing one's.
fn permissions_with(symbol: &str) -> crucible_smithy::ir::Service {
    let mut service = permissions_service();
    let graph = service
        .permissions
        .values_mut()
        .find(|resource| resource.name == "graph")
        .unwrap();
    let mut twin = graph.permissions[0].clone();
    twin.symbol = symbol.into();
    twin.value = "graph:twin".into();
    twin.alias_of = None;
    graph.permissions.push(twin);
    service
}

#[test]
fn permission_constants_that_go_spells_alike_are_diagnosed() {
    let options = Options {
        module_path: "example.com/leyline".into(),
        package_name: "leyline".into(),
        endpoint: None,
    };
    let constant = emit(&permissions_with("Read"), &options).err().unwrap();
    assert!(
        constant
            .to_string()
            .contains("spells the Go constant `GraphPermissionRead`"),
        "{constant}"
    );
    // A package that embeds the client and never names a permission (the
    // Terraform provider) neither checks the permissions nor ships them.
    let embedded = emit_without_permissions(&permissions_with("Read"), &options).unwrap();
    assert!(!embedded.iter().any(|f| f.path.starts_with("permissions/")));
    // Without permissions there is no sub-package.
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let files = emit(&service, &options).unwrap();
    assert!(!files.iter().any(|f| f.path.starts_with("permissions/")));
}

/// A hyphen in a permission's symbol (`mail-send`, which `.cru` allows)
/// starts a word of the constant's name, since no client language allows
/// one in a name (`constant_symbol`).
#[test]
fn hyphenated_permission_symbols_spell_words() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/permissions");
    let compiled = crucible_lang::compile(&crucible_lang::load_package(&dir).unwrap(), None);
    let full = compiled.service.expect("the fixture compiles");
    let mut service = crucible_smithy::project(&full, crucible_smithy::Audience::External).unwrap();
    let graph = service
        .permissions
        .values_mut()
        .find(|resource| resource.name == "graph")
        .unwrap();
    let mut hyphenated = graph.permissions[0].clone();
    hyphenated.symbol = "mail-send".into();
    hyphenated.value = "graph:mail-send".into();
    graph.permissions.push(hyphenated);
    let files = emit(
        &service,
        &Options {
            module_path: "example.com/leyline".into(),
            package_name: "leyline".into(),
            endpoint: None,
        },
    )
    .unwrap();
    let expected = r#"GraphPermissionMailSend"#;
    assert!(
        files.iter().any(|file| file.contents.contains(expected)),
        "no file spells `mail-send` as {expected}"
    );
}
