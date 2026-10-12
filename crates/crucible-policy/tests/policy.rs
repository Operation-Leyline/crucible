use crucible_policy::{Config, Severity, check, diff};
use crucible_smithy::ir::Service;
use crucible_smithy::{Capabilities, Feature, Model};
use serde_json::{Value, json};

fn model() -> Value {
    serde_json::from_str(include_str!("fixtures/contract.json")).unwrap()
}

fn lower(model: &Value) -> Service {
    Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &Capabilities::new(Feature::ALL.iter().copied()))
        .unwrap()
}

fn wire() -> Config {
    Config::parse("schema_version = 1\n[compatibility]\nsdk_source = false\n").unwrap()
}

fn rules(old: &Value, new: &Value) -> Vec<String> {
    diff(&lower(old), &lower(new), &wire())
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
    let report = check(&lower(&model()), &config);
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
    let mut value = model();
    value["shapes"]["example.api#GetItem"]["traits"]["smithy.api#http"]["uri"] =
        json!("/v10/items");
    let config = Config::parse("schema_version = 1\n[rules.paths]\nprefix = '/v1'").unwrap();
    assert!(check(&lower(&value), &config).has_errors());
    let config =
        Config::parse("schema_version = 1\n[rules.paths]\nprefix = '/v1'\nseverity = 'off'")
            .unwrap();
    assert!(check(&lower(&value), &config).findings.is_empty());
}

#[test]
fn no_op_and_documentation_changes_pass_with_recursive_shapes() {
    let old = model();
    let mut new = old.clone();
    new["shapes"]["example.api#GetItem"]["traits"]["smithy.api#documentation"] =
        json!("Fetch an item.");
    assert!(
        diff(&lower(&old), &lower(&new), &Config::default())
            .findings
            .is_empty()
    );
}

#[test]
fn removing_readonly_is_a_breaking_retry_contract_change() {
    let mut old = model();
    old["shapes"]["example.api#GetItem"]["traits"]["smithy.api#readonly"] = json!({});
    let mut new = old.clone();
    new["shapes"]["example.api#GetItem"]["traits"]
        .as_object_mut()
        .unwrap()
        .remove("smithy.api#readonly");
    let findings = diff(&lower(&old), &lower(&new), &Config::default()).findings;
    assert!(findings.iter().any(|finding| {
        finding.rule == "compatibility.operation"
            && finding.shape == "example.api#GetItem"
            && finding.message.contains("readonly")
    }));
}

#[test]
fn optional_inputs_pass_but_new_required_inputs_and_lost_output_guarantees_fail() {
    let old = model();
    let mut new = old.clone();
    new["shapes"]["example.api#GetItemInput"]["members"]["extra"] =
        json!({"target":"smithy.api#String"});
    assert!(rules(&old, &new).is_empty());
    new["shapes"]["example.api#GetItemInput"]["members"]["extra"]["traits"] =
        json!({"smithy.api#required":{}});
    assert!(rules(&old, &new).contains(&"compatibility.required".into()));
    let mut new = old.clone();
    new["shapes"]["example.api#GetItemOutput"]["members"]["name"]
        .as_object_mut()
        .unwrap()
        .remove("traits");
    assert!(rules(&old, &new).contains(&"compatibility.nullability".into()));
}

#[test]
fn defaulted_new_required_inputs_do_not_require_old_callers_to_send_them() {
    let old = model();
    let mut new = old.clone();
    new["shapes"]["example.api#GetItemInput"]["members"]["extra"] = json!({"target":"smithy.api#String","traits":{"smithy.api#required":{},"smithy.api#default":"hello"}});
    assert!(!rules(&old, &new).contains(&"compatibility.required".into()));
}

#[test]
fn enum_direction_is_checked_independently_for_shared_input_and_output_shapes() {
    let old = model();
    let mut new = old.clone();
    new["shapes"]["example.api#State"]["members"]["WAITING"] =
        json!({"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":"waiting"}});
    assert!(rules(&old, &new).contains(&"compatibility.enum".into()));
    let mut old_input = old.clone();
    old_input["shapes"]["example.api#GetItemOutput"]["members"]
        .as_object_mut()
        .unwrap()
        .remove("state");
    let mut new_input = new;
    new_input["shapes"]["example.api#GetItemOutput"]["members"]
        .as_object_mut()
        .unwrap()
        .remove("state");
    assert!(rules(&old_input, &new_input).is_empty());
    assert!(rules(&new_input, &old_input).contains(&"compatibility.enum".into()));
}

