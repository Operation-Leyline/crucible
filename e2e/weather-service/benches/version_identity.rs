//! The complete in-process HTTP request for a version whose conversion is empty.
//! Compare the same benchmark before/after changing identity registration.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use criterion::{Criterion, criterion_group, criterion_main};
use crucible_server_runtime_rust::__generated as generated;
use crucible_server_runtime_rust::{Crucible, Operation, Unversioned};
use std::sync::Arc;

include!(concat!(env!("OUT_DIR"), "/versioned.rs"));
include!("../tests/support/versioned_shop.rs");

async fn request(service: &Crucible, version: &'static str) {
    let response = service
        .handle(
            Request::post("/payments")
                .header("crucible-version", version)
                .header("payment-signature", "t=1,v1=abc")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["crucible-version"], version);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(
        body.as_ref(),
        format!(r#"{{"release":"{version}"}}"#).as_bytes()
    );
}

// The same generated payment operation, with only the old registration
// strategy changed. Both services use the same handler, descriptions and
// codecs; they are measured in one optimized binary on the shared machine.
fn payment_service(identity: bool) -> Crucible {
    static CURRENT: generated::ServiceInfo = generated::service_info(
        "example.shop#PaymentBench",
        "v4",
        None,
        &[shop::RecordPayment::INFO],
        shop::SERVICE.errors,
        shop::SERVICE.classify,
        shop::SERVICE.reject,
        shop::SERVICE.internal,
        shop::SERVICE.unknown,
    );
    static OLDER: generated::ServiceInfo = generated::service_info(
        "example.shop#PaymentBench",
        "v1",
        None,
        &[shop::versions::v1::RecordPayment::INFO],
        shop::versions::v1::SERVICE.errors,
        shop::versions::v1::SERVICE.classify,
        shop::versions::v1::SERVICE.reject,
        shop::versions::v1::SERVICE.internal,
        shop::versions::v1::SERVICE.unknown,
    );
    static VERSIONS: &[(
        generated::versioning::ApiVersion,
        Option<&str>,
        &generated::ServiceInfo,
    )] = &[(shop::versions::V1, None, &OLDER)];
    let builder = generated::Operations::builder(&CURRENT)
        .add::<shop::RecordPayment, _, _>(Arc::new(()), |_, input| async move {
            assert_eq!(input.signature, "t=1,v1=abc");
            Ok(shop::RecordPaymentOutput {
                release: "v1".into(),
            })
        })
        .versions(shop::versions::CURRENT, VERSIONS);
    let builder = if identity {
        builder.add_versioned_identity::<shop::versions::v1::RecordPayment, shop::RecordPayment>(
            shop::versions::V1,
            &[],
        )
    } else {
        builder.add_versioned::<shop::versions::v1::RecordPayment, shop::RecordPayment>(
            shop::versions::V1,
            &[],
            None,
        )
    };
    Crucible::builder(builder.build().unwrap())
        .defaults()
        .api_version(Unversioned::Required)
        .build()
        .unwrap()
}

fn versions(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let service = versioned_shop::service(Unversioned::Oldest);
    for (name, service) in [
        ("paired_payment_conversion", payment_service(false)),
        ("paired_payment_identity", payment_service(true)),
    ] {
        c.bench_function(name, |b| {
            b.to_async(&runtime).iter(|| request(&service, "v1"))
        });
    }
    for version in ["v1", "v4"] {
        c.bench_function(&format!("record_payment_{version}"), |b| {
            b.to_async(&runtime).iter(|| request(&service, version))
        });
    }
}

criterion_group!(benches, versions);
criterion_main!(benches);
