use crucible_ir::ir::Service;
use crucible_policy::{Config, Severity, check, diff};

/// The `.cru` package the tests change (`tests/models/policy-contract`).
fn package() -> crucible_lang::Package {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/policy-contract");
    let mut package = crucible_lang::load_package(&dir).unwrap();
    // Checkouts may carry CRLF; edits are written against LF.
    for file in &mut package.files {
        file.text = file.text.replace("\r\n", "\n");
    }
    package
}

/// The contract with each `(old, new)` pair replaced in its sources, and
/// `extra` as one more source file.
fn contract(edits: &[(&str, &str)], extra: &str) -> Service {
    let mut package = package();
    for (old, new) in edits {
        let mut found = false;
        for file in &mut package.files {
            if file.text.contains(old) {
                file.text = file.text.replace(old, new);
                found = true;
            }
        }
        assert!(found, "the contract has no `{old}`");
    }
    if !extra.is_empty() {
        package.files.push(crucible_lang::SourceFile {
            path: "extra.cru".into(),
            text: extra.into(),
        });
    }
    let compiled = crucible_lang::compile(&package, None);
    let rendered = compiled.render();
    compiled
        .service
        .unwrap_or_else(|| panic!("the edited contract compiles:\n{rendered}"))
}

fn model() -> Service {
    contract(&[], "")
}

/// The contract with `new` where it says `old`.
fn with(old: &str, new: &str) -> Service {
    contract(&[(old, new)], "")
}

const INPUT: &str = "        node?: Node\n    }\n    output {";
const OUTPUT: &str = "        state?: State\n        node?: Node\n    }\n}";

fn wire() -> Config {
    Config::parse("schema_version = 1\n[compatibility]\nsdk_source = false\n").unwrap()
}

fn rules(old: &Service, new: &Service) -> Vec<String> {
    diff(old, new, &wire())
        .findings
        .into_iter()
        .map(|f| f.rule)
        .collect()
}

#[test]
fn configuration_rejects_unknown_fields_versions_patterns_and_exceptions() {
    for text in [
        "schema_version = 2",
        "schema_version = 1\n[rules.typo]",
        "schema_version = 1\n[rules.naming]\noperations_pattern = '['",
        "schema_version = 1\n[rules.naming]\noperations_pattern = '(?x)Get.* # verbs'",
        "schema_version = 1\n[rules.paths]\nprefix = '/v1/'",
        "schema_version = 1\n[[exceptions]]\nrule = 'typo'\nshape = 'example.api#Catalog'\nreason = 'intentional'",
        "schema_version = 1\n[[exceptions]]\nrule = 'paths.prefix'\nshape = 'example.api#GetItem'\nreason = ' '",
    ] {
        assert!(Config::parse(text).is_err(), "{text}");
    }
}

#[test]
fn static_rules_have_locations_severity_and_visible_exceptions() {
    let config = Config::parse(
        r#"
schema_version = 1
[rules.naming]
operations_pattern = 'List[A-Z].*'
[rules.paths]
prefix = '/v10'
segment_case = 'kebab-case'
[rules.documentation]
severity = 'warning'
targets = ['operations', 'errors']
[[exceptions]]
rule = 'naming.operations'
shape = 'example.api#GetItem'
reason = 'Existing operation name'
"#,
    )
    .unwrap();
    let report = check(&model(), &config);
    assert!(report.has_errors());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule == "naming.operations" && f.exception.is_some())
    );
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule == "documentation.required" && f.severity == Severity::Warning)
    );
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule == "paths.prefix" && f.shape == "example.api#GetItem")
    );
}

#[test]
fn prefix_matches_segment_boundaries_and_off_disables_a_group() {
    let value = with("\"/v1/items\"", "\"/v10/items\"");
    let config = Config::parse("schema_version = 1\n[rules.paths]\nprefix = '/v1'").unwrap();
    assert!(check(&value, &config).has_errors());
    let config =
        Config::parse("schema_version = 1\n[rules.paths]\nprefix = '/v1'\nseverity = 'off'")
            .unwrap();
    assert!(check(&value, &config).findings.is_empty());
}

