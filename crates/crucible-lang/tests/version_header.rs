use crucible_ir::{ShapeId, ir::UnversionedPolicy, snapshot};
use crucible_lang::{compile, load_package};
use std::path::Path;

fn compiled(name: &str) -> crucible_lang::Compiled {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(format!("version-header-{name}"));
    compile(&load_package(&path).unwrap(), None)
}
fn load(name: &str) -> crucible_ir::ir::Service {
    let c = compiled(name);
    let rendered = c.render();
    c.service.unwrap_or_else(|| panic!("{rendered}"))
}
#[test]
fn named_header_and_required_policy_are_lowered() {
    let s = load("named");
    assert_eq!(s.version_header.as_deref(), Some("Example-Version"));
    assert_eq!(s.version_header(), "Example-Version");
    assert_eq!(s.unversioned, UnversionedPolicy::Required);
}
#[test]
fn absent_clauses_keep_the_default_contract() {
    let s = load("default");
    assert_eq!(s.version_header, None);
    assert_eq!(s.version_header(), "Crucible-Version");
    assert_eq!(s.unversioned, UnversionedPolicy::Oldest);
}
fn refused(name: &str, code: &str, expected: &str) {
    let c = compiled(name);
    assert!(c.service.is_none(), "{}", c.render());
    let d = c
        .diagnostics
        .iter()
        .find(|d| d.diagnostic.message.starts_with(code))
        .unwrap_or_else(|| panic!("{}", c.render()));
    let source = &c.files[d.file.unwrap()].text;
    let start = source.find(expected).unwrap();
    assert_eq!(
        d.diagnostic.span,
        crucible_lang::Span::new(start, start + expected.len())
    );
}

#[test]
fn invalid_token_is_refused() {
    refused(
        "invalid-token",
        "versionHeader.token:",
        "service Example {\n    version \"v1\"\n    protocol json\n    versionHeader \"Not A Token\"\n    operations [Ping]\n}",
    );
}

#[test]
fn invalid_length_is_refused() {
    refused(
        "invalid-length",
        "versionHeader.length:",
        "service Example {\n    version \"v1\"\n    protocol json\n    versionHeader \"XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX\"\n    operations [Ping]\n}",
    );
}

#[test]
fn invalid_reserved_is_refused() {
    refused(
        "invalid-reserved",
        "versionHeader.reserved:",
        "service Example {\n    version \"v1\"\n    protocol json\n    versionHeader \"CoNnEcTiOn\"\n    operations [Ping]\n}",
    );
}

#[test]
fn invalid_runtime_is_refused() {
    refused(
        "invalid-runtime",
        "versionHeader.runtime:",
        "service Example {\n    version \"v1\"\n    protocol json\n    versionHeader \"VaRy\"\n    operations [Ping]\n}",
    );
}

#[test]
fn invalid_browser_is_refused() {
    refused(
        "invalid-browser",
        "versionHeader.browser:",
        "service Example {\n    version \"v1\"\n    protocol json\n    versionHeader \"Cookie\"\n    operations [Ping]\n}",
    );
}

#[test]
fn invalid_sec_is_refused() {
    refused(
        "invalid-sec",
        "versionHeader.browser:",
        "service Example {\n    version \"v1\"\n    protocol json\n    versionHeader \"Sec-Release\"\n    operations [Ping]\n}",
    );
}

#[test]
fn invalid_proxy_is_refused() {
    refused(
        "invalid-proxy",
        "versionHeader.browser:",
        "service Example {\n    version \"v1\"\n    protocol json\n    versionHeader \"Proxy-Release\"\n    operations [Ping]\n}",
    );
}

#[test]
fn invalid_access_control_is_refused() {
    refused(
        "invalid-access-control",
        "versionHeader.browser:",
        "service Example {\n    version \"v1\"\n    protocol json\n    versionHeader \"Access-Control-Release\"\n    operations [Ping]\n}",
    );
}

#[test]
fn invalid_binding_is_refused() {
    refused(
        "invalid-binding",
        "versionHeader.binding:",
        "@header(\"example-version\")\n        release?: string",
    );
}

