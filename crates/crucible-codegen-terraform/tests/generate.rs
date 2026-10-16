use crucible_codegen_infra::{Bindings, resolve};
use crucible_codegen_terraform::{Options, emit};

fn fixture() -> (crucible_ir::ir::Service, Bindings) {
    (
        crucible_lang::load_service(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/models/terraform-inventory"),
        )
        .unwrap_or_else(|rendered| panic!("terraform-inventory:\n{rendered}")),
        serde_json::from_str(include_str!("fixtures/bindings.json")).unwrap(),
    )
}
fn options() -> Options {
    Options {
        module_path: "example.com/terraform-provider-inventory".into(),
        provider_name: "inventory".into(),
        provider_address: "registry.terraform.io/example/inventory".into(),
    }
}

#[test]
fn emits_provider_using_go_wire_runtime() {
    let (service, bindings) = fixture();
    let plan = resolve(&service, &bindings).unwrap();
    let files = emit(&service, &plan, &options()).unwrap();
    assert!(
        files
            .iter()
            .any(|f| f.path == "main.go" && f.contents.contains("ProtocolVersion: 6"))
    );
    for file in files.iter().filter(|f| {
        ["main.go", "go.mod", "internal/provider/terraform.go"].contains(&f.path.as_str())
    }) {
        let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(&file.path);
        if std::env::var_os("CRUCIBLE_BLESS").is_some() {
            std::fs::create_dir_all(golden.parent().unwrap()).unwrap();
            std::fs::write(&golden, &file.contents).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(&golden)
                .unwrap()
                .replace("\r\n", "\n"),
            file.contents.replace("\r\n", "\n"),
            "golden {}",
            file.path
        );
    }
    assert!(files.iter().any(
        |f| f.path == "internal/provider/terraform.go" && f.contents.contains("MaxAttempts: 1")
    ));
    assert!(
        files
            .iter()
            .any(|f| f.path == "internal/provider/runtime.go"
                && f.contents.contains("func (c *clientRuntime) callValues"))
    );
    if let Ok(out) = std::env::var("CRUCIBLE_TERRAFORM_OUT") {
        for file in files {
            let path = std::path::Path::new(&out).join(file.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, file.contents).unwrap();
        }
    }
}

#[test]
fn rejects_unrelated_sources_and_sensitive_identifiers() {
    let (mut service, bindings) = fixture();
    let mut plan = resolve(&service, &bindings).unwrap();
    plan.resources[0]
        .properties
        .get_mut("name")
        .unwrap()
        .source
        .as_mut()
        .unwrap()
        .shape = "example.infra#UpdateWidgetInput".parse().unwrap();
    let target = "example.infra#Id".parse().unwrap();
    let crucible_ir::ir::Shape::Simple(id) = service.shapes.get_mut(&target).unwrap() else {
        panic!()
    };
    id.sensitive = true;
    let errors = emit(&service, &plan, &options()).err().unwrap().to_string();
    assert!(errors.contains("canonical create input"));
    assert!(errors.contains("identifiers cannot be sensitive"));
}

#[test]
fn diagnoses_lifecycle_constraint_disagreement() {
    let (mut service, bindings) = fixture();
    let plan = resolve(&service, &bindings).unwrap();
    let input = "example.infra#UpdateWidgetInput".parse().unwrap();
    let crucible_ir::ir::Shape::Structure(input) = service.shapes.get_mut(&input).unwrap() else {
        panic!()
    };
    input
        .members
        .iter_mut()
        .find(|m| m.name == "name")
        .unwrap()
        .constraints
        .length
        .as_mut()
        .unwrap()
        .max = Some(10);
    let errors = emit(&service, &plan, &options()).err().unwrap().to_string();
    assert!(errors.contains("inconsistent type, constraints, or sensitivity"));
}

#[test]
fn rejects_reserved_property_names_and_integer_refinement() {
    let (service, bindings) = fixture();
    let mut plan = resolve(&service, &bindings).unwrap();
    let mut property = plan.resources[0].properties["name"].clone();
    property.integer = true;
    plan.resources[0]
        .properties
        .insert("count".into(), property);
    let errors = emit(&service, &plan, &options()).err().unwrap().to_string();
    assert!(errors.contains("non-reserved lowercase"));
    assert!(errors.contains("integer refinement"));
}

#[test]
fn rejects_unsafe_or_unsupported_lifecycle_policy() {
    let (service, bindings) = fixture();
    let mut plan = resolve(&service, &bindings).unwrap();
    let resource = &mut plan.resources[0];
    resource.terraform.as_mut().unwrap()["synchronous"] = false.into();
    resource.properties.get_mut("name").unwrap().required = false;
    let errors = emit(&service, &plan, &options()).err().unwrap().to_string();
    assert!(errors.contains("synchronous mutations"));
    assert!(errors.contains("null/reset semantics"));
}

#[test]
fn rejects_unmapped_required_inputs_and_unknown_error_names() {
    let (mut service, bindings) = fixture();
    let plan = resolve(&service, &bindings).unwrap();
    let input: crucible_ir::ShapeId = "example.infra#ReadWidgetInput".parse().unwrap();
    let crucible_ir::ir::Shape::Structure(input) = service.shapes.get_mut(&input).unwrap() else {
        panic!()
    };
    let mut extra = input.members[0].clone();
    extra.name = "owner".into();
    input.members.push(extra);
    let mut plan = plan;
    plan.resources[0].terraform.as_mut().unwrap()["not_found_errors"] =
        serde_json::json!(["AccessDenied"]);
    let errors = emit(&service, &plan, &options()).err().unwrap().to_string();
    assert!(errors.contains("unmapped required input `owner`"));
    assert!(errors.contains("must be modelled"));
}