#[test]
fn no_op_and_documentation_changes_pass_with_recursive_shapes() {
    let old = model();
    let new = with(
        "operation GetItem {",
        "/// Fetch an item.\noperation GetItem {",
    );
    assert!(diff(&old, &new, &Config::default()).findings.is_empty());
}

#[test]
fn removing_readonly_is_a_breaking_retry_contract_change() {
    let old = with("operation GetItem {", "readonly operation GetItem {");
    let new = model();
    let findings = diff(&old, &new, &Config::default()).findings;
    assert!(findings.iter().any(|finding| {
        finding.rule == "compatibility.operation"
            && finding.shape == "example.api#GetItem"
            && finding.message.contains("readonly")
    }));
}

#[test]
fn optional_inputs_pass_but_new_required_inputs_and_lost_output_guarantees_fail() {
    let old = model();
    let new = with(
        INPUT,
        &INPUT.replace("    }\n", "        extra?: string\n    }\n"),
    );
    assert!(rules(&old, &new).is_empty());
    let new = with(
        INPUT,
        &INPUT.replace("    }\n", "        extra: string\n    }\n"),
    );
    assert!(rules(&old, &new).contains(&"compatibility.required".into()));
    let new = with(
        "    output {\n        name: Name",
        "    output {\n        name?: Name",
    );
    assert!(rules(&old, &new).contains(&"compatibility.nullability".into()));
}

#[test]
fn defaulted_new_required_inputs_do_not_require_old_callers_to_send_them() {
    let old = model();
    let new = with(
        INPUT,
        &INPUT.replace("    }\n", "        extra: string = \"hello\"\n    }\n"),
    );
    assert!(!rules(&old, &new).contains(&"compatibility.required".into()));
}

#[test]
fn enum_direction_is_checked_independently_for_shared_input_and_output_shapes() {
    let old = model();
    let new = with(
        "    READY = \"ready\"\n",
        "    READY = \"ready\"\n    WAITING = \"waiting\"\n",
    );
    assert!(rules(&old, &new).contains(&"compatibility.enum".into()));
    let without_state = |extra: &[(&str, &str)]| {
        let mut edits = vec![(
            "        state?: State\n        node?: Node\n    }\n}",
            "        node?: Node\n    }\n}",
        )];
        edits.extend_from_slice(extra);
        contract(&edits, "")
    };
    let old_input = without_state(&[]);
    let new_input = without_state(&[(
        "    READY = \"ready\"\n",
        "    READY = \"ready\"\n    WAITING = \"waiting\"\n",
    )]);
    assert!(rules(&old_input, &new_input).is_empty());
    assert!(rules(&new_input, &old_input).contains(&"compatibility.enum".into()));
}

#[test]
fn tightening_input_constraints_and_loosening_output_constraints_fail() {
    let old = model();
    let new = with("length 1..100", "length 1..50");
    assert!(rules(&old, &new).contains(&"compatibility.constraints".into()));
    let new = with("length 1..100", "length 1..150");
    assert!(rules(&old, &new).contains(&"compatibility.constraints".into()));
}

#[test]
fn removed_operations_and_rebound_members_are_reported() {
    let old = model();
    let new = with("    operations [GetItem]\n", "");
    assert!(rules(&old, &new).contains(&"compatibility.operation".into()));
    let new = with(
        "        name?: Name\n        filter",
        "        @header(\"X-Name\")\n        name?: Name\n        filter",
    );
    assert!(rules(&old, &new).contains(&"compatibility.member".into()));
}

/// The `compatibility.sdk` findings, as `shape: message`.
fn sdk_findings(old: &Service, new: &Service) -> Vec<String> {
    diff(old, new, &Config::default())
        .findings
        .into_iter()
        .filter(|f| f.rule == "compatibility.sdk")
        .map(|f| format!("{}: {}", f.shape, f.message))
        .collect()
}

