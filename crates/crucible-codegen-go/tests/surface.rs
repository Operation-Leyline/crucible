//! The surface names what the generated module declares.

use std::path::Path;

use crucible_codegen_common::surface::Surface;
use crucible_codegen_go::{File, Options, emit, surface};

fn load(name: &str) -> crucible_ir::ir::Service {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name);
    crucible_lang::load_service(&dir).unwrap_or_else(|rendered| panic!("{name}:\n{rendered}"))
}

fn options(endpoint: Option<&str>) -> Options {
    Options {
        module_path: "example.com/weather".into(),
        package_name: "weather".into(),
        endpoint: endpoint.map(str::to_owned),
    }
}

fn source(files: &[File], path: &str) -> String {
    files
        .iter()
        .find(|file| file.path == path)
        .unwrap_or_else(|| panic!("no {path}"))
        .contents
        .clone()
}

/// The text from `header` to the end of its declaration (`\n}\n`).
fn block<'a>(code: &'a str, header: &str) -> &'a str {
    let start = code.find(header).unwrap_or_else(|| panic!("no {header}"));
    let rest = &code[start..];
    &rest[..rest.find("\n}\n").map_or(rest.len(), |at| at + 3)]
}

/// `native` as the module itself writes it, without `qualifier`.
fn local(native: &str, qualifier: &str) -> String {
    native.replace(qualifier, "")
}

