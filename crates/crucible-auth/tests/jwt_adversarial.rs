#![cfg(feature = "test-support")]
use crucible_auth::{Algorithm, AuthError, JwtAuthenticator, test_support::TestTokenIssuer};
use jsonwebtoken::{EncodingKey, Header, encode};
use serde_json::json;

#[tokio::test]
async fn algorithms_signature_and_required_claims_fail_closed() {
    let issuer = TestTokenIssuer::es256("https://issuer.example", "known").unwrap();
    let auth =
        JwtAuthenticator::with_jwks(issuer.config(vec!["client".into()]), issuer.jwks()).unwrap();
    let claims = issuer.id_claims("person", "client");
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some("known".into());
    let forged = encode(
        &header,
        &claims,
        &EncodingKey::from_secret(b"public-key-material"),
    )
    .unwrap();
    assert!(auth.verify(&forged).await.is_err());
    let other = TestTokenIssuer::es256("https://issuer.example", "known").unwrap();
    assert!(auth.verify(&other.mint(&claims).unwrap()).await.is_err());
    for name in ["sub", "iss", "exp", "aud", "token_use"] {
        let mut wrong = claims.clone();
        wrong.as_object_mut().unwrap().remove(name);
        assert!(
            auth.verify(&issuer.mint(&wrong).unwrap()).await.is_err(),
            "missing {name}"
        );
        let mut wrong = claims.clone();
        wrong[name] = json!({});
        assert!(
            auth.verify(&issuer.mint(&wrong).unwrap()).await.is_err(),
            "mistyped {name}"
        );
    }
    assert!(
        auth.verify("eyJhbGciOiJub25lIiwia2lkIjoia25vd24ifQ.e30.")
            .await
            .is_err()
    );
}

/// GHSA-h395-gr6q-cpjc: jsonwebtoken < 10.3 treated a wrongly typed claim as
/// absent, so its check was skipped. A numeric claim sent as text must refuse.
#[tokio::test]
async fn claims_with_the_wrong_json_type_are_refused() {
    let issuer = TestTokenIssuer::es256("https://issuer.example", "known").unwrap();
    let auth =
        JwtAuthenticator::with_jwks(issuer.config(vec!["client".into()]), issuer.jwks()).unwrap();
    let claims = issuer.id_claims("person", "client");
    let far_future = "4102444800";
    for (name, value) in [
        ("exp", json!(far_future)),
        ("nbf", json!(far_future)),
        ("nbf", json!("0")),
        ("aud", json!(1)),
        ("iss", json!(1)),
        ("sub", json!(1)),
    ] {
        let mut wrong = claims.clone();
        wrong[name] = value.clone();
        assert!(
            auth.verify(&issuer.mint(&wrong).unwrap()).await.is_err(),
            "{name} = {value}"
        );
    }
}

#[test]
fn public_jwks_rejects_ambiguous_kids_and_non_signing_keys() {
    let issuer = TestTokenIssuer::es256("https://issuer.example", "known").unwrap();
    let config = issuer.config(vec!["client".into()]);
    let key = issuer.jwks()["keys"][0].clone();
    assert!(JwtAuthenticator::with_jwks(config.clone(), json!({"keys":[key, key]})).is_err());
    for (name, value) in [
        ("use", json!("enc")),
        ("alg", json!("HS256")),
        ("key_ops", json!(["encrypt"])),
    ] {
        let mut wrong = key.clone();
        wrong[name] = value;
        assert!(
            JwtAuthenticator::with_jwks(config.clone(), json!({"keys":[wrong]})).is_err(),
            "{name}"
        );
    }
}

#[tokio::test]
async fn token_algorithm_cannot_change_a_known_keys_family() {
    let ec = TestTokenIssuer::es256("https://issuer.example", "known").unwrap();
    let rsa = TestTokenIssuer::new("https://issuer.example", "known").unwrap();
    let mut config = ec.config(vec!["client".into()]);
    config.algorithms = vec![Algorithm::RS256, Algorithm::ES256];
    let auth = JwtAuthenticator::with_jwks(config, ec.jwks()).unwrap();
    let token = rsa.mint(&rsa.id_claims("person", "client")).unwrap();
    assert!(matches!(auth.verify(&token).await, Err(AuthError::Refused)));
}
