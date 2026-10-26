use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::{Deserialize, Serialize, Serializer};

use super::{entry, identity_entry, versioned_entry};
use crate::Operation;
use crate::error::ModelledError;
use crate::operation::{AuthScheme, OperationInfo, Output, OutputFromJson, ServiceInfo};
use crate::permissions::Requirements;
use crate::validate::{Validate, Validator};
use crate::versioning::{ApiVersion, Conversion, Transform};
use crucible_wire::__generated::{operation_info, service_info};

static INPUT_JSON: AtomicUsize = AtomicUsize::new(0);
static OUTPUT_JSON: AtomicUsize = AtomicUsize::new(0);
static CURRENT_VALIDATIONS: AtomicUsize = AtomicUsize::new(0);
static OLD_VALIDATIONS: AtomicUsize = AtomicUsize::new(0);

fn input_json<S: Serializer>(value: &i32, serializer: S) -> Result<S::Ok, S::Error> {
    INPUT_JSON.fetch_add(1, Ordering::Relaxed);
    value.serialize(serializer)
}

fn output_json<S: Serializer>(value: &i32, serializer: S) -> Result<S::Ok, S::Error> {
    OUTPUT_JSON.fetch_add(1, Ordering::Relaxed);
    value.serialize(serializer)
}

#[derive(Serialize, Deserialize)]
struct CurrentInput {
    #[serde(serialize_with = "input_json")]
    value: i32,
}
impl Validate for CurrentInput {
    fn validate(&self, _: &mut Validator) {
        CURRENT_VALIDATIONS.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Serialize, Deserialize)]
struct OldInput {
    #[serde(serialize_with = "input_json")]
    value: i32,
}
impl Validate for OldInput {
    fn validate(&self, _: &mut Validator) {
        OLD_VALIDATIONS.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Serialize, Deserialize)]
struct CurrentOutput {
    #[serde(serialize_with = "output_json")]
    value: i32,
}
#[derive(Serialize, Deserialize)]
struct OldOutput {
    #[serde(serialize_with = "output_json")]
    value: i32,
}

