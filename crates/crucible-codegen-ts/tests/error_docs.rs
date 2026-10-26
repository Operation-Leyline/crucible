use crucible_codegen_ts::{Options, emit};

fn options() -> Options {
    Options {
        package_name: "@example/errors".into(),
        package_version: "0.0.0".into(),
        client_name: None,
        endpoint: None,
        middleware: vec![],
        package_json: None,
        metadata: Default::default(),
    }
}

#[test]
fn doc_url_collision_is_diagnosed_only_for_configured_services() {
    let source = "service Example { version \"1\" protocol json errors [Problem] } error Problem { status 400 docUrl?: integer }";
    for configured in [false, true] {
        let source = if configured {
            source.replace(
                "protocol json",
                "protocol json errorDocs \"http://localhost/errors\"",
            )
        } else {
            source.into()
        };
        let package = crucible_lang::Package::new(
            "example.errors",
            vec![crucible_lang::SourceFile {
                path: "test.cru".into(),
                text: source,
            }],
        );
        let compiled = crucible_lang::compile(&package, None);
        let rendered = compiled.render();
        let service = compiled.service.unwrap_or_else(|| panic!("{rendered}"));
        match emit(&service, &options()) {
            Ok(files) => {
                assert!(!configured);
                assert!(
                    files
                        .iter()
                        .find(|f| f.path == "src/errors.ts")
                        .unwrap()
                        .contents
                        .contains("docUrl?: number")
                );
            }
            Err(error) => {
                assert!(configured);
                assert!(error.to_string().contains("docUrl"));
            }
        }
    }
}