/// Every name the surface gives is declared where it should be: each
/// method on the client, taking its operation's input and calling its own
/// operation; each member inside its own struct, with the field's type.
fn assert_declared(service: &crucible_ir::ir::Service, options: &Options, surface: &Surface) {
    let files = emit(service, options).unwrap();
    let client = source(&files, "client.go");
    let models = source(&files, "models.go");
    let runtime = source(&files, "runtime.go");
    let package = &options.package_name;
    let qualifier = &surface.package.qualifier;
    // A snippet imports the module under its package name, unless that is
    // a standard-library name or a name a snippet program declares.
    let alias = if [
        "big", "bigInt", "client", "context", "ctx", "err", "errors", "fmt", "input", "log",
        "math", "os", "output", "page", "pages", "ptr", "time",
    ]
    .contains(&package.as_str())
    {
        format!("{package}sdk")
    } else {
        package.clone()
    };
    assert_eq!(qualifier, &format!("{alias}."));
    assert_eq!(
        surface.package.imports[0],
        format!("import {alias} \"{}\"", options.module_path)
    );
    assert!(
        client.starts_with(&format!("package {package}\n"))
            || client.contains(&format!("\npackage {package}\n"))
    );
    let name = surface
        .client
        .name
        .strip_prefix(qualifier.as_str())
        .unwrap();
    assert!(client.contains(&format!("type {name} struct ")));
    // The construct calls the emitted constructor with a `Config` whose
    // `Endpoint` field it sets, and handles its error result.
    assert!(client.contains(&format!(
        "func New{name}(config Config) (*{name}, error) {{"
    )));
    assert!(surface.client.fallible);
    assert!(
        surface
            .client
            .construct
            .starts_with(&format!("{alias}.New{name}({alias}.Config{{"))
    );
    if surface.client.construct.contains("Endpoint:") {
        assert!(block(&runtime, "type Config struct {").contains("\tEndpoint "));
    }
    assert_eq!(surface.operations.len(), service.operations.len());
    for (id, operation) in &surface.operations {
        let op = service
            .operations
            .iter()
            .find(|op| op.id.to_string() == *id)
            .unwrap();
        let input = operation.input.as_ref().map_or("Unit".to_owned(), |input| {
            format!("*{}", local(&input.type_name, qualifier))
        });
        let method = block(
            &client,
            &format!(
                "func (c *{name}) {}(ctx context.Context, input {input}) (",
                operation.method
            ),
        );
        assert!(
            method.contains(&format!("c.call(ctx, \"{}\", input)", op.name())),
            "{id}: {} calls {}",
            operation.method,
            op.name()
        );
        // The snippet is a program calling this method with a context, and
        // every struct it builds is one the module declares.
        assert!(
            operation.snippet.starts_with(
                "package main
"
            ),
            "{id}"
        );
        assert!(
            operation.snippet.contains(
                "
func main() {
"
            ),
            "{id}"
        );
        assert!(
            operation
                .snippet
                .contains(&format!("client.{}(ctx, ", operation.method)),
            "{id}: {}",
            operation.snippet
        );
        for snippet in std::iter::once(&operation.snippet).chain(&operation.paginated_snippet) {
            for (at, _) in snippet.match_indices(&format!("&{qualifier}")) {
                let rest = &snippet[at + 1 + qualifier.len()..];
                let name = &rest[..rest.find('{').unwrap()];
                assert!(
                    models.contains(&format!(
                        "type {name} struct {{
"
                    )),
                    "{id}: {name}"
                );
            }
        }
        assert_eq!(
            operation.paginated_snippet.is_some(),
            op.paginated.is_some(),
            "{id}"
        );
        for pages in operation.paginators.iter().flatten() {
            assert!(
                operation
                    .paginated_snippet
                    .as_ref()
                    .is_some_and(|snippet| snippet.contains(&format!("client.{pages}("))),
                "{id}: {pages}"
            );
            let paginator = block(
                &client,
                &format!("func (c *{name}) {pages}(input {input}) (*"),
            );
            assert!(paginator.contains("encodeModel("), "{id}: {pages}");
            assert!(
                client.contains(&format!(
                    "p.client.callValues(ctx, \"{}\", p.input)",
                    op.name()
                )),
                "{id}: {pages} pages through {}",
                op.name()
            );
        }
        for structure in [&operation.input, &operation.output].into_iter().flatten() {
            let declared = block(
                &models,
                &format!(
                    "type {} struct {{\n",
                    local(&structure.type_name, qualifier)
                ),
            );
            for member in &structure.members {
                assert!(
                    declared.contains(&format!(
                        "\n    {} {}\n",
                        member.name,
                        local(&member.type_name, qualifier)
                    )),
                    "{}.{}: {}",
                    structure.type_name,
                    member.name,
                    member.type_name
                );
                assert_eq!(
                    member.optional,
                    member
                        .type_name
                        .starts_with(&format!("{qualifier}Optional[")),
                    "{}.{}",
                    structure.type_name,
                    member.name
                );
                for (uses, import) in [
                    ("time.", "import \"time\""),
                    ("big.", "import \"math/big\""),
                ] {
                    if member.type_name.contains(uses) {
                        assert!(surface.package.imports.iter().any(|line| line == import));
                    }
                }
            }
        }
    }
}

#[test]
fn surface_names_what_the_module_declares() {
    for model in [
        "weather",
        "client-docs",
        "client-docs-surface-keywords",
        "client-docs-surface-defaults",
        "extended-types",
    ] {
        let service = load(model);
        for endpoint in [None, Some("https://weather.example")] {
            let options = options(endpoint);
            let surface = surface(&service, &options);
            assert_eq!(surface.language, "go");
            assert_eq!(surface.package.ecosystem, "go");
            assert_eq!(surface.package.name, "example.com/weather");
            assert_eq!(surface.package.import, "example.com/weather");
            assert_declared(&service, &options, &surface);
        }
    }
}

/// A package clause that differs from the import path's last element is
/// the alias the import and every qualified type use.
#[test]
fn the_package_name_qualifies_types() {
    let service = load("weather");
    let options = Options {
        module_path: "example.com/sdk/v2".into(),
        package_name: "forecast".into(),
        endpoint: None,
    };
    let surface = surface(&service, &options);
    assert_eq!(
        surface.package.imports[0],
        "import forecast \"example.com/sdk/v2\""
    );
    assert_eq!(surface.client.name, "forecast.WeatherClient");
    assert_eq!(
        surface.operations["example.weather#GetCity"]
            .input
            .as_ref()
            .unwrap()
            .type_name,
        "forecast.GetCityInput"
    );
    assert_declared(&service, &options, &surface);
}

/// A package named like a variable or import of the snippet program is
/// imported under another name, so the snippet's `client` stays the client.
#[test]
fn a_package_named_like_a_snippet_variable_is_aliased() {
    let service = load("weather");
    let options = Options {
        module_path: "example.com/client".into(),
        package_name: "client".into(),
        endpoint: None,
    };
    let surface = surface(&service, &options);
    assert_eq!(
        surface.package.imports[0],
        "import clientsdk \"example.com/client\""
    );
    let snippet = &surface.operations["example.weather#GetCity"].snippet;
    assert!(
        snippet.contains(
            "	clientsdk \"example.com/client\"
"
        ),
        "{snippet}"
    );
    assert!(snippet.contains("client, err := clientsdk.NewWeatherClient("));
    assert_declared(&service, &options, &surface);
}

/// The module carries a README and `AGENTS.md` naming its install line,
/// client and a complete call.
#[test]
fn the_module_documents_itself() {
    let service = load("weather");
    let files = emit(&service, &options(Some("https://weather.example"))).unwrap();
    let readme = source(&files, "README.md");
    assert!(readme.contains(
        "go get example.com/weather
"
    ));
    assert!(readme.contains("client.GetCity(ctx, input)"));
    assert!(readme.contains("client.ListCitiesPages(input)"));
    let agents = source(&files, "AGENTS.md");
    assert!(agents.contains("`weather.WeatherClient`"));
    assert!(agents.contains("errors.As"));
}

#[test]
fn weather_surface_spells_go() {
    let service = load("weather");
    let surface = surface(&service, &options(None));
    assert_eq!(surface.client.name, "weather.WeatherClient");
    assert_eq!(
        surface.client.construct,
        "weather.NewWeatherClient(weather.Config{Endpoint: endpoint})"
    );
    assert!(surface.client.fallible && !surface.client.statements);
    assert_eq!(
        surface.package.imports,
        ["import weather \"example.com/weather\""]
    );
    let configured = crucible_codegen_go::surface(&service, &options(Some("https://x.example")));
    assert_eq!(
        configured.client.construct,
        "weather.NewWeatherClient(weather.Config{})"
    );
    assert_eq!(configured.endpoint.as_deref(), Some("https://x.example"));
    let list = &surface.operations["example.weather#ListCities"];
    assert_eq!(list.method, "ListCities");
    assert_eq!(list.paginators, Some(vec!["ListCitiesPages".to_owned()]));
    let input = list.input.as_ref().unwrap();
    assert_eq!(input.type_name, "weather.ListCitiesInput");
    assert_eq!(input.style, "fields");
    assert_eq!(input.members[0].name, "NextToken");
    assert_eq!(input.members[0].type_name, "weather.Optional[string]");
    assert!(input.members[0].optional);
    let create = surface.operations["example.weather#CreateCity"]
        .input
        .as_ref()
        .unwrap();
    assert_eq!(create.members[0].type_name, "weather.CityName");
    assert_eq!(
        create.members[1].type_name,
        "weather.Optional[*weather.Location]"
    );
    assert!(
        surface.operations["example.weather#Ping"]
            .paginators
            .is_none()
    );
}

/// Go exports every member, so a reserved word becomes an ordinary
/// capitalized identifier.
#[test]
fn keywords_are_exported_identifiers() {
    let surface = surface(&load("client-docs-surface-keywords"), &options(None));
    let input = surface.operations["example.keywords#Describe"]
        .input
        .as_ref()
        .unwrap();
    let names: Vec<_> = input.members.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "In", "Object", "Type", "Default", "Self", "Class", "Fun", "Def", "Func", "End",
            "When", "Struct"
        ]
    );
}

