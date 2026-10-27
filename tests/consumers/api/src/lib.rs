#![deny(deprecated)]
#[cfg(feature = "server")]
pub fn server_identity(value: crucible_api::server::Blob) -> server_runtime::Blob {
    value
}
#[cfg(feature = "client")]
pub fn client_identity(value: crucible_api::client::Token) -> client_runtime::Token {
    value
}
#[cfg(feature = "auth")]
pub fn auth_identity(value: crucible_api::auth::Principal) -> server_auth::Principal {
    value
}
#[cfg(all(test, feature = "client"))]
#[test]
fn public_client_configuration_constructs_without_network() {
    let _config = crucible_api::client::Config::new("https://api.example.com")
        .token(crucible_api::client::Token::fixed("token"));
}