#[test]
fn an_optional_member_after_the_others_is_not_a_source_change() {
    let old = model();
    let new = with(
        OUTPUT,
        &OUTPUT.replace("    }\n}", "        zzz?: string\n    }\n}"),
    );
    assert_eq!(sdk_findings(&old, &new), Vec::<String>::new());

    // Required, it is a new constructor argument in most clients.
    let new = with(
        INPUT,
        &INPUT.replace("    }\n", "        zzz: string\n    }\n"),
    );
    assert!(
        sdk_findings(&old, &new)
            .iter()
            .any(|f| f.starts_with("example.api#GetItemInput$zzz: new public member"))
    );

    // Between existing members, it shifts positional arguments. Written so
    // that insertion order and name order agree.
    let new = with(
        "        name: Name\n        state?: State",
        "        name: Name\n        nox?: string\n        state?: State",
    );
    let findings = sdk_findings(&old, &new);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].starts_with(
            "example.api#GetItemOutput$nox: new optional member before an existing one"
        )
    );

    // A defaulted member is a new constructor argument in most clients.
    let new = with(
        OUTPUT,
        &OUTPUT.replace("    }\n}", "        zzz: string = \"z\"\n    }\n}"),
    );
    assert!(
        sdk_findings(&old, &new)
            .iter()
            .any(|f| f.starts_with("example.api#GetItemOutput$zzz: new public member"))
    );
}

#[test]
fn errors_and_unions_report_every_new_member() {
    let base = |missing: &str, shape: &str| {
        contract(
            &[
                (
                    "    output {\n        name: Name",
                    "    output {\n        shape?: Shape\n        name: Name",
                ),
                (
                    "        node?: Node\n    }\n}",
                    "        node?: Node\n    }\n    errors [Missing]\n}",
                ),
            ],
            &format!(
                "error Missing {{\n    status 404\n    message?: string\n{missing}}}\n\nunion Shape {{\n    circle: integer\n{shape}}}\n"
            ),
        )
    };
    let old = base("", "");

    // C++ and PHP error constructors take their metadata after the members.
    let new = base("    zzz?: string\n", "");
    assert!(
        sdk_findings(&old, &new)
            .iter()
            .any(|f| f.starts_with("example.api#Missing$zzz: new public member"))
    );

    // A union arm breaks exhaustive matches.
    let new = base("", "    zzz: integer\n");
    assert!(
        sdk_findings(&old, &new)
            .iter()
            .any(|f| f.starts_with("example.api#Shape$zzz: new public member"))
    );
}

#[test]
fn findings_are_deterministic_and_exceptions_do_not_hide_them() {
    let old = model();
    let new = with("\"/v1/items\"", "\"/v2/items\"");
    let config = Config::parse("schema_version = 1\n[compatibility]\nsdk_source = false\n[[exceptions]]\nrule = 'compatibility.http'\nshape = 'example.api#GetItem'\nreason = 'Major API version migration'").unwrap();
    let a = diff(&old, &new, &config);
    let b = diff(&old, &new, &config);
    assert!(!a.has_errors());
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap()
    );
    assert!(a.findings[0].exception.is_some());
}

#[test]
fn patterns_match_the_whole_name_including_regex_alternatives() {
    let config =
        Config::parse("schema_version = 1\n[rules.naming]\noperations_pattern = '(Get|GetItem)'")
            .unwrap();
    assert!(check(&model(), &config).findings.is_empty());
    let verbose = Config::parse(
        "schema_version = 1\n[rules.naming]\noperations_pattern = '''(?x)Get.* # verbs\n'''",
    )
    .unwrap();
    assert!(check(&model(), &verbose).findings.is_empty());
}

/// The contract with a `limit` input of type `Limit`, a long ranged to `max`.
fn limited(max: &str) -> Service {
    contract(
        &[(
            INPUT,
            &INPUT.replace("    }\n", "        limit?: Limit\n    }\n"),
        )],
        &format!("long Limit {{ range ..{max} }}\n"),
    )
}

#[test]
fn numeric_bounds_preserve_large_integer_precision_and_allow_input_relaxation() {
    let old = limited("9007199254740993");
    let new = limited("9007199254740992");
    assert!(rules(&old, &new).contains(&"compatibility.constraints".into()));
    let new = limited("9007199254740994");
    assert!(rules(&old, &new).is_empty());
    let new = limited("9007199254740992.0");
    assert!(rules(&old, &new).contains(&"compatibility.constraints".into()));
}

/// The contract with a `limit` input of type `Limit`, a `kind` ranged to
/// `max`.
fn ranged(kind: &str, max: &str) -> Service {
    contract(
        &[(
            INPUT,
            &INPUT.replace("    }\n", "        limit?: Limit\n    }\n"),
        )],
        &format!("{kind} Limit {{ range ..{max} }}\n"),
    )
}

