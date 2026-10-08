use std::{fs, path::Path};

use crucible_codegen_cdk::{Options, emit};
use crucible_codegen_infra::{Bindings, resolve};
use crucible_smithy::Model;

fn inputs() -> (crucible_smithy::ir::Service, Bindings, Options) {
    (
        Model::from_json(include_str!("fixtures/model.json"))
            .unwrap()
            .lower_infrastructure(None)
            .unwrap(),
        serde_json::from_str(include_str!("fixtures/bindings.json")).unwrap(),
        serde_json::from_str(include_str!("fixtures/options.json")).unwrap(),
    )
}

#[test]
fn constructs_match_goldens() {
    let (service, bindings, options) = inputs();
    let plan = resolve(&service, &bindings).unwrap();
    let files = emit(&service, &plan, &options).unwrap();
    assert_eq!(files, emit(&service, &plan, &options).unwrap());
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/weather");
    for file in files {
        let path = root.join(file.path);
        if std::env::var_os("CRUCIBLE_BLESS").is_some() {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &file.contents).unwrap();
        }
        assert_eq!(
            fs::read_to_string(&path).unwrap().replace("\r\n", "\n"),
            file.contents,
            "{}",
            path.display()
        );
    }
}

#[test]
fn unsupported_targets_and_symbol_collisions_are_diagnostics() {
    let (mut service, bindings, options) = inputs();
    let mut plan = resolve(&service, &bindings).unwrap();
    plan.resources[0].properties.get_mut("Name").unwrap().target =
        "smithy.api#Document".parse().unwrap();
    service.shapes.insert(
        "smithy.api#Document".parse().unwrap(),
        crucible_smithy::ir::Shape::Unit,
    );
    plan.resources[0].cdk.as_mut().unwrap().class_name = Some("CfnResource".into());
    let errors = emit(&service, &plan, &options).unwrap_err().to_string();
    assert!(
        errors.contains("unsupported CDK property target"),
        "{errors}"
    );
    assert!(errors.contains("not a supported CDK type name"), "{errors}");
}

#[test]
fn absent_prelude_context_shapes_are_supported() {
    let (mut service, bindings, options) = inputs();
    service.shapes.remove(&"smithy.api#String".parse().unwrap());
    let plan = resolve(&service, &bindings).unwrap();
    let files = emit(&service, &plan, &options).unwrap();
    assert!(files[0].contents.contains("readonly secretArn: string"));
}

#[test]
fn nested_shapes_use_jsii_member_spelling_and_map_to_wire_names() {
    let (mut service, bindings, options) = inputs();
    if let Some(crucible_smithy::ir::Shape::Structure(s)) =
        service.shapes.get_mut(&"example#Settings".parse().unwrap())
    {
        s.members[0].name = "Max_Count".into();
    }
    let plan = resolve(&service, &bindings).unwrap();
    let files = emit(&service, &plan, &options).unwrap();
    let source = &files[0].contents;
    assert!(source.contains("readonly maxCount:"), "{source}");
    assert!(!source.contains("Max_Count:"), "{source}");
    // The schema carries the wire spelling so synthesis can restore it.
    assert!(
        source.contains("\"maxCount\":{\"wire\":\"Max_Count\""),
        "{source}"
    );
}

#[test]
fn snake_case_resource_properties_and_attributes_are_jsii_camel_case() {
    let (service, bindings, options) = inputs();
    let mut plan = resolve(&service, &bindings).unwrap();
    let properties = &mut plan.resources[0].properties;
    let property = properties.remove("Name").unwrap();
    properties.insert("max_count".into(), property);
    let attribute = plan.resources[0].attributes.remove("Id").unwrap();
    plan.resources[0]
        .attributes
        .insert("city_id".into(), attribute);
    let files = emit(&service, &plan, &options).unwrap();
    let source = &files[0].contents;
    assert!(source.contains("readonly maxCount: string;"), "{source}");
    assert!(
        source.contains("\"max_count\": toWire(props.maxCount,"),
        "{source}"
    );
    assert!(
        source.contains("public readonly attrCityId: string;"),
        "{source}"
    );
    assert!(source.contains("getAttString(\"city_id\")"), "{source}");
}

