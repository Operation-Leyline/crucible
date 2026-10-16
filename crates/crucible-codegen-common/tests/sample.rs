//! Generated samples pass the model's own validation: the property every
//! `tests/models` package is held to, and what each sampling rule yields.

use std::path::{Path, PathBuf};

use crucible_codegen_common::describe;
use crucible_codegen_common::sample::{self, Fallback, Sample, Variant};
use crucible_ir::ir::{Constraints, Service, Shape};
use crucible_ir::{Audience, ShapeId, project};
use serde_json::{Value, json};

fn models() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models")
}

fn load(name: &str) -> Service {
    crucible_lang::load_service(&models().join(name))
        .unwrap_or_else(|rendered| panic!("{name}:\n{rendered}"))
}

/// Every package that compiles, as the server and as external readers see it.
fn services() -> Vec<(String, Service)> {
    let mut dirs: Vec<_> = std::fs::read_dir(models())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.join("crucible.toml").is_file())
        .collect();
    dirs.sort();
    let mut services = Vec::new();
    for dir in dirs {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        // Packages the compiler refuses have nothing to sample.
        let Ok(service) = crucible_lang::load_service(&dir) else {
            continue;
        };
        if let Ok(external) = project(&service, Audience::External) {
            services.push((format!("{name} (external)"), external));
        }
        services.push((name, service));
    }
    assert!(services.len() > 20, "the packages load");
    services
}

/// The model's checks on `sample`, less the problems at a reported fallback.
fn unexplained(
    service: &Service,
    path: &str,
    target: Option<&ShapeId>,
    constraints: &Constraints,
    sample: &Sample,
) -> Vec<String> {
    let problems = match target {
        Some(target) => {
            crucible_ir::validate::value(&service.shapes, path, target, constraints, &sample.value)
        }
        None if sample.value == json!({}) => Vec::new(),
        None => vec![format!("{path} is not empty")],
    };
    let covered = |problem: &str, fallback: &Fallback| {
        problem
            .strip_prefix(&fallback.path)
            .is_some_and(|rest| rest.starts_with([' ', ':', '.', '[']))
    };
    problems
        .into_iter()
        .filter(|problem| !sample.fallbacks.iter().any(|f| covered(problem, f)))
        .collect()
}

/// Checks every sample of `service`: each operation's full and minimal
/// input and its output, each error, and each shape. Collects the problems
/// no fallback explains, and the fallbacks.
fn check_service(
    name: &str,
    service: &Service,
    failures: &mut Vec<String>,
    fallbacks: &mut Vec<String>,
) {
    let mut check = |what: String, target: Option<&ShapeId>, c: &Constraints, s: Sample| {
        for problem in unexplained(service, path_of(&what), target, c, &s) {
            failures.push(format!("{name}: {what}: {problem}"));
        }
        for fallback in &s.fallbacks {
            fallbacks.push(format!(
                "{name}: {}: {:?}: {}",
                fallback.shape,
                fallback.pattern.as_deref().unwrap_or_default(),
                fallback.reason
            ));
        }
    };
    for op in &service.operations {
        for variant in [Variant::Full, Variant::Minimal] {
            let input = sample::structure(service, op.input.as_ref(), variant, "input");
            check(
                format!("{} input", op.id),
                op.input.as_ref(),
                &none(),
                input,
            );
        }
        let output = sample::structure(service, op.output.as_ref(), Variant::Full, "output");
        check(
            format!("{} output", op.id),
            op.output.as_ref(),
            &none(),
            output,
        );
    }
    for (id, shape) in &service.shapes {
        if matches!(shape, Shape::Structure(s) if s.error.is_some()) {
            let error = sample::structure(service, Some(id), Variant::Full, "error");
            check(format!("{id} error"), Some(id), &none(), error);
        }
        let constraints = match shape {
            Shape::Simple(s) => s.constraints.clone(),
            Shape::List(s) => s.constraints.clone(),
            Shape::Map(s) => s.constraints.clone(),
            _ => none(),
        };
        let value = sample::shape(service, id, Variant::Full);
        check(format!("{id} shape"), Some(id), &constraints, value);
    }
}