#[test]
fn duplicate_header_is_refused() {
    refused(
        "duplicate-header",
        "service.duplicate:",
        "versionHeader \"Other-Version\"",
    );
}

#[test]
fn duplicate_policy_is_refused() {
    refused(
        "duplicate-policy",
        "service.duplicate:",
        "unversioned required",
    );
}

#[test]
fn unknown_policy_is_refused() {
    refused("unknown-policy", "service.unversioned:", "latest");
}

#[test]
fn formatting_preserves_the_two_clauses() {
    let source = "service Example {\nversionHeader \"Example-Version\"\nunversioned required\n}\n";
    let formatted = crucible_lang::format(source, &Default::default());
    assert_eq!(
        formatted,
        "service Example {\n    versionHeader \"Example-Version\"\n    unversioned required\n}\n"
    );
    assert_eq!(
        crucible_lang::format(&formatted, &Default::default()),
        formatted
    );
}
#[test]
fn snapshots_record_explicit_settings() {
    let service = load("named");
    let restored = snapshot::read(&snapshot::write(&service, "test")).unwrap();
    assert_eq!(restored.version_header, service.version_header);
    assert_eq!(restored.unversioned, service.unversioned);
}
#[test]
fn existing_snapshot_reserializes_byte_identically() {
    let text = include_str!("../../../tests/models/version-projection-conditions/releases/v1.json");
    let value: serde_json::Value = serde_json::from_str(text).unwrap();
    let service = snapshot::read(text).unwrap();
    assert_eq!(
        snapshot::write(&service, value["compiler"].as_str().unwrap()),
        text
    );
}
#[test]
fn historical_projection_uses_the_working_settings() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models/version-projection-conditions");
    let mut service = crucible_lang::load_versioned_service(&path).unwrap();
    service.version_header = Some("Working-Version".into());
    service.unversioned = UnversionedPolicy::Required;
    let projections = crucible_ir::versions::projections(&service).unwrap();
    assert!(!projections.is_empty());
    for projected in projections {
        assert_eq!(projected.service.version_header(), "Working-Version");
        assert_eq!(projected.service.unversioned, UnversionedPolicy::Required);
    }
}
fn compare(old: &str, new: &str) -> crucible_policy::Report {
    let old = load(old);
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(format!(
            "version-header-{}/baselines/v1.json",
            if old.unversioned == UnversionedPolicy::Required {
                "named"
            } else {
                "oldest"
            }
        ));
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let temporary = path.with_extension(format!("{new}.tmp"));
        std::fs::write(&temporary, snapshot::write(&old, "test")).unwrap();
        std::fs::rename(temporary, &path).unwrap();
    }
    let old = snapshot::read(&std::fs::read_to_string(path).unwrap()).unwrap();
    crucible_policy::diff(&old, &load(new), &Default::default())
}
#[test]
fn changing_the_header_is_breaking() {
    let report = compare("named", "changed");
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].rule, "compatibility.protocol");
    assert_eq!(
        report.findings[0].shape,
        ShapeId::new("example.version_header", "Example").to_string()
    );
}
#[test]
fn changing_only_header_case_is_compatible() {
    assert!(compare("named", "case").findings.is_empty());
}
#[test]
fn requiring_a_previously_optional_release_is_breaking() {
    let report = compare("oldest", "named");
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].rule, "compatibility.protocol");
}
#[test]
fn allowing_an_unversioned_request_is_compatible() {
    assert!(compare("named", "oldest").findings.is_empty());
}
#[test]
fn hand_built_ir_validates_the_header() {
    let mut service = load("default");
    service.version_header = Some("Vary".into());
    let mut diagnostics = crucible_ir::Diagnostics::default();
    crucible_ir::validate::service(&service, &mut diagnostics);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.shape == service.id && d.message.starts_with("versionHeader.runtime:"))
    );
}