/// Go refuses method collisions instead of suffixing them, so there is
/// never a surface for a module that does not generate.
#[test]
fn colliding_method_names_are_refused() {
    let service = load("client-docs-surface-collisions");
    assert!(emit(&service, &options(None)).is_err());
}

/// Go fills no modeled defaults: a member is absent only through its
/// `Optional` field, so a defaulted input member is not omittable (a plain
/// field sends its value), and the same holds for outputs.
#[test]
fn only_optional_fields_may_be_absent() {
    let surface = surface(&load("client-docs-surface-defaults"), &options(None));
    let operation = &surface.operations["example.defaults#Configure"];
    let optional = |members: &[crucible_codegen_common::surface::Member]| {
        members
            .iter()
            .map(|member| (member.member.clone(), member.optional))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        optional(&operation.input.as_ref().unwrap().members),
        [
            ("name".to_owned(), false),
            ("count".to_owned(), false),
            ("mode".to_owned(), false),
            ("token".to_owned(), true),
            ("note".to_owned(), true),
        ]
    );
    assert_eq!(
        optional(&operation.output.as_ref().unwrap().members),
        [
            ("name".to_owned(), false),
            ("count".to_owned(), false),
            ("note".to_owned(), true),
        ]
    );
}

/// A package named like a standard-library package a member type needs is
/// imported under another name, so `time.Time` stays the standard
/// library's and the module's types read `timesdk.`.
#[test]
fn a_package_named_like_the_standard_library_is_aliased() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/weather");
    let mut package = crucible_lang::load_package(&dir).unwrap();
    for file in &mut package.files {
        file.text = file.text.replace("\r\n", "\n").replace(
            "        location?: Location\n",
            "        location?: Location\n        at?: timestamp\n        huge?: bigInteger\n",
        );
    }
    let service = crucible_lang::compile(&package, None).service.unwrap();
    for name in ["time", "big"] {
        let options = Options {
            module_path: format!("example.com/{name}"),
            package_name: name.into(),
            endpoint: None,
        };
        let surface = surface(&service, &options);
        let alias = format!("{name}sdk");
        assert_eq!(
            surface.package.imports,
            [
                format!("import {alias} \"example.com/{name}\""),
                "import \"math/big\"".to_owned(),
                "import \"time\"".to_owned(),
            ]
        );
        assert_eq!(surface.client.name, format!("{alias}.WeatherClient"));
        assert!(
            surface
                .client
                .construct
                .starts_with(&format!("{alias}.NewWeatherClient("))
        );
        let input = surface.operations["example.weather#CreateCity"]
            .input
            .as_ref()
            .unwrap();
        let ty = |member: &str| {
            input
                .members
                .iter()
                .find(|entry| entry.member == member)
                .unwrap()
                .type_name
                .clone()
        };
        assert_eq!(ty("at"), format!("{alias}.Optional[time.Time]"));
        assert_eq!(ty("huge"), format!("{alias}.Optional[*big.Int]"));
        assert_declared(&service, &options, &surface);
    }
}