#[test]
fn every_sample_of_every_package_passes_the_models_validation() {
    let mut failures = Vec::new();
    let mut fallbacks = Vec::new();
    for (name, service) in services() {
        check_service(&name, &service, &mut failures, &mut fallbacks);
    }
    assert!(
        failures.is_empty(),
        "{}",
        failures.join(
            "
"
        )
    );
    // A fallback is a pattern or bound the generator cannot meet; only
    // the packages written to have one do.
    fallbacks.sort();
    fallbacks.dedup();
    for fallback in &fallbacks {
        assert!(
            ["patterns", "samples", "hostile-text"]
                .iter()
                .any(|package| fallback.starts_with(&format!("{package}:"))
                    || fallback.starts_with(&format!("{package} (external):"))),
            "unexpected fallback {fallback}
all:
{}",
            fallbacks.join(
                "
"
            )
        );
    }
}

/// The same check of the packages named in `CRUCIBLE_SAMPLE_PACKAGES`
/// (separated by the platform's path separator), such as a service's own
/// model: `CRUCIBLE_SAMPLE_PACKAGES=../leyline/model/api cargo test -p
/// crucible-codegen-common --test sample -- --nocapture`. Fallbacks are
/// printed, not failed.
#[test]
fn named_packages_pass_the_models_validation() {
    let Some(paths) = std::env::var_os("CRUCIBLE_SAMPLE_PACKAGES") else {
        return;
    };
    let mut failures = Vec::new();
    let mut fallbacks = Vec::new();
    for dir in std::env::split_paths(&paths) {
        let service = crucible_lang::load_service(&dir).unwrap_or_else(|rendered| {
            panic!(
                "{}:
{rendered}",
                dir.display()
            )
        });
        let name = dir.display().to_string();
        println!("{name}: {} operations", service.operations.len());
        check_service(&name, &service, &mut failures, &mut fallbacks);
        let external = project(&service, Audience::External).unwrap();
        check_service(
            &format!("{name} (external)"),
            &external,
            &mut failures,
            &mut fallbacks,
        );
    }
    fallbacks.sort();
    fallbacks.dedup();
    println!("{} fallbacks", fallbacks.len());
    for fallback in &fallbacks {
        println!("  {fallback}");
    }
    assert!(
        failures.is_empty(),
        "{}",
        failures.join(
            "
"
        )
    );
}

/// The path a sample's fallbacks use for `what`: `input`, `output`, `error`,
/// or a shape's name.
fn path_of(what: &str) -> &str {
    let (id, kind) = what.rsplit_once(' ').unwrap();
    match kind {
        "shape" => id.rsplit_once('#').map_or(id, |(_, name)| name),
        kind => kind,
    }
}

fn none() -> Constraints {
    Constraints::default()
}

#[test]
fn samples_are_deterministic() {
    for (_, service) in services() {
        let first = describe::describe(&service).map(|d| d.to_json());
        let second = describe::describe(&service).map(|d| d.to_json());
        assert_eq!(first.ok(), second.ok());
    }
}

fn operation(name: &str) -> Value {
    let service = load("samples");
    let description = describe::describe(&service).unwrap();
    description.operations[&format!("example.samples#{name}")]["sample"].clone()
}