#[test]
fn a_sender_signature_cannot_carry_the_release() {
    refused(
        "invalid-signature",
        "versionHeader.binding:",
        "@header(\"example-signature\")\n        signature: string",
    );
}
#[test]
fn the_old_header_is_not_reserved_as_an_alias() {
    let mut package = load_package(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/models/version-header-invalid-binding"),
    )
    .unwrap();
    package.files[0].text = package.files[0].text.replace(
        "@header(\"example-version\")",
        "@header(\"Crucible-Version\")",
    );
    let c = compile(&package, None);
    assert!(c.service.is_some(), "{}", c.render());
}

#[test]
fn default_header_case_does_not_require_a_target_capability() {
    let mut service = load("default");
    service.version_header = Some("cRuCiBlE-vErSiOn".into());
    service.require_capabilities(&Default::default()).unwrap();
}
#[test]
fn both_unsupported_clauses_are_reported() {
    let service = load("named");
    let diagnostics = service
        .require_capabilities(&Default::default())
        .unwrap_err();
    assert_eq!(diagnostics.iter().count(), 2);
    assert!(diagnostics.iter().all(|d| d.shape == service.id));
}

#[test]
fn an_invalid_release_diagnostic_names_the_configured_header() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/version-header-named");
    let mut package = load_package(&path).unwrap();
    package.files[0].text = package.files[0]
        .text
        .replace("version \"v1\"", "version \"release one\"");
    let compiled = compile(&package, None);
    assert!(compiled.service.is_none());
    let diagnostic = compiled
        .diagnostics
        .iter()
        .find(|d| d.diagnostic.message.contains("visible ASCII"))
        .unwrap();
    assert!(diagnostic.diagnostic.message.contains("`Example-Version`"));
    assert_eq!(
        diagnostic.diagnostic.span.text(&compiled.files[0].text),
        "service Example {\n    version \"release one\"\n    protocol json\n    versionHeader \"Example-Version\"\n    unversioned required\n    operations [Ping]\n}"
    );
}

#[test]
fn a_64_byte_header_is_valid() {
    let mut service = load("default");
    service.version_header = Some("X".repeat(64));
    let mut diagnostics = crucible_ir::Diagnostics::default();
    crucible_ir::validate::service(&service, &mut diagnostics);
    assert!(diagnostics.is_empty(), "{diagnostics}");
}

#[test]
fn accept_encoding_cannot_carry_a_release() {
    refused(
        "invalid-accept-encoding",
        "versionHeader.browser:",
        "service Example {\n    version \"v1\"\n    protocol json\n    versionHeader \"Accept-Encoding\"\n    operations [Ping]\n}",
    );
}

#[test]
fn accept_charset_cannot_carry_a_release() {
    refused(
        "invalid-accept-charset",
        "versionHeader.browser:",
        "service Example {\n    version \"v1\"\n    protocol json\n    versionHeader \"Accept-Charset\"\n    operations [Ping]\n}",
    );
}

#[test]
fn dnt_cannot_carry_a_release() {
    refused(
        "invalid-dnt",
        "versionHeader.browser:",
        "service Example {\n    version \"v1\"\n    protocol json\n    versionHeader \"DNT\"\n    operations [Ping]\n}",
    );
}

#[test]
fn cookie2_cannot_carry_a_release() {
    refused(
        "invalid-cookie2",
        "versionHeader.browser:",
        "service Example {\n    version \"v1\"\n    protocol json\n    versionHeader \"Cookie2\"\n    operations [Ping]\n}",
    );
}

#[test]
fn a_custom_release_header_is_not_browser_forbidden() {
    let mut service = load("default");
    service.version_header = Some("Example-Version".into());
    let mut diagnostics = crucible_ir::Diagnostics::default();
    crucible_ir::validate::service(&service, &mut diagnostics);
    assert!(diagnostics.is_empty(), "{diagnostics}");
}

#[test]
fn http_owned_release_headers_keep_their_classification() {
    let mut service = load("default");
    service.version_header = Some("Connection".into());
    let mut diagnostics = crucible_ir::Diagnostics::default();
    crucible_ir::validate::service(&service, &mut diagnostics);
    assert_eq!(diagnostics.iter().count(), 1);
    assert!(
        diagnostics
            .iter()
            .next()
            .unwrap()
            .message
            .starts_with("versionHeader.reserved:")
    );
}