/// A bound changes when the server compares it differently: exactly for a
/// `bigDecimal`, as the integers it admits for an integer kind, in `f64` or
/// `f32` for a `double` or `float`.
#[test]
fn numeric_bounds_compare_as_the_server_does() {
    let changed = |kind: &str, old: &str, new: &str| {
        rules(&ranged(kind, old), &ranged(kind, new)).contains(&"compatibility.constraints".into())
    };
    // Past f64's digits, a bigDecimal bound is tightened...
    assert!(changed("bigDecimal", "0.3", "0.29999999999999999999"));
    assert!(!changed("bigDecimal", "0.29999999999999999999", "0.3"));
    assert!(!changed("bigDecimal", "0.3", "3e-1"));
    // ...and a double's is the same f64, a float's the same f32.
    assert!(!changed("double", "0.3", "0.29999999999999999999"));
    assert!(changed("double", "0.3", "0.2999999999999999"));
    assert!(!changed("float", "0.1", "0.100000001"));
    assert!(changed("float", "0.1", "0.0999999"));
    // An integer kind admits the same integers below 10.5 as below 10.
    assert!(!changed("integer", "10.5", "10"));
    assert!(!changed("integer", "10", "1.0e1"));
    assert!(changed("integer", "10.5", "9.99"));
}

#[test]
fn effective_member_constraints_override_changed_target_constraints() {
    let member_limited = |name_max: &str| {
        contract(
            &[
                ("length 1..100", &format!("length 1..{name_max}")),
                (
                    "        name?: Name\n",
                    "        name?: Name { length 1..20 }\n",
                ),
                (
                    "        name: Name\n",
                    "        name: Name { length 1..20 }\n",
                ),
            ],
            "",
        )
    };
    let old = member_limited("100");
    let new = member_limited("10");
    assert!(rules(&old, &new).is_empty());
}

#[test]
fn effective_member_timestamps_override_changed_target_formats() {
    let timed = |target: &str, member: &str| {
        contract(
            &[(
                INPUT,
                &INPUT.replace("    }\n", &format!("        time?: When{member}\n    }}\n")),
            )],
            &format!("timestamp When {{ format {target} }}\n"),
        )
    };
    let old = timed("date-time", " { format http-date }");
    let new = timed("epoch-seconds", " { format http-date }");
    assert!(rules(&old, &new).is_empty());
    let new = timed("epoch-seconds", "");
    assert!(rules(&old, &new).contains(&"compatibility.timestamp".into()));
}

#[test]
fn each_check_can_carry_its_own_severity() {
    let config = Config::parse(
        r#"
[rules.naming]
severity = "error"
operations = "camelCase"
members = { case = "PascalCase", severity = "warning" }
"#,
    )
    .unwrap();
    let report = check(&model(), &config);
    let operation = report
        .findings
        .iter()
        .find(|f| f.rule == "naming.operations")
        .unwrap();
    assert_eq!(operation.severity, Severity::Error);
    assert_eq!(
        operation.message,
        "operation name `GetItem` is not camelCase"
    );
    let member = report
        .findings
        .iter()
        .find(|f| f.rule == "naming.members")
        .unwrap();
    assert_eq!(member.severity, Severity::Warning);
    assert!(
        member.message.ends_with("is not PascalCase"),
        "{}",
        member.message
    );
}

#[test]
fn a_package_manifest_can_hold_its_policy() {
    let text = "[model]\nnamespace = \"example.api\"\nsources = [\".\"]\n\n[rules.paths]\nprefix = \"/v1\"\n";
    assert!(Config::declares_policy(text));
    assert!(!Config::declares_policy(
        "[model]\nnamespace = \"example.api\"\n"
    ));
    let config = Config::parse(text).unwrap();
    assert_eq!(config.schema_version, 1);
    assert_eq!(config.rules.paths.unwrap().prefix.unwrap().value, "/v1");
}