#[test]
fn ids_timestamps_defaults_and_collections_follow_the_model() {
    let sample = operation("PutGraph");
    assert_eq!(sample["generated"], true);
    assert_eq!(sample["fallbacks"], json!([]));
    let input = &sample["input"];
    let id = input["labels"]["graphId"].as_str().unwrap();
    assert!(id.starts_with("g_") && id.len() == 28, "{id}");
    assert_eq!(
        input["uri"].as_str().unwrap().split('?').next().unwrap(),
        format!("/graphs/{id}")
    );
    assert_eq!(input["method"], "PUT");
    // Each timestamp in its resolved format, from one fixed instant: the
    // protocol's RFC 3339 in headers and queries, epoch seconds where the
    // member says so.
    assert_eq!(
        input["headers"]["If-Modified-Since"],
        "2026-01-15T09:30:00Z"
    );
    assert_eq!(
        input["query"],
        json!([{"name": "since", "value": "2026-01-15T09:30:00Z"}])
    );
    let body = &input["body"];
    assert_eq!(body["created"], 1_768_469_400);
    assert_eq!(body["updated"], "2026-01-15T09:30:00Z");
    // Defaults, enum and integer enum values, bounds.
    assert_eq!(body["tier"], "small");
    assert_eq!(body["count"], 42);
    assert_eq!(body["priority"], 1);
    assert_eq!(body["port"], 1024);
    assert_eq!(body["negative"], -5);
    assert_eq!(body["ratio"], 0.25);
    assert_eq!(body["price"], 10.5);
    assert_eq!(body["enabled"], false);
    assert_eq!(body["settings"], json!({}));
    assert_eq!(body["name"], "name");
    assert_eq!(body["description"], "description");
    let region = input["headers"]["X-Region"].as_str().unwrap();
    assert!(
        regex_match("^[a-z]{2}-[a-z]{4,9}-[0-9]$", region),
        "{region}"
    );
    // An idempotency token is a UUID.
    assert_eq!(body["clientToken"], "3f2b8c1e-5d4a-4e6f-9a7b-0c1d2e3f4a5b");
    // Minimum lengths, with distinct items and keys.
    let tags = body["tags"].as_array().unwrap();
    assert_eq!(tags.len(), 2);
    assert_ne!(tags[0], tags[1]);
    assert_eq!(body["limits"].as_object().unwrap().len(), 2);
    assert_eq!(body["regions"].as_array().unwrap().len(), 1);
    // A blob within its byte length, base64 encoded.
    assert_eq!(body["thumbnail"], "dGh1bWJuYWls");
    // The union's first member.
    assert_eq!(
        body["source"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["snapshot"]
    );
    // The minimal request carries what the server requires.
    let minimal = &sample["minimalInput"]["body"];
    assert_eq!(
        minimal.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["name", "tags"]
    );
    assert_eq!(
        sample["output"]["headers"]["Last-Modified"],
        "2026-01-15T09:30:00Z"
    );
}

#[test]
fn recursion_ends_at_the_shortest_valid_value() {
    let sample = operation("Walk");
    let body = &sample["input"]["body"];
    // The list would hold another root: it stays empty.
    assert_eq!(body["root"], json!({"name": "name", "children": []}));
    // The first arm leads straight back, so the sample takes the next.
    assert_eq!(body["expression"], json!({"value": 1}));
}

#[test]
fn payloads_and_errors_are_sampled_on_the_wire() {
    let sample = operation("GetGraph");
    assert_eq!(sample["output"]["payload"]["encoding"], "base64");
    assert!(
        sample["minimalInput"].is_null(),
        "the same as the full input"
    );
    let description = describe::describe(&load("samples")).unwrap();
    let busy = &description.shapes["example.samples#Busy"]["error"]["sample"];
    assert_eq!(busy["generated"], true);
    assert_eq!(busy["message"]["headers"]["Retry-After"], "1");
    assert_eq!(busy["message"]["headers"]["X-Crucible-Error"], "Busy");
    assert_eq!(busy["message"]["body"]["message"], "message");
}

#[test]
fn a_pattern_outside_the_subset_is_reported_not_passed_off() {
    let sample = operation("Check");
    let fallbacks = sample["fallbacks"].as_array().unwrap();
    assert_eq!(fallbacks.len(), 1, "{fallbacks:?}");
    assert_eq!(fallbacks[0]["path"], "input.bounded");
    assert_eq!(fallbacks[0]["shape"], "example.samples#CheckInput$bounded");
    assert_eq!(fallbacks[0]["pattern"], "\\bword\\b");
    assert!(
        fallbacks[0]["reason"]
            .as_str()
            .unwrap()
            .contains("word boundaries")
    );
}

#[test]
fn a_member_sample_is_on_the_wire_in_its_format() {
    let service = load("samples");
    let owner: ShapeId = "example.samples#PutGraphInput".parse().unwrap();
    let Some(Shape::Structure(input)) = service.shape(&owner) else {
        panic!("input")
    };
    let created = input.member("created").unwrap();
    let wire = sample::wire_member(&service, &owner, created, None, Variant::Full);
    assert_eq!(wire.value, 1_768_469_400);
    let updated = input.member("updated").unwrap();
    let header = sample::wire_member(
        &service,
        &owner,
        updated,
        Some(crucible_ir::ir::TimestampFormat::HttpDate),
        Variant::Full,
    );
    assert_eq!(header.value, "Thu, 15 Jan 2026 09:30:00 GMT");
}

fn regex_match(pattern: &str, text: &str) -> bool {
    regex::Regex::new(pattern).unwrap().is_match(text)
}
