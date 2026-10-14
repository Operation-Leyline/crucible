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

// Typed permissions (docs/permissions.md): a resource declares the
// permissions it accepts, an operation what it requires and on which
// resource. Crucible never decides: the service's `Authorizer` does.

/// The permissions a resource accepts, one enum member each.
///
/// The member name is the permission's symbol in generated code; the enum
/// value is its identity, stored in grants and audit records together with
/// the resource's authorization name. A value is segments of lowercase-led
/// letters and digits joined by `:`, with single `-` inside a segment
/// (`mail:send`, `api-key:manage`), compared case-sensitively.
///
/// Per member: `@internal` keeps a permission out of every client-facing
/// output, `@permissionOnly` marks one no operation requires, `@notGrantable`
/// one the service's policy confers whole, and `@aliasOf` with `@deprecated`
/// keeps a renamed permission's old value. One enum per resource.
@trait(selector: "enum")
@idRef(failWhenMissing: true, selector: "resource")
string permissionsOf

/// The resource's name in grants and audit records, instead of its shape
/// name in lowerCamelCase. Set it before renaming a shape whose permissions
/// are released: the name is stored data.
@trait(selector: "resource")
@pattern("^[a-z][a-zA-Z0-9]*$")
string authorizationName

/// A permission no operation requires: checked by the service's policy
/// (a role bundle, a delegation rule). Documented in its own table.
@trait(selector: "enum > member", conflicts: [notGrantable])
structure permissionOnly {}

/// A permission the service's policy confers whole, such as an
/// administrator bundle: never offered for composition.
@trait(selector: "enum > member", conflicts: [permissionOnly])
structure notGrantable {}

/// This value is a deprecated alias of the named member of the same enum:
/// a grant stored under it satisfies a check of the primary.
@trait(selector: "enum > member")
string aliasOf

/// The wire enum of resource types a grant can name, checked both ways:
/// every value is the authorization name of a resource with permissions,
/// and every resource with grantable permissions is a value or excluded.
@trait(selector: "enum")
structure permissionTypes {
    exclude: PermissionTypesExclusions
}

@private
list PermissionTypesExclusions {
    @idRef(failWhenMissing: true, selector: "resource")
    member: String
}

/// What the operation requires: every entry must hold (AND), in order.
///
/// `permission` is the value, `resource` the resource that declares it.
/// `bind` maps an identifier of the resource to `"*"` (every instance; the
/// resource's own identifier only), `"$label"` (the member bound to that URI
/// label) or a member path (`"content.template.templateId"`); an identifier
/// not listed binds the input member of its name. `when` is a condition over
/// the input (`has a.b`, `a == "x"`, `a in ["x", "y"]`, `and`, `or`, `not`).
/// `mode` is `check` (the default), `filter` (a listing answered with what
/// the caller may see) or `handler` (an obligation the handler discharges;
/// no `bind` or `when`).
@trait(selector: "operation", conflicts: [requiresNone, requiresPending])
list requires {
    member: PermissionRequirement
}

@private
structure PermissionRequirement {
    @required
    permission: String

    @required
    @idRef(failWhenMissing: true, selector: "resource")
    resource: String

    bind: PermissionBindings

    when: String

    mode: PermissionRequirementMode

    documentation: String
}

@private
map PermissionBindings {
    key: String
    value: String
}

@private
enum PermissionRequirementMode {
    CHECK = "check"
    FILTER = "filter"
    HANDLER = "handler"
}

/// An authenticated operation that needs no permission; the value says why.
@trait(selector: "operation", conflicts: [requires, requiresPending])
string requiresNone

/// A migration suppression: the handler still checks, for the reason given,
/// until the operation declares its requirements.
@trait(selector: "operation", conflicts: [requires, requiresNone])
@length(min: 1)
string requiresPending
