//! Generated samples pass the model's own validation: the property every
//! `tests/models` package is held to, and what each sampling rule yields.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crucible_codegen_common::describe;
use crucible_codegen_common::sample::{self, Fallback, Sample, Variant};
use crucible_ir::ir::{Bindings, Constraints, Service, Shape};
use crucible_ir::{Audience, ShapeId, project};
use serde_json::{Value, json};

fn models() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models")
}

fn load(name: &str) -> Service {
    crucible_lang::load_service(&models().join(name))
        .unwrap_or_else(|rendered| panic!("{name}:\n{rendered}"))
}

/// The packages written for the compiler to refuse: nothing to sample.
const REFUSED: &[&str] = &[
    "doc-links-unresolved",
    "example-range-refusals",
    "external-docs-refusals",
    "number-literal-refusals",
    "patterns-unsupported",
    "recursive-output",
    "resource-entry-refusals",
    "sender-signed-refusals",
];

/// Every fallback a package's samples report, as `package: shape: path`.
/// Anything else is a value the generator should have made.
const FALLBACKS: &[&str] = &[
    // A word boundary, and a class that holds nothing.
    "patterns: example.patterns#CheckInput$boundary: CheckInput.boundary",
    "patterns: example.patterns#CheckInput$boundary: input.boundary",
    "patterns: example.patterns#CheckInput$empty: CheckInput.empty",
    "patterns: example.patterns#CheckInput$empty: input.empty",
    // Lengths past the sample's limits.
    "samples: example.samples#Archive: Archive",
    "samples: example.samples#BulkInput$archive: BulkInput.archive",
    "samples: example.samples#BulkInput$archive: input.archive",
    "samples: example.samples#BulkInput$essay: BulkInput.essay",
    "samples: example.samples#BulkInput$essay: input.essay",
    "samples: example.samples#BulkInput$labels: BulkInput.labels",
    "samples: example.samples#BulkInput$labels: input.labels",
    "samples: example.samples#BulkInput$limits: BulkInput.limits",
    "samples: example.samples#BulkInput$limits: input.limits",
    "samples: example.samples#Essay: Essay",
    "samples: example.samples#ManyLabels: ManyLabels",
    "samples: example.samples#ManyLimits: ManyLimits",
    // Header values HTTP cannot carry intact (only where they are headers).
    "samples: example.samples#BulkInput$headerBell: input.headerBell",
    "samples: example.samples#BulkInput$headerBlank: input.headerBlank",
    // A word boundary.
    "samples: example.samples#Bounded: Bounded",
    "samples: example.samples#CheckInput$bounded: CheckInput.bounded",
    "samples: example.samples#CheckInput$bounded: input.bounded",
];

/// Every package, as the server and as external readers see it; a package
/// that does not load must be one of [`REFUSED`].
fn services() -> Vec<(String, Service)> {
    let mut dirs: Vec<_> = std::fs::read_dir(models())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.join("crucible.toml").is_file())
        .collect();
    dirs.sort();
    let mut services = Vec::new();
    let mut refused = Vec::new();
    for dir in dirs {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let Ok(service) = crucible_lang::load_service(&dir) else {
            refused.push(name);
            continue;
        };
        if let Ok(external) = project(&service, Audience::External) {
            services.push((format!("{name} (external)"), external));
        }
        services.push((name, service));
    }
    assert_eq!(refused, REFUSED, "the packages that do not load");
    services
}