static CURRENT: OperationInfo = operation_info(
    "test#Counted",
    "Counted",
    None,
    "POST",
    &[],
    &[],
    200,
    &[],
    &[],
    &[],
    false,
    false,
    false,
    None,
    &[],
);
static OLDER: OperationInfo = operation_info(
    "test#Counted",
    "Counted",
    None,
    "PUT",
    &[],
    &[],
    201,
    &[],
    &[],
    &[],
    false,
    false,
    false,
    None,
    &[],
);
fn classify<'a>(_: &'a (dyn std::error::Error + 'static)) -> Option<&'a dyn ModelledError> {
    None
}
static SERVICE: ServiceInfo = service_info(
    "test#Service",
    "v2",
    None,
    &[&CURRENT],
    &[],
    classify,
    None,
    None,
    None,
);

struct CurrentOperation;
impl Operation for CurrentOperation {
    const INFO: &'static OperationInfo = &CURRENT;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = Some(&[]);
    const REQUIREMENTS: Requirements = Requirements::None;
    const PUBLIC: bool = true;
    const OUTPUT_FROM_JSON: Option<OutputFromJson<CurrentOutput>> = Some(serde_json::from_value);
    type Input = CurrentInput;
    type Output = CurrentOutput;
}
struct OldOperation;
impl Operation for OldOperation {
    const INFO: &'static OperationInfo = &OLDER;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = Some(&[AuthScheme::Bearer]);
    type Input = OldInput;
    type Output = OldOutput;
}

async fn invoke(entry: &crate::operations::Entry) -> Box<dyn Output> {
    let mut input = (entry.decode_json)(br#"{"value":4}"#).unwrap();
    if let Some(upgrade) = &entry.upgrade {
        input = upgrade(input).await.unwrap();
    }
    let mut output = (entry.invoke)(input).await.unwrap();
    if let Some(downgrade) = &entry.downgrade {
        output = downgrade(output).await.unwrap();
    }
    output
}

#[tokio::test]
async fn identity_decodes_current_types_without_either_intermediate_json_serialization() {
    for count in [
        &INPUT_JSON,
        &OUTPUT_JSON,
        &CURRENT_VALIDATIONS,
        &OLD_VALIDATIONS,
    ] {
        count.store(0, Ordering::Relaxed);
    }
    let mut current = entry::<CurrentOperation, (), _>(Arc::new(()), |_, input| async move {
        Ok(CurrentOutput {
            value: input.value + 1,
        })
    });
    current.body_limit = Some(128);
    let identity = identity_entry::<OldOperation>(&current);
    assert!(std::ptr::eq(identity.info, &OLDER));
    assert_eq!(identity.info.status, 201);
    assert!(identity.metadata.bearer_auth);
    assert_eq!(*identity.metadata.requirements, Requirements::None);
    assert!(identity.metadata.public);
    assert!(identity.metadata.replays_output);
    assert!(identity.output_from_json.is_some());
    assert_eq!(identity.body_limit, Some(128));
    assert!(identity.upgrade.is_none());
    assert!(identity.downgrade.is_none());
    assert!(identity.downgrade_error.is_none());
    let output = invoke(&identity).await;
    assert_eq!(
        output
            .as_any()
            .downcast_ref::<CurrentOutput>()
            .unwrap()
            .value,
        5
    );
    assert_eq!(INPUT_JSON.load(Ordering::Relaxed), 0);
    assert_eq!(OUTPUT_JSON.load(Ordering::Relaxed), 0);
    assert_eq!(CURRENT_VALIDATIONS.load(Ordering::Relaxed), 1);
    assert_eq!(OLD_VALIDATIONS.load(Ordering::Relaxed), 0);
    assert!((identity.decode_json)(br#"{"value":"wrong"}"#).is_err());

    // The previous empty-chain implementation really performs both JSON
    // conversions and uses the older codec before upgrading to CurrentInput.
    let slow =
        versioned_entry::<OldOperation, CurrentOperation>(&current, &SERVICE, &[], Arc::default());
    let output = invoke(&slow).await;
    assert_eq!(
        output.as_any().downcast_ref::<OldOutput>().unwrap().value,
        5
    );
    assert_eq!(INPUT_JSON.load(Ordering::Relaxed), 1);
    assert_eq!(OUTPUT_JSON.load(Ordering::Relaxed), 1);
    assert_eq!(CURRENT_VALIDATIONS.load(Ordering::Relaxed), 2);
    assert_eq!(OLD_VALIDATIONS.load(Ordering::Relaxed), 1);
}

#[test]
fn a_nonempty_or_refused_chain_cannot_bypass_the_conversion_path() {
    static CHANGE: Conversion = Conversion {
        upgrade: &[Transform::Default {
            at: &[],
            member: "added",
            value: "1",
        }],
        ..Conversion::NONE
    };
    static REFUSED: Conversion = Conversion {
        refused: Some("not served"),
        ..Conversion::NONE
    };
    const V1: ApiVersion = ApiVersion::new(0, "v1");
    const V2: ApiVersion = ApiVersion::new(1, "v2");
    static CHANGED_CHAIN: &[&Conversion] = &[&CHANGE];
    static REFUSED_CHAIN: &[&Conversion] = &[&REFUSED];
    static OLDER_VERSIONS: &[(ApiVersion, Option<&str>, &ServiceInfo)] = &[(V1, None, &SERVICE)];
    for (chain, refused) in [(CHANGED_CHAIN, false), (REFUSED_CHAIN, true)] {
        let operations = crate::Operations::builder(&SERVICE)
            .add::<CurrentOperation, _, _>(Arc::new(()), |_, input| async move {
                Ok(CurrentOutput { value: input.value })
            })
            .versions(V2, OLDER_VERSIONS)
            .add_versioned_identity::<OldOperation, CurrentOperation>(V1, chain)
            .build()
            .unwrap();
        let entry = operations.versions.older[0].operations.entry("Counted");
        if refused {
            assert!(entry.is_none());
        } else {
            let entry = entry.unwrap();
            assert!(entry.upgrade.is_some());
            assert!(entry.downgrade.is_some());
        }
    }
}