#[test]
fn tightening_input_constraints_and_loosening_output_constraints_fail() {
    let old = model();
    let mut new = old.clone();
    new["shapes"]["example.api#Name"]["traits"]["smithy.api#length"]["max"] = json!(50);
    assert!(rules(&old, &new).contains(&"compatibility.constraints".into()));
    new["shapes"]["example.api#Name"]["traits"]["smithy.api#length"]["max"] = json!(150);
    assert!(rules(&old, &new).contains(&"compatibility.constraints".into()));
}

#[test]
fn removed_operations_and_rebound_members_are_reported() {
    let old = model();
    let mut new = old.clone();
    new["shapes"]["example.api#Catalog"]["operations"] = json!([]);
    assert!(rules(&old, &new).contains(&"compatibility.operation".into()));
    let mut new = old.clone();
    new["shapes"]["example.api#GetItemInput"]["members"]["name"]["traits"] =
        json!({"smithy.api#httpHeader":"X-Name"});
    assert!(rules(&old, &new).contains(&"compatibility.member".into()));
}

#[test]
fn source_symbol_changes_are_separate_from_wire_changes() {
    let old = model();
    let mut new = old.clone();
    let name = new["shapes"]["example.api#GetItemInput"]["members"]
        .as_object_mut()
        .unwrap()
        .remove("name")
        .unwrap();
    new["shapes"]["example.api#GetItemInput"]["members"]["label"] = name;
    new["shapes"]["example.api#GetItemInput"]["members"]["label"]["traits"] =
        json!({"smithy.api#jsonName":"name"});
    assert!(rules(&old, &new).is_empty());
    assert!(
        diff(&lower(&old), &lower(&new), &Config::default())
            .findings
            .iter()
            .any(|f| f.rule == "compatibility.sdk")
    );
}

/// The `compatibility.sdk` findings, as `shape: message`.
fn sdk_findings(old: &Value, new: &Value) -> Vec<String> {
    diff(&lower(old), &lower(new), &Config::default())
        .findings
        .into_iter()
        .filter(|f| f.rule == "compatibility.sdk")
        .map(|f| format!("{}: {}", f.shape, f.message))
        .collect()
}

#[test]
fn an_optional_member_after_the_others_is_not_a_source_change() {
    let old = model();
    let output = "example.api#GetItemOutput";
    let mut new = old.clone();
    new["shapes"][output]["members"]["zzz"] = json!({"target":"smithy.api#String"});
    assert_eq!(sdk_findings(&old, &new), Vec::<String>::new());

    // Required, it is a new constructor argument in most clients.
    let mut new = old.clone();
    new["shapes"]["example.api#GetItemInput"]["members"]["zzz"] =
        json!({"target":"smithy.api#String","traits":{"smithy.api#required":{}}});
    assert!(
        sdk_findings(&old, &new)
            .iter()
            .any(|f| f.starts_with("example.api#GetItemInput$zzz: new public member"))
    );

    // Between existing members, it shifts positional arguments. Written so
    // that insertion order and name order agree, whichever serde_json keeps.
    let members = &old["shapes"][output]["members"];
    let mut new = old.clone();
    new["shapes"][output]["members"] = json!({
        "name": members["name"],
        "nox": {"target":"smithy.api#String"},
        "state": members["state"],
        "node": members["node"],
    });
    let findings = sdk_findings(&old, &new);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].starts_with(
            "example.api#GetItemOutput$nox: new optional member before an existing one"
        )
    );
}

#[test]
fn unsupported_wire_families_fail_closed() {
    let mut value = model();
    value["shapes"]["example.api#Catalog"]["traits"] = json!({"aws.protocols#restXml":{}});
    assert!(rules(&value, &value).contains(&"compatibility.unchecked".into()));
}