/// The members a binding sends as text, as the description passes them.
fn text(b: &Bindings) -> Vec<&str> {
    b.labels
        .iter()
        .map(String::as_str)
        .chain(b.query.iter().chain(&b.headers).map(|q| q.member.as_str()))
        .collect()
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
/// no fallback explains, and the fallbacks as `name: shape: path`.
fn check_service(
    name: &str,
    service: &Service,
    failures: &mut Vec<String>,
    fallbacks: &mut BTreeSet<String>,
) {
    let mut check = |what: String, target: Option<&ShapeId>, c: &Constraints, s: Sample| {
        for problem in unexplained(service, path_of(&what), target, c, &s) {
            failures.push(format!("{name}: {what}: {problem}"));
        }
        for fallback in &s.fallbacks {
            fallbacks.insert(format!("{name}: {}: {}", fallback.shape, fallback.path));
        }
    };
    for op in &service.operations {
        for variant in [Variant::Full, Variant::Minimal] {
            let input = sample::structure(
                service,
                op.input.as_ref(),
                variant,
                "input",
                &text(&op.request),
            );
            check(
                format!("{} input", op.id),
                op.input.as_ref(),
                &none(),
                input,
            );
        }
        let output = sample::structure(
            service,
            op.output.as_ref(),
            Variant::Full,
            "output",
            &text(&op.response),
        );
        check(
            format!("{} output", op.id),
            op.output.as_ref(),
            &none(),
            output,
        );
    }
    for (id, shape) in &service.shapes {
        if let Shape::Structure(s) = shape
            && let Some(info) = &s.error
        {
            let headers: Vec<&str> = info.headers.iter().map(|h| h.member.as_str()).collect();
            let error = sample::structure(service, Some(id), Variant::Full, "error", &headers);
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
    let mut fallbacks = BTreeSet::new();
    for (name, service) in services() {
        // The external projection reports what the full model does.
        let name = name.trim_end_matches(" (external)").to_owned();
        check_service(&name, &service, &mut failures, &mut fallbacks);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let expected: BTreeSet<String> = FALLBACKS.iter().map(|f| (*f).to_owned()).collect();
    assert_eq!(
        fallbacks,
        expected,
        "the fallbacks, as reported:\n{}",
        fallbacks
            .iter()
            .map(|f| format!("    {f:?},"))
            .collect::<Vec<_>>()
            .join("\n")
    );
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
    let mut fallbacks = BTreeSet::new();
    for dir in std::env::split_paths(&paths) {
        let service = crucible_lang::load_service(&dir)
            .unwrap_or_else(|rendered| panic!("{}:\n{rendered}", dir.display()));
        let name = dir.display().to_string();
        println!("{name}: {} operations", service.operations.len());
        check_service(&name, &service, &mut failures, &mut fallbacks);
        let external = project(&service, Audience::External).unwrap();
        check_service(&name, &external, &mut failures, &mut fallbacks);
    }
    println!("{} fallbacks", fallbacks.len());
    for fallback in &fallbacks {
        println!("  {fallback}");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
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

fn fallback_paths(sample: &Value) -> Vec<&str> {
    sample["fallbacks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect()
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
    // Always present, even where it is the same request.
    assert_eq!(sample["minimalInput"], sample["input"]);
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
fn large_and_exact_values_are_capped_or_kept_exact() {
    let sample = operation("Bulk");
    let body = &sample["input"]["body"];
    // Collections and strings stop at the sample's limits, and say so.
    assert_eq!(body["labels"].as_array().unwrap().len(), sample::MAX_ITEMS);
    assert_eq!(body["limits"].as_object().unwrap().len(), sample::MAX_ITEMS);
    assert_eq!(
        body["essay"].as_str().unwrap().chars().count(),
        sample::MAX_LENGTH
    );
    assert_eq!(
        fallback_paths(&sample),
        [
            "input.archive",
            "input.essay",
            "input.headerBell",
            "input.headerBlank",
            "input.labels",
            "input.limits",
        ]
    );
    let reasons: Vec<&str> = sample["fallbacks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["reason"].as_str().unwrap())
        .collect();
    assert!(
        reasons[4].contains("a minimum of 150 items is too many to show"),
        "{reasons:?}"
    );
    assert!(reasons[2].contains("header, label or query"), "{reasons:?}");
    // Past i64, and just below a value f64 rounds onto, exactly.
    assert_eq!(body["huge"].to_string(), "9223372036854775808");
    assert_eq!(body["tight"].to_string(), "0.29999999999999999999");
    // In a body a control character is fine; white space items differ.
    assert_eq!(body["bell"], "\u{1}");
    let blanks = body["blanks"].as_array().unwrap();
    assert_eq!(blanks.len(), 2);
    assert_ne!(blanks[0], blanks[1]);
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

/// A `bigInteger` example the server reads in any notation (`1e2`,
/// `100e-2`, `1.0`) is written in digits in a snippet, which every
/// language's big integer parses.
#[test]
fn a_big_integer_snippet_is_written_in_digits() {
    use crucible_codegen_common::snippet::{self, Kind};
    let service = load("samples");
    let owner: ShapeId = "example.samples#EdgesInput".parse().unwrap();
    let Some(Shape::Structure(input)) = service.shape(&owner) else {
        panic!("input")
    };
    let huge = input.member("huge").unwrap();
    let thousand_and_one = format!("1{}", "0".repeat(1000));
    let ten_thousand = format!("-1{}", "0".repeat(9999));
    for (written, digits) in [
        ("1e2", "100"),
        ("100e-2", "1"),
        ("-1.0", "-1"),
        ("1.5E30", "1500000000000000000000000000000"),
        (
            "123456789012345678901234567890",
            "123456789012345678901234567890",
        ),
        // Past the thousand-digit span arithmetic writes out, up to the
        // server's 10,000 digits.
        ("1e1000", thousand_and_one.as_str()),
        (thousand_and_one.as_str(), thousand_and_one.as_str()),
        ("-1e9999", ten_thousand.as_str()),
    ] {
        let value: Value = serde_json::from_str(written).unwrap();
        let Kind::Number { text, .. } = snippet::member(&service, huge, &value).kind else {
            panic!("{written}: not a number")
        };
        assert_eq!(text, digits, "{}", &written[..written.len().min(20)]);
    }
}

/// A paginated call's input leaves out the page token, whether a sample
/// sets it (Samples' `ListGraphs`, `SearchGraphs`) or an example might:
/// the first page sends none, and a sample's is no token the service
/// issued. The rest of the input stays.
#[test]
fn a_snippet_input_has_no_page_token() {
    use crucible_codegen_common::snippet::{self, Kind};
    let service = load("samples");
    for (name, token, kept) in [
        ("ListGraphs", "nextToken", "maxResults"),
        ("SearchGraphs", "pageToken", "query"),
    ] {
        let operation = service
            .operations
            .iter()
            .find(|op| op.id.name() == name)
            .unwrap();
        assert_eq!(operation.paginated.as_ref().unwrap().input_token, token);
        let call = snippet::call(&service, operation);
        assert!(call.example.is_none(), "{name}: the sample's input");
        let Some(Kind::Structure(members)) = call.input.map(|node| node.kind) else {
            panic!("{name}: a structure input")
        };
        let names: Vec<&str> = members.iter().map(|(m, _)| m.name.as_str()).collect();
        assert!(!names.contains(&token), "{name}: {names:?}");
        assert!(names.contains(&kept), "{name}: {names:?}");
    }
}