#[test]
fn colliding_camel_case_members_are_diagnosed() {
    let (service, bindings, options) = inputs();
    let mut plan = resolve(&service, &bindings).unwrap();
    let properties = &mut plan.resources[0].properties;
    let property = properties.get("Name").unwrap().clone();
    properties.insert("name_".into(), property);
    assert!(
        emit(&service, &plan, &options)
            .unwrap_err()
            .to_string()
            .contains("collides")
    );
}

#[test]
fn fixed_width_integers_carry_their_type_range() {
    let (mut service, bindings, options) = inputs();
    if let Some(crucible_smithy::ir::Shape::Structure(s)) =
        service.shapes.get_mut(&"example#Settings".parse().unwrap())
    {
        s.members[1].constraints = Default::default();
    }
    let plan = resolve(&service, &bindings).unwrap();
    let files = emit(&service, &plan, &options).unwrap();
    assert!(
        files[0]
            .contents
            .contains("\"intRange\":[-2147483648,2147483647]")
    );
}

#[test]
fn timestamps_use_explicit_provider_representation() {
    let (mut service, bindings, options) = inputs();
    let id = "example#Epoch".parse().unwrap();
    service.shapes.insert(
        id,
        crucible_smithy::ir::Shape::Simple(crucible_smithy::ir::Simple {
            streaming: false,
            requires_length: false,
            kind: crucible_smithy::ir::SimpleKind::Timestamp,
            constraints: Default::default(),
            timestamp_format: None,
            docs: None,
            traits: Default::default(),
        }),
    );
    let mut plan = resolve(&service, &bindings).unwrap();
    let property = plan.resources[0].properties.get_mut("Name").unwrap();
    property.target = "example#Epoch".parse().unwrap();
    property.timestamp_format = Some(crucible_smithy::ir::TimestampFormat::EpochSeconds);
    plan.resources[0].attributes.get_mut("Id").unwrap().target = "example#Epoch".parse().unwrap();
    let files = emit(&service, &plan, &options).unwrap();
    assert!(files[0].contents.contains("readonly name: number;"));
    assert!(files[0].contents.contains("readonly attrId: string;"));
    assert!(
        files[0]
            .contents
            .contains("\"timestamp\":\"epoch-seconds\"")
    );
}

#[test]
fn property_only_packages_do_not_import_unused_resolvable_type() {
    let (service, bindings, options) = inputs();
    let mut plan = resolve(&service, &bindings).unwrap();
    plan.resources[0]
        .properties
        .retain(|name, _| name == "Name");
    let files = emit(&service, &plan, &options).unwrap();
    assert!(!files[0].contents.contains("IResolvable"));
}

#[test]
fn recursive_collection_aliases_are_diagnosed_instead_of_erased() {
    let (mut service, bindings, options) = inputs();
    if let Some(crucible_smithy::ir::Shape::List(list)) =
        service.shapes.get_mut(&"example#Tags".parse().unwrap())
    {
        list.member.target = "example#Tags".parse().unwrap();
    }
    let plan = resolve(&service, &bindings).unwrap();
    assert!(
        emit(&service, &plan, &options)
            .unwrap_err()
            .to_string()
            .contains("recursive collection aliases")
    );
}

#[test]
fn csharp_class_member_collision_is_diagnosed_before_emission() {
    let (service, bindings, options) = inputs();
    let mut plan = resolve(&service, &bindings).unwrap();
    plan.resources[0].cdk.as_mut().unwrap().class_name = Some("AttrId".into());
    assert!(
        emit(&service, &plan, &options)
            .unwrap_err()
            .to_string()
            .contains("enclosing class in .NET")
    );
}

#[test]
fn runtime_rejects_unknown_keys_aliasing_a_member_wire_name() {
    let (service, bindings, options) = inputs();
    let plan = resolve(&service, &bindings).unwrap();
    let files = emit(&service, &plan, &options).unwrap();
    let runtime = files
        .iter()
        .find(|file| file.path == "src/validation.ts")
        .unwrap();
    assert!(
        runtime.contents.contains("aliases '${owner}'"),
        "{}",
        runtime.contents
    );
}