#[test]
fn findings_are_deterministic_and_exceptions_do_not_hide_them() {
    let old = model();
    let mut new = old.clone();
    new["shapes"]["example.api#GetItem"]["traits"]["smithy.api#http"]["uri"] = json!("/v2/items");
    let config = Config::parse("schema_version = 1\n[compatibility]\nsdk_source = false\n[[exceptions]]\nrule = 'compatibility.http'\nshape = 'example.api#GetItem'\nreason = 'Major API version migration'").unwrap();
    let a = diff(&lower(&old), &lower(&new), &config);
    let b = diff(&lower(&old), &lower(&new), &config);
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
    assert!(check(&lower(&model()), &config).findings.is_empty());
    let verbose = Config::parse(
        "schema_version = 1\n[rules.naming]\noperations_pattern = '''(?x)Get.* # verbs\n'''",
    )
    .unwrap();
    assert!(check(&lower(&model()), &verbose).findings.is_empty());
}

#[test]
fn numeric_bounds_preserve_large_integer_precision_and_allow_input_relaxation() {
    let mut old = model();
    old["shapes"]["example.api#Limit"] =
        json!({"type":"long","traits":{"smithy.api#range":{"max":9007199254740993_u64}}});
    old["shapes"]["example.api#GetItemInput"]["members"]["limit"] =
        json!({"target":"example.api#Limit"});
    let mut new = old.clone();
    new["shapes"]["example.api#Limit"]["traits"]["smithy.api#range"]["max"] =
        json!(9007199254740992_u64);
    assert!(rules(&old, &new).contains(&"compatibility.constraints".into()));
    new["shapes"]["example.api#Limit"]["traits"]["smithy.api#range"]["max"] =
        json!(9007199254740994_u64);
    assert!(rules(&old, &new).is_empty());
    new["shapes"]["example.api#Limit"]["traits"]["smithy.api#range"]["max"] =
        json!(9007199254740992.0_f64);
    assert!(rules(&old, &new).contains(&"compatibility.constraints".into()));
}

#[test]
fn root_header_mapping_does_not_hide_nested_json_names_on_the_same_shape() {
    let mut old = model();
    old["shapes"]["example.api#GetItemInput"]["members"]["name"]["traits"] =
        json!({"smithy.api#httpHeader":"X-Name"});
    old["shapes"]["example.api#GetItemInput"]["members"]["node"] =
        json!({"target":"example.api#GetItemInput"});
    let mut new = old.clone();
    new["shapes"]["example.api#GetItemInput"]["members"]["name"]["traits"]["smithy.api#jsonName"] =
        json!("label");
    assert!(rules(&old, &new).contains(&"compatibility.member".into()));
}

#[test]
fn effective_member_constraints_override_changed_target_constraints() {
    let mut old = model();
    for id in ["example.api#GetItemInput", "example.api#GetItemOutput"] {
        old["shapes"][id]["members"]["name"]["traits"]["smithy.api#length"] =
            json!({"min":1,"max":20});
    }
    let mut new = old.clone();
    new["shapes"]["example.api#Name"]["traits"]["smithy.api#length"]["max"] = json!(10);
    assert!(rules(&old, &new).is_empty());
}

#[test]
fn effective_member_timestamps_override_changed_target_formats() {
    let mut old = model();
    old["shapes"]["example.api#When"] =
        json!({"type":"timestamp","traits":{"smithy.api#timestampFormat":"date-time"}});
    old["shapes"]["example.api#GetItemInput"]["members"]["time"] =
        json!({"target":"example.api#When","traits":{"smithy.api#timestampFormat":"http-date"}});
    let mut new = old.clone();
    new["shapes"]["example.api#When"]["traits"]["smithy.api#timestampFormat"] =
        json!("epoch-seconds");
    assert!(rules(&old, &new).is_empty());
    new["shapes"]["example.api#GetItemInput"]["members"]["time"]["traits"]
        .as_object_mut()
        .unwrap()
        .remove("smithy.api#timestampFormat");
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
    let report = check(&lower(&model()), &config);
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