#[test]
fn problems_point_at_what_is_wrong() {
    let text = "[rules.naming]\noperations = \"Pascal\"\n";
    let error = Config::parse_located(text).unwrap_err();
    let span = error.span.clone().unwrap();
    assert!(text[span].contains("Pascal"), "{error:?}");

    let text = "[rules.paths]\nprefix = { path = \"v1\", severity = \"warning\" }\n";
    let error = Config::parse_located(text).unwrap_err();
    // A check written as a table is located as a whole.
    assert!(text[error.span.clone().unwrap()].starts_with("{ path = \"v1\""));

    // An equal string in a comment or an earlier entry does not take the span.
    let text = "# \"naming.typo\" is not a rule\n[[exceptions]]\nrule = \"naming.members\"\nshape = \"example.api#GetItem\"\nreason = \"naming.typo\"\n[[exceptions]]\nrule = \"naming.typo\"\nshape = \"example.api#GetItem\"\nreason = \"x\"\n";
    let error = Config::parse_located(text).unwrap_err();
    let span = error.span.clone().unwrap();
    assert_eq!(&text[span.clone()], "\"naming.typo\"");
    assert_eq!(span.start, text.rfind("\"naming.typo\"").unwrap());
    assert!(
        error.message.contains("naming.members"),
        "{}",
        error.message
    );
}

/// `tests/models/policy-examples`, and the policy in its manifest.
fn coverage() -> (Service, Config) {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/policy-examples");
    let service = crucible_lang::load_service(&dir).unwrap();
    let config = Config::load(&dir.join("crucible.toml")).unwrap();
    (service, config)
}

fn findings(report: &crucible_policy::Report) -> Vec<(String, String, Severity, bool)> {
    report
        .findings
        .iter()
        .map(|f| {
            (
                f.rule.clone(),
                f.shape.clone(),
                f.severity,
                f.exception.is_some(),
            )
        })
        .collect()
}

#[test]
fn example_coverage_warns_by_default_and_honours_exceptions() {
    let (service, config) = coverage();
    let report = check(&service, &config);
    assert!(!report.has_errors(), "{report}");
    assert_eq!(
        findings(&report),
        [
            // GetNote shows NoteNotFound in an example, not NoteLocked.
            // The service's Throttled is shared by every operation, and
            // not asked of each.
            (
                "examples.errors".to_owned(),
                "example.coverage#GetNote".to_owned(),
                Severity::Warning,
                false
            ),
            (
                "examples.operations".to_owned(),
                "example.coverage#ListNotes".to_owned(),
                Severity::Warning,
                false
            ),
            (
                "examples.operations".to_owned(),
                "example.coverage#Ping".to_owned(),
                Severity::Warning,
                true
            ),
        ]
    );
    let errors: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.rule == "examples.errors")
        .map(|f| f.message.as_str())
        .collect();
    assert_eq!(
        errors,
        ["no `example` shows the error `NoteLocked`: add one with `error NoteLocked { .. }`"]
    );
}

#[test]
fn example_coverage_checks_only_the_selected_namespaces() {
    let (service, _) = coverage();
    let config =
        Config::parse("[rules]\nnamespaces = [\"example.other\"]\n[rules.examples]\n").unwrap();
    assert!(check(&service, &config).findings.is_empty());
}

#[test]
fn example_coverage_checks_take_their_own_severity_or_turn_off() {
    let (service, _) = coverage();
    let config = Config::parse(
        "[rules.examples]\noperations = false\nerrors = { enabled = true, severity = \"error\" }\n",
    )
    .unwrap();
    let report = check(&service, &config);
    assert!(report.has_errors());
    assert_eq!(
        findings(&report),
        [(
            "examples.errors".to_owned(),
            "example.coverage#GetNote".to_owned(),
            Severity::Error,
            false
        )]
    );
    let config = Config::parse("[rules.examples]\nseverity = \"error\"\n").unwrap();
    assert_eq!(check(&service, &config).findings.len(), 3);
    let config = Config::parse("[rules.examples]\nseverity = \"off\"\n").unwrap();
    assert!(check(&service, &config).findings.is_empty());
    // Without the table, nothing is checked.
    assert!(check(&service, &Config::default()).findings.is_empty());
    assert!(Config::parse("[rules.examples]\nshapes = true\n").is_err());
}

