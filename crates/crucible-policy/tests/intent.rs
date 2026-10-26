use std::path::Path;

use crucible_policy::{Config, Severity, check};

fn service() -> crucible_ir::ir::Service {
    crucible_lang::load_service(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/operation-intent"),
    )
    .unwrap()
}

#[test]
fn intent_is_opt_in_warns_for_missing_or_incomplete_guidance_and_respects_namespaces() {
    let service = service();
    assert!(check(&service, &Config::default()).findings.is_empty());
    let config = Config::parse("[rules.intent]").unwrap();
    let report = check(&service, &config);
    assert!(!report.has_errors());
    let findings: Vec<_> = report
        .findings
        .iter()
        .map(|f| (f.rule.as_str(), f.shape.as_str(), f.severity))
        .collect();
    assert_eq!(
        findings,
        [
            (
                "intent.operations",
                "example.intent#Fenced",
                Severity::Warning
            ),
            (
                "intent.operations",
                "example.intent#Missing",
                Severity::Warning
            ),
            (
                "intent.operations",
                "example.intent#Partial",
                Severity::Warning
            ),
        ]
    );
    let config = Config::parse("[rules]\nnamespaces = ['other.service']\n[rules.intent]").unwrap();
    assert!(check(&service, &config).findings.is_empty());
}

#[test]
fn intent_severity_and_exceptions_use_the_existing_policy_path() {
    let service = service();
    let config = Config::parse("[rules.intent]\nseverity = 'error'").unwrap();
    assert!(check(&service, &config).has_errors());
    let config = Config::parse("[rules.intent]\nseverity = 'off'").unwrap();
    assert!(check(&service, &config).findings.is_empty());
    let config = Config::parse("[rules.intent]\nseverity = 'error'\n[[exceptions]]\nrule = 'intent.operations'\nshape = 'example.intent#Missing'\nreason = 'A draft operation'").unwrap();
    let report = check(&service, &config);
    assert_eq!(
        report
            .findings
            .iter()
            .find(|f| f.shape == "example.intent#Missing")
            .unwrap()
            .exception
            .as_deref(),
        Some("A draft operation")
    );
    for text in [
        "[rules.intent]\nseverity = 'loud'",
        "[rules.intent]\nunknown = true",
    ] {
        assert!(Config::parse(text).is_err());
    }
    let config = Config::parse("[rules.documentation]\ntargets = ['operations']").unwrap();
    let report = check(&service, &config);
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].rule, "documentation.required");
    assert_eq!(report.findings[0].severity, Severity::Error);
}
