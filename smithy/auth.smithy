$version: "2"

// Authorization vocabulary a model shares with the server framework. A model
// that uses it imports this file beside crucible.smithy (see README.md).

namespace crucible.auth

/// The OAuth 2.0 scopes a caller's access token must carry (RFC 6749 §3.3).
///
/// - On an operation, every listed scope is required (AND), as in an OpenAPI
///   security requirement. An empty list requires none, which also overrides
///   a service default.
/// - On a service, the list is the default for operations that declare none.
///   It is also the scope a `401` challenge advertises.
/// - Scopes match literally against the space-separated `scope` claim. A scope
///   is a nonempty token of printable ASCII without spaces, `"` or `\`.
///
/// Declaring scopes does not enforce them: install `ScopeEnforcer` from
/// `crucible-auth`. Operations that are public (`@auth([])`) cannot declare any.
@trait(selector: ":is(service, operation)")
list scopes {
    member: ScopeName
}

/// One scope token.
@length(min: 1)
@pattern("^[\\x21\\x23-\\x5B\\x5D-\\x7E]+$")
string ScopeName