/// A released operation that becomes sender-signed (decision 81) leaves
/// every client SDK, with the types only it reached: each is a
/// `compatibility.sdk` finding that says why.
#[test]
fn an_operation_that_becomes_sender_signed_leaves_the_sdks() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/sender-signed");
    let mut package = crucible_lang::load_package(&dir).unwrap();
    let new = {
        let compiled = crucible_lang::compile(&package, None);
        compiled.service.expect("sender-signed compiles")
    };
    for file in &mut package.files {
        file.text = file
            .text
            .replace("\r\n", "\n")
            .replace("@sender(\"Payments Inc.\", signature: signature)\n", "");
    }
    let old = crucible_lang::compile(&package, None)
        .service
        .expect("without @sender");
    let findings = sdk_findings(&old, &new);
    for expected in [
        "example.hooks#RecordPaymentEvent: operation is now sender-signed (`@sender`); client SDKs drop it",
        "example.hooks#RecordPaymentEventInput: type is now reached only by sender-signed operations (`@sender`); client SDKs drop it",
        "example.hooks#PaymentEventBody: type is now reached only by sender-signed operations (`@sender`); client SDKs drop it",
    ] {
        assert!(
            findings.iter().any(|f| f == expected),
            "{expected}: {findings:?}"
        );
    }
    // Already sender-signed, ReportLeakedKeys was in no SDK, and still is not.
    assert!(
        !findings.iter().any(|f| f.contains("LeakedKey")),
        "{findings:?}"
    );
    // Nothing a client calls changed.
    assert!(
        !findings.iter().any(|f| f.contains("GetAccount")),
        "{findings:?}"
    );
    // The wire is unchanged: the server still serves it.
    assert!(rules(&old, &new).is_empty(), "{:?}", rules(&old, &new));
}

const WIRE_ENUMS: &str = "enum Mode {\n    ACTIVE = \"active\"\n    READY\n    Done = \"done_now\"\n}\nenum Level: integer {\n    LOW = 1\n}\n";

fn wire_findings(naming: &str) -> Vec<(String, Severity, String)> {
    let config = Config::parse(&format!("schema_version = 1\n[rules.naming]\n{naming}")).unwrap();
    // An enum is in the service only when something reaches it.
    let reached = [(
        "    next?: Node
",
        "    next?: Node
    mode?: Mode
    level?: Level
",
    )];
    check(&contract(&reached, WIRE_ENUMS), &config)
        .findings
        .into_iter()
        .filter(|f| f.rule == "naming.enum_wire_values")
        .map(|f| (f.shape.to_string(), f.severity, f.message))
        .collect()
}

#[test]
fn enum_wire_values_are_checked_for_explicit_and_bare_values_but_not_integers() {
    let found = wire_findings("enum_wire_values = \"camelCase\"\n");
    let shapes: Vec<_> = found.iter().map(|f| f.0.as_str()).collect();
    assert_eq!(shapes, ["example.api#Mode$Done", "example.api#Mode$READY"]);
    assert_eq!(found[1].2, "enum wire value `READY` is not camelCase");
    assert_eq!(found[0].2, "enum wire value `done_now` is not camelCase");
    assert!(
        wire_findings(
            "enum_wire_values = \"SCREAMING_SNAKE_CASE\"\nenum_values = \"SCREAMING_SNAKE_CASE\"\n"
        )
        .iter()
        .all(|f| !f.0.contains("Level"))
    );
}

#[test]
fn enum_wire_values_pass_when_every_value_matches() {
    assert!(wire_findings("enum_wire_values_pattern = \"[a-z][A-Za-z_]*|READY\"\n").is_empty());
}

#[test]
fn enum_wire_value_patterns_and_severity_apply() {
    let found = wire_findings(
        "severity = \"error\"\nenum_wire_values_pattern = { pattern = \"[a-z]+\", severity = \"warning\" }\n",
    );
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.iter().all(|f| f.1 == Severity::Warning));
    assert!(found[0].2.contains("does not match the pattern `[a-z]+`"));
    let found = wire_findings(
        "severity = \"error\"\nenum_wire_values = { case = \"camelCase\", severity = \"warning\" }\n",
    );
    assert!(found.iter().all(|f| f.1 == Severity::Warning));
    let found = wire_findings("severity = \"error\"\nenum_wire_values = \"camelCase\"\n");
    assert!(found.iter().all(|f| f.1 == Severity::Error));
}
