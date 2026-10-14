//! Hover: what the word under the caret means. A keyword, built-in type or
//! built-in annotation explains itself from [`KEYWORDS`]; a declaration, or
//! a name that refers to one, shows its whole shape with its constraints; a
//! member shows its type, whether it is required, its default and every
//! constraint that applies to it. Code goes in fenced blocks tagged
//! [`FENCE`], so an editor highlights it as Crucible; prose stays outside.

use crucible_lang::ast::{
    Annotation, Arg, BindingValue, Body, BodyKind, Clause, ClauseValue, Condition, ConditionKind,
    Constraint, ConstraintValue, Decl, EnumValue, ErrorEntry, Ident, Item, Key, Member, MemberKind,
    MetaType, NumLit, OpEntry, PermissionEntry, Requirement, RequirementKind, ResourceEntry,
    ResourceEntryKind, TypeRef, Value,
};
use crucible_lang::{Parsed, TokenKind};

use crate::analysis::{self, BUILTIN_TYPES, DEFAULT_LIBRARY, Index, Role};
use crate::insight;

/// The language fenced code is tagged with. The IntelliJ plugin registers
/// its language with the id `Crucible` (`CrucibleLanguage`), and the
/// platform matches a fence's tag to a language id ignoring case, so its
/// own highlighter colors the block; `crucible` is also the `languageId`
/// clients open `.cru` documents with.
pub const FENCE: &str = "crucible";

/// Contexts a [`KEYWORDS`] row applies in.
const DECLARATION: &[&str] = &["declaration"];
const ANYWHERE: &[&str] = &[];
// Templates are not compiled yet, so words inside one explain nothing.
const OPERATION: &[&str] = &["operation"];
const SERVICE: &[&str] = &["service"];
const RESOURCE: &[&str] = &["resource"];
const ERROR: &[&str] = &["error"];
const METADATA: &[&str] = &["metadata"];
const MIGRATION: &[&str] = &["migration"];
const EXAMPLE: &[&str] = &["example"];
/// Where a member may elide a resource entry: views and operation bodies.
const MEMBERS: &[&str] = &["structure", "operation"];
const TYPE: &[&str] = &["type"];

/// What each keyword, built-in type and built-in annotation means: the word
/// (`@name` for an annotation), where it means that, an explanation and a
/// small example. A context is `declaration` for the word that starts a
/// declaration, `type` for a built-in type, `example` inside an operation's
/// example, and otherwise the kind of declaration the word is in; no
/// contexts means anywhere. The text follows `docs/idl/reference.md` and
/// the decisions it cites; a test parses every example and hovers the word
/// in it, so each row is reachable and each example is valid syntax.
const KEYWORDS: &[Row] = &[
    // ---- declarations ------------------------------------------------------
    (
        "service",
        DECLARATION,
        "Declares a service: its `version` and `protocol` (both required), its `auth` scheme, and the `operations`, root `resources` and `errors` it serves.",
        "service Weather {\n    version \"2026-10-01\"\n    protocol json\n    auth bearer\n    operations [Ping]\n    resources [City]\n}",
    ),
    (
        "resource",
        DECLARATION,
        "Declares a resource by what it is: its identifiers (`id`), its properties (`property`) and, with `in`, a parent whose identifiers it inherits. Operations attach themselves with a role such as `reads`; a resource never lists them.",
        "resource City in Country {\n    id cityId: CityId\n    property name: CityName\n}",
    ),
    (
        "operation",
        DECLARATION,
        "Declares an operation: its `http` binding, its anonymous `input` and `output` bodies and its `errors`. A role after the name (`reads City`) attaches it to a resource; `readonly`, `idempotent` and `public` go before `operation`.",
        "readonly operation GetCity reads City {\n    http GET \"/cities/{cityId}\"\n    input { $cityId }\n    output { city: CityData }\n    errors [CityNotFound]\n}",
    ),
    (
        "structure",
        DECLARATION,
        "Declares a structure of named members. A member is required unless marked `?`, and `= value` gives it a default. `for Resource` makes the structure a view of a resource.",
        "structure Coordinates {\n    latitude: double { range -90..90 }\n    longitude: double\n    label?: string\n}",
    ),
    (
        "union",
        DECLARATION,
        "Declares a tagged union: a value is exactly one of its members. A member without a type carries nothing.",
        "union Shape {\n    circle: Circle\n    square: Square\n    empty\n}",
    ),
    (
        "error",
        DECLARATION,
        "Declares an error: a structure sent with an HTTP `status`, 4xx for a client fault and 5xx for a server fault. `retryable` lets clients retry. Operations and services list their errors in `errors [..]`.",
        "error CityNotFound {\n    status 404\n    message: string\n}",
    ),
    (
        "enum",
        DECLARATION,
        "Declares an enum. A value's name is also its wire value unless `= \"..\"` gives another. `enum Name: integer` declares an integer enum, whose values are numbers.",
        "enum Size {\n    SMALL = \"small\"\n    LARGE\n}",
    ),
    (
        "list",
        DECLARATION,
        "Declares a list of one item type. The constraints in braces are the list's own: `length` bounds the number of items and `unique` forbids duplicates.",
        "list CityList: CityData { length ..100 }",
    ),
    (
        "map",
        DECLARATION,
        "Declares a map from a key type to a value type. Keys are strings or string enums; `length` bounds the number of entries.",
        "map Labels: string -> LabelValue { length ..50 }",
    ),
    (
        "metadata",
        DECLARATION,
        "Declares a project metadata key: what it applies to after `on`, an optional value type, and optional `conflicts` and `retention`. It is applied as `@key` or `@key(value)`.",
        "metadata tier on operation: enum { GOLD, SILVER }",
    ),
    (
        "migration",
        DECLARATION,
        "Declares how the current model serves the previous release: each step (`rename`, `require`, `add`, `move`, `reshape`, `remove operation`, `behavior`, `sunset`, `retype`) is one breaking change since then. The compiler checks the steps against the frozen release.",
        "migration \"2026-11-01\" from \"2026-10-01\" {\n    rename CityData.label -> name\n}",
    ),
    (
        "rule",
        DECLARATION,
        "Declares a policy rule (`every .. must ..`, `no ..`). Not supported by the compiler yet: policy lives in `crucible.toml`.",
        "rule Documented {\n    every operation\n    must documented\n}",
    ),
    (
        "template",
        DECLARATION,
        "Declares what every operation of a role carries, such as the pagination members of every `lists` operation. Not supported by the compiler yet.",
        "template lists {\n    paginated(items: items)\n}",
    ),
    (
        "except",
        DECLARATION,
        "Excepts one shape or operation from a rule, with the reason. Not supported by the compiler yet.",
        "except Documented for Ping because \"a health check\"",
    ),
    (
        "protocol",
        DECLARATION,
        "Declares a protocol and its settings. Not supported by the compiler yet; a service names the built-in Crucible JSON protocol with `protocol json`.",
        "protocol json {}",
    ),
    (
        "conversion",
        DECLARATION,
        "Declares a reusable type change, served by a hook, for the `retype` steps between the two types. Not supported by the compiler yet.",
        "conversion MicroUsd -> Money by hook",
    ),
    // ---- modifiers ---------------------------------------------------------
    (
        "readonly",
        // An operation's modifier, and a flag rules test; not a property's.
        &["operation", "rule"],
        "Marks an operation that only reads, so it is safe to call again. Under `http`, GET and HEAD must be `readonly`; the `reads` and `lists` roles imply it.",
        "readonly operation GetStatus {\n    http GET \"/status\"\n}",
    ),
    (
        "idempotent",
        ANYWHERE,
        "Marks an operation whose effect is the same however often it is repeated with the same input, so clients may retry it. PUT and DELETE must be `idempotent` or `readonly`; the `puts` and `deletes` roles and a `@token` input member imply it.",
        "idempotent operation PutCity puts City {\n    http PUT \"/cities/{cityId}\"\n    input { $cityId }\n}",
    ),
    (
        "public",
        ANYWHERE,
        "Marks an operation that needs no authentication.",
        "public readonly operation Ping {\n    http GET \"/ping\"\n}",
    ),
    (
        "strict",
        ANYWHERE,
        "Makes a structure or input refuse members the model does not define: the server answers an unknown member with a validation error. Clients never enforce it, so a newer service can add members.",
        "operation CreateCity {\n    strict input {\n        name: string\n    }\n}",
    ),
    (
        "internal",
        ANYWHERE,
        "Hides a shape, member, operation or enum value from the external audience: clients and docs generated for external callers leave it out. The server still serves it.",
        "structure City {\n    name: string\n    internal shardId?: string\n}",
    ),
    (
        "sensitive",
        ANYWHERE,
        "Marks a shape, member or operation whose values are redacted from logs and debug output.",
        "structure Login {\n    user: string\n    sensitive password: string\n}",
    ),
    // ---- resources ---------------------------------------------------------
    (
        "id",
        RESOURCE,
        "Declares an identifier of the resource. Identifiers are strings or string enums, and a child resource inherits its parent's.",
        "resource City {\n    id cityId: CityId\n}",
    ),
    (
        "property",
        RESOURCE,
        "Declares a property of the resource. Views and attached operations bind it with `$name`, taking its type and documentation.",
        "resource City {\n    id cityId: CityId\n    property name: CityName\n}",
    ),
    (
        "readonly",
        RESOURCE,
        "On a property: the service assigns it. A `creates`, `puts` or `updates` input cannot bind it; views and outputs return it.",
        "resource City {\n    id cityId: CityId\n    readonly property createdAt: DateTime\n}",
    ),
    (
        "immutable",
        RESOURCE,
        "On a property: set by a `creates` or `puts` input and fixed afterwards, so an `updates` input cannot bind it and changing it replaces the resource. Some `creates` or `puts` must bind it. `immutable writeonly` is set once and never returned.",
        "resource Graph {\n    id graphId: GraphId\n    immutable property homeRegion: Region\n}",
    ),
    (
        "writeonly",
        RESOURCE,
        "On a property: written by inputs and never returned, so no view or output may show it and `$*` leaves it out. `immutable writeonly` is set once at creation.",
        "resource Account {\n    id accountId: AccountId\n    writeonly property password: Password\n}",
    ),
    (
        "as",
        RESOURCE,
        "On an identifier: its name in the resource's own views and outputs, where `$cityId` and `$*` show it under this name. Inputs still bind it by its identifier name, the URI label's.",
        "resource City {\n    id cityId: CityId as id\n}",
    ),
    (
        "in",
        RESOURCE,
        "Makes the resource a child of another: it inherits the parent's identifiers and is addressed by both.",
        "resource District in City {\n    id districtId: DistrictId\n}",
    ),
    // ---- operations --------------------------------------------------------
    (
        "http",
        OPERATION,
        "The operation's HTTP binding: method, URI and success status (200 unless `->` gives another). A `{label}` in the URI binds the input member of that name; `{name+}` is greedy.",
        "operation CreateCity {\n    http POST \"/cities\" -> 201\n}",
    ),
    (
        "input",
        &["operation", "migration"],
        "The operation's input: an anonymous body of members, never a named type. `$name` binds an identifier or property of the operation's resource; `strict input` refuses unknown members.",
        "operation GetCity reads City {\n    input {\n        $cityId\n        @query verbose?: boolean\n    }\n}",
    ),
    (
        "output",
        &["operation", "migration"],
        "The operation's output: an anonymous body of members. A member whose type is a `for` view of a resource, or a list or map of one, carries the resource's properties.",
        "operation GetCity reads City {\n    output {\n        city: CityData\n    }\n}",
    ),
    (
        "errors",
        OPERATION,
        "The errors this operation may return, besides the service's.",
        "operation GetCity {\n    errors [CityNotFound]\n}",
    ),
    (
        "errors",
        SERVICE,
        "Errors every operation of the service may return, after its own.",
        "service Weather {\n    errors [ThrottlingError]\n}",
    ),
    (
        "example",
        &["operation", "example"],
        "A worked example: an input with the output or the error it produces, checked against the shapes and shown in docs and OpenAPI.",
        "operation GetCity {\n    example \"Lisbon\" {\n        input { cityId: \"lis\" }\n        output { city: { name: \"Lisbon\" } }\n    }\n}",
    ),
    (
        "input",
        EXAMPLE,
        "The example's input values.",
        "operation GetCity {\n    example \"Lisbon\" {\n        input { cityId: \"lis\" }\n    }\n}",
    ),
    (
        "output",
        EXAMPLE,
        "The output the example's input produces.",
        "operation GetCity {\n    example \"Lisbon\" {\n        output { city: { name: \"Lisbon\" } }\n    }\n}",
    ),
    (
        "error",
        EXAMPLE,
        "The error the example's input produces, by name, with its values.",
        "operation GetCity {\n    example \"Unknown\" {\n        input { cityId: \"x\" }\n        error CityNotFound { message: \"no such city\" }\n    }\n}",
    ),
    (
        "paginated",
        OPERATION,
        "Paginates the operation: the members that carry the input token, the output token, the items and the page size. A value the operation leaves out comes from the service's defaults.",
        "operation ListCities {\n    paginated(inputToken: nextToken, outputToken: nextToken, items: cities)\n}",
    ),
    (
        "paginated",
        SERVICE,
        "Pagination defaults every paginated operation inherits; an operation's own values win.",
        "service Weather {\n    paginated(inputToken: nextToken, outputToken: nextToken)\n}",
    ),
    (
        "requires",
        OPERATION,
        "What the caller must hold before the operation runs: a permission on the resource instance the bindings identify (`on Inbox`, `on Template(content.template.templateId)`, `on Inbox(*)`), optionally `when` a condition on the input holds. Every clause must hold. `requires none` says no permission is needed, `requires pending because \"..\"` defers to the handler during migration, and `by handler` declares a check the handler discharges.",
        "operation SendMessage on Message collection {\n    requires mailSend on Inbox\n    requires read on Template(content.template.templateId) when has content.template\n}",
    ),
    (
        "filters",
        OPERATION,
        "Answers a listing with only the instances the caller holds the permission on: the service's authorizer says what is visible under the bound parent, and the handler filters to it.",
        "operation ListInboxes lists Inbox {\n    filters read on Inbox\n}",
    ),
    (
        "permission",
        RESOURCE,
        "A permission the resource accepts: its symbol in generated code, and its value, stored in grants and audit records with the resource's authorization name. `internal` keeps it out of client SDKs, docs and 403 bodies.",
        "resource Inbox {\n    /// Send from this address.\n    permission mailSend = \"mail:send\"\n}",
    ),
    (
        "creates",
        OPERATION,
        "Attaches the operation to a resource as its create. The input binds only the parent's identifiers; the server assigns the new one (a client-chosen identifier is `puts`).",
        "operation CreateCity creates City {\n    http POST \"/cities\" -> 201\n}",
    ),
    (
        "puts",
        OPERATION,
        "Attaches the operation as the resource's create-or-replace at a client-chosen identifier. The input binds every identifier; implies `idempotent`.",
        "operation PutCity puts City {\n    http PUT \"/cities/{cityId}\"\n    input { $cityId }\n}",
    ),
    (
        "reads",
        OPERATION,
        "Attaches the operation as the read of one resource instance. The input binds every identifier, inherited ones too; implies `readonly`.",
        "operation GetCity reads City {\n    http GET \"/cities/{cityId}\"\n    input { $cityId }\n}",
    ),
    (
        "updates",
        OPERATION,
        "Attaches the operation as the update of one resource instance. The input binds every identifier. It implies nothing about repeating: an update that is safe to repeat says `idempotent`, and one served by PUT must.",
        "operation UpdateCity updates City {\n    http PATCH \"/cities/{cityId}\"\n    input { $cityId, $name? }\n}",
    ),
    (
        "deletes",
        OPERATION,
        "Attaches the operation as the delete of one resource instance. The input binds every identifier; implies `idempotent`.",
        "operation DeleteCity deletes City {\n    http DELETE \"/cities/{cityId}\" -> 204\n    input { $cityId }\n}",
    ),
    (
        "lists",
        OPERATION,
        "Attaches the operation as the list of a resource's collection. The input binds only the parent's identifiers; implies `readonly`.",
        "operation ListCities lists City {\n    http GET \"/cities\"\n}",
    ),
    (
        "on",
        OPERATION,
        "Attaches any other operation to a resource: to one instance, binding every identifier, or with `collection` to the collection, binding only the parent's.",
        "operation RevertCity on City {\n    http POST \"/cities/{cityId}/revert\"\n    input { $cityId }\n}",
    ),
    (
        "collection",
        OPERATION,
        "After `on Resource`: the operation acts on the collection, so its input binds only the parent's identifiers.",
        "operation BatchGetCities on City collection {\n    http POST \"/cities/batch-get\"\n}",
    ),
    // ---- services ----------------------------------------------------------
    (
        "version",
        SERVICE,
        "The service's API version. A release is frozen under it (`releases/<version>.json`) and generated clients send it. Every service declares one.",
        "service Weather {\n    version \"2026-10-01\"\n}",
    ),
    (
        "title",
        SERVICE,
        "A human-readable title for the service's documentation.",
        "service Weather {\n    title \"Weather\"\n}",
    ),
    (
        "protocol",
        SERVICE,
        "The service's wire protocol. `json`, Crucible JSON, is the one supported today.",
        "service Weather {\n    protocol json\n}",
    ),
    (
        "auth",
        SERVICE,
        "The service's authentication scheme. `bearer` is the one supported today; `public` on an operation opts out.",
        "service Weather {\n    auth bearer\n}",
    ),
    (
        "operations",
        SERVICE,
        "Operations the service serves directly. Operations attached to its resources come with them.",
        "service Weather {\n    operations [Ping, Status]\n}",
    ),
    (
        "notFound",
        SERVICE,
        "The error the service answers a missing resource with: one of the package's own errors, with status 404. The compiler adds it, instead of `crucible.errors#NotFoundException`, to every operation with a `requires`, `filters` or `by handler` clause (except one whose every clause is on a root resource with `*`, which hides nothing) and to every `requires pending` operation. A hidden resource is answered with it too, built by the service's `not_found_factory`, so the two read alike.",
        "service Weather {
    notFound CityNotFound
}",
    ),
    (
        "resources",
        SERVICE,
        "The root resources the service serves. The operations attached to them, and to their child resources, are part of the service.",
        "service Weather {\n    resources [City]\n}",
    ),
    // ---- structures and members --------------------------------------------
    (
        "for",
        &["structure"],
        "Makes the structure a view of a resource: `$name` members bind its identifiers and properties, with their types and docs. An output member of the view's type, or a list or map of it, carries those bindings.",
        "structure CityData for City {\n    $cityId as id\n    $name\n}",
    ),
    (
        "for",
        &["except"],
        "The shape or operation the exception applies to.",
        "except Documented for Ping because \"a health check\"",
    ),
    (
        "include",
        ANYWHERE,
        "Copies another structure's members here, with their docs and annotations; further members may follow.",
        "structure CityError {\n    include RequestIdMixin\n    message: string\n}",
    ),
    (
        "as",
        MEMBERS,
        "Binds a resource identifier or property under another member name.",
        "structure CityData for City {\n    $cityId as id\n}",
    ),
    (
        "$*",
        &["structure"],
        "In a `for` view: every identifier the resource declares, under its `as` name, then every property that is not `writeonly`, in declaration order, each with the property's presence. Inherited identifiers are not included; `$name?` after `$*` makes one entry optional in this view.",
        "structure CityData for City {\n    $*\n    $population?\n}",
    ),
    // ---- errors ------------------------------------------------------------
    (
        "status",
        ERROR,
        "The HTTP status the error is sent with: 4xx for a client fault, 5xx for a server fault.",
        "error CityNotFound {\n    status 404\n}",
    ),
    (
        "retryable",
        ERROR,
        "Clients may retry a request that failed with this error. `retryable throttling` also marks it as throttling, so clients back off.",
        "error TooManyRequests {\n    status 429\n    retryable throttling\n}",
    ),
    (
        "throttling",
        ERROR,
        "After `retryable`: the error means the caller is being throttled, so clients back off before retrying.",
        "error TooManyRequests {\n    status 429\n    retryable throttling\n}",
    ),
    // ---- metadata ----------------------------------------------------------
    (
        "on",
        METADATA,
        "What the key applies to: `service`, `resource`, `operation`, `structure`, `union`, `error`, `enum`, `scalar`, `list`, `map`, `member` or `enum value`.",
        "metadata owner on operation, member: string",
    ),
    (
        "enum",
        METADATA,
        "`: enum { .. }` makes the key's value one of the listed values. `enum` and `enum value` are also targets.",
        "metadata tier on operation: enum { GOLD, SILVER }",
    ),
    (
        "conflicts",
        METADATA,
        "Keys, built-in annotations or modifiers this key cannot be combined with.",
        "metadata auditField on member conflicts sensitive",
    ),
    (
        "retention",
        METADATA,
        "`retention source` keeps the key for the compiler: it is checked and never written to the IR. `runtime`, the default, reaches generated code.",
        "metadata review on operation: string retention source",
    ),
    // ---- migrations --------------------------------------------------------
    (
        "from",
        MIGRATION,
        "In a migration's header, the release it serves from the current model; in `move`, the operation's old method and URI.",
        "migration \"2026-11-01\" from \"2026-10-01\" {\n    move GetCity from GET \"/city/{cityId}\"\n}",
    ),
    (
        "rename",
        MIGRATION,
        "Declares a member renamed since the previous release: its old path, then its new name.",
        "migration \"2026-11-01\" from \"2026-10-01\" {\n    rename CityData.label -> name\n}",
    ),
    (
        "require",
        MIGRATION,
        "Declares an optional input member made required. Old requests without it get the `default`, or a hook supplies it.",
        "migration \"2026-11-01\" from \"2026-10-01\" {\n    require CreateCity.input.region default EU\n}",
    ),
    (
        "default",
        MIGRATION,
        "The value old requests get for a member that became required.",
        "migration \"2026-11-01\" from \"2026-10-01\" {\n    require CreateCity.input.region default EU\n}",
    ),
    (
        "add",
        MIGRATION,
        "Declares a new enum value or error and what old versions see instead (`downgrade`).",
        "migration \"2026-11-01\" from \"2026-10-01\" {\n    add CityStatus.RETIRED downgrade CLOSED\n}",
    ),
    (
        "downgrade",
        MIGRATION,
        "What old versions see instead of the added enum value or error.",
        "migration \"2026-11-01\" from \"2026-10-01\" {\n    add CityStatus.RETIRED downgrade CLOSED\n}",
    ),
    (
        "move",
        MIGRATION,
        "Declares an operation's route changed; old versions keep the old method and URI.",
        "migration \"2026-11-01\" from \"2026-10-01\" {\n    move GetCity from GET \"/city/{cityId}\"\n}",
    ),
    (
        "reshape",
        MIGRATION,
        "Declares members restructured; a hook converts between the old and new shapes.",
        "migration \"2026-11-01\" from \"2026-10-01\" {\n    reshape CityData { latitude, longitude } -> location by hook\n}",
    ),
    (
        "remove",
        MIGRATION,
        "`remove operation Name`: the operation is gone, and old versions keep it through a version-scoped handler.",
        "migration \"2026-11-01\" from \"2026-10-01\" {\n    remove operation GetWeather\n}",
    ),
    (
        "behavior",
        MIGRATION,
        "Declares a behaviour change with no shape difference; the handler branches on a generated version constant.",
        "migration \"2026-11-01\" from \"2026-10-01\" {\n    behavior ListCities.excludesClosed\n}",
    ),
    (
        "sunset",
        MIGRATION,
        "The date the previous release stops being supported; callers at it get a `Sunset` header.",
        "migration \"2026-11-01\" from \"2026-10-01\" {\n    sunset \"2028-11-01\"\n}",
    ),
    (
        "retype",
        MIGRATION,
        "Declares a member's type changed, converted by a hook.",
        "migration \"2026-11-01\" from \"2026-10-01\" {\n    retype CityData.population -> Count by hook\n}",
    ),
    (
        "by",
        &["migration", "conversion"],
        "`by hook` names a function the service implements to convert between versions; `by async hook`, one that may do I/O.",
        "migration \"2026-11-01\" from \"2026-10-01\" {\n    retype CityData.population -> Count by hook\n}",
    ),
    (
        "hook",
        &["migration", "conversion"],
        "A conversion the service implements in code, generated as a typed function the build requires.",
        "migration \"2026-11-01\" from \"2026-10-01\" {\n    retype CityData.population -> Count by hook\n}",
    ),
    (
        "async",
        &["migration", "conversion"],
        "`by async hook`: the conversion may do I/O, such as an extra read, which the source makes visible.",
        "migration \"2026-11-01\" from \"2026-10-01\" {\n    retype CityData.population -> Count by async hook\n}",
    ),
    // ---- constraints -------------------------------------------------------
    (
        "length",
        ANYWHERE,
        "Bounds a length, inclusive at both ends: the characters of a string, the bytes of a blob, the items of a list or the entries of a map. Either end may be left open (`1..`, `..64`).",
        "string CityName { length 1..64 }",
    ),
    (
        "range",
        ANYWHERE,
        "Bounds a number, inclusive at both ends. Either end may be left open (`0..`, `..5`).",
        "integer Population { range 0.. }",
    ),
    (
        "pattern",
        ANYWHERE,
        "A regular expression (ECMA-262) a string must match.",
        "string CityCode { pattern \"^[A-Z]{3}$\" }",
    ),
    (
        "unique",
        ANYWHERE,
        "The list's items must be distinct.",
        "list Tags: Tag { unique }",
    ),
    (
        "format",
        ANYWHERE,
        "A timestamp's wire format: `date-time` (RFC 3339, the default), `http-date` or `epoch-seconds`.",
        "timestamp HttpDate { format http-date }",
    ),
    (
        "mediaType",
        ANYWHERE,
        "The media type of a blob's or string's content. It belongs to the type; a member cannot set one.",
        "blob Photo { mediaType \"image/jpeg\" }",
    ),
    // ---- annotations -------------------------------------------------------
    (
        "@query",
        ANYWHERE,
        "Binds an input member to a query parameter, named after the member unless a name is given.",
        "operation ListCities {\n    input {\n        @query(\"max\") maxResults?: integer\n    }\n}",
    ),
    (
        "@header",
        ANYWHERE,
        "Binds a member of an input, output or error to the HTTP header it names.",
        "operation GetCity {\n    output {\n        @header(\"X-Request-Id\") requestId?: string\n    }\n}",
    ),
    (
        "@payload",
        ANYWHERE,
        "Makes the member the whole HTTP body, so no other member is body. At most one per message.",
        "operation PutPhoto {\n    input {\n        @payload photo: Photo\n    }\n}",
    ),
    (
        "@token",
        ANYWHERE,
        "Marks the input member a client fills with a fresh idempotency token: at most one per input, an optional string. It makes the operation idempotent.",
        "operation CreateCity {\n    input {\n        @token clientToken?: string\n    }\n}",
    ),
    (
        "@since",
        ANYWHERE,
        "The version that added it.",
        "@since(\"2026-10-01\")\nstructure CityData {}",
    ),
    (
        "@deprecated",
        ANYWHERE,
        "Deprecated: `message` says why and what to use instead, `since` when.",
        "@deprecated(message: \"Use CityData.\", since: \"2026-09\")\nstructure OldCity {}",
    ),
    (
        "@unstable",
        ANYWHERE,
        "Not stable yet: it may still change.",
        "@unstable\nstructure CityPreview {}",
    ),
    (
        "@externalDocs",
        ANYWHERE,
        "Links to further documentation, as name and URL pairs.",
        "@externalDocs(\"Guide\" = \"https://example.com/guide\")\nstructure CityData {}",
    ),
    (
        "@permissionOnly",
        ANYWHERE,
        "A permission no operation requires: the service's policy checks it (a role bundle, a delegation rule). Docs list it in its own table.",
        "resource Inbox {\n    @permissionOnly\n    permission export = \"inbox:export\"\n}",
    ),
    (
        "@notGrantable",
        ANYWHERE,
        "A permission the service's policy confers whole, such as an administrator bundle: never offered for composition in a grant picker.",
        "resource Workspace {\n    @notGrantable\n    permission admin = \"workspace:admin\"\n}",
    ),
    (
        "@aliasOf",
        ANYWHERE,
        "Keeps a renamed permission's old value as a deprecated alias of the named primary: a grant stored under it still satisfies a check of the primary. Requirements name the primary.",
        "resource Inbox {\n    @deprecated @aliasOf(mailSend)\n    permission emailSend = \"email:send\"\n}",
    ),
    (
        "@authorizationName",
        ANYWHERE,
        "The resource's name in grants and audit records, instead of its shape name in lowerCamelCase. Set it before renaming a shape whose permissions are released: the name is stored data.",
        "@authorizationName(\"inbox\")\nresource Mailbox {\n    id inboxId: string\n}",
    ),
    (
        "@permissionTypes",
        ANYWHERE,
        "Checks the hand-written wire enum of resource types a grant names, both ways: every value is the authorization name of a resource with permissions, and every resource with grantable permissions is a value or listed in `exclude`.",
        "@permissionTypes(exclude: [Webhook])\nenum ResourceType {\n    inbox = \"inbox\"\n}",
    ),
    (
        "@mutability",
        ANYWHERE,
        "Declares when a member that binds no resource property, such as a create-time parameter, can be written and read: `full`, `create`, `createAndRead`, `read` or `write`. Infrastructure generators read it.",
        "operation CreateInbox creates Inbox {\n    input {\n        @mutability(create) localPart: string\n    }\n}",
    ),
    // ---- built-in types ----------------------------------------------------
    (
        "string",
        TYPE,
        "Text, a sequence of Unicode characters. `length`, `pattern` and `mediaType` constrain it.",
        "string CityName { length 1..64 }",
    ),
    (
        "boolean",
        TYPE,
        "`true` or `false`.",
        "structure Flags {\n    enabled: boolean = false\n}",
    ),
    (
        "byte",
        TYPE,
        "An 8-bit signed integer.",
        "byte Level { range 0..9 }",
    ),
    (
        "short",
        TYPE,
        "A 16-bit signed integer.",
        "short Year { range 1..9999 }",
    ),
    (
        "integer",
        TYPE,
        "A 32-bit signed integer.",
        "integer Population { range 0.. }",
    ),
    (
        "long",
        TYPE,
        "A 64-bit signed integer.",
        "long ByteCount { range 0.. }",
    ),
    (
        "float",
        TYPE,
        "A 32-bit IEEE 754 floating-point number.",
        "float Ratio { range 0..1 }",
    ),
    (
        "double",
        TYPE,
        "A 64-bit IEEE 754 floating-point number.",
        "double Latitude { range -90..90 }",
    ),
    (
        "bigInteger",
        TYPE,
        "An integer of any size.",
        "bigInteger Huge",
    ),
    (
        "bigDecimal",
        TYPE,
        "A decimal number of any precision.",
        "bigDecimal Amount",
    ),
    (
        "timestamp",
        TYPE,
        "A point in time. On the wire it is an RFC 3339 `date-time` unless the type or member declares `format http-date` or `format epoch-seconds`.",
        "timestamp DateTime",
    ),
    (
        "blob",
        TYPE,
        "Raw bytes. `mediaType` names their type and `length` bounds their size.",
        "blob Photo { mediaType \"image/jpeg\" }",
    ),
    (
        "document",
        TYPE,
        "An untyped JSON-like value: null, a boolean, number, string, list or object.",
        "document Settings",
    ),
];

/// One row of [`KEYWORDS`]: word, contexts, explanation, example.
type Row = (
    &'static str,
    &'static [&'static str],
    &'static str,
    &'static str,
);

/// The row for `word` in `context`.
fn keyword(word: &str, context: &str) -> Option<&'static Row> {
    KEYWORDS.iter().find(|(name, contexts, _, _)| {
        *name == word && (contexts.is_empty() || contexts.contains(&context))
    })
}

/// The hover for the identifier at `offset` in `parsed`: its byte range and
/// Markdown. `package` is every file of the package, imports included; the
/// open document comes first, so its unsaved text wins.
pub fn hover(
    parsed: &Parsed,
    text: &str,
    index: &Index,
    package: &[Parsed],
    offset: usize,
) -> Option<(u32, u32, String)> {
    let model = Model {
        files: std::iter::once(parsed).chain(package).collect(),
    };
    if let Some((start, end)) = star_at(parsed, offset as u32) {
        return model
            .star(parsed, start)
            .map(|markdown| (start, end, markdown));
    }
    let (start, end, word) = analysis::ident_at(parsed, text, offset)?;
    let highlight = analysis::highlights(parsed, index)
        .into_iter()
        .find(|highlight| highlight.start == start)?;
    let markdown = match highlight.role {
        Role::Keyword => {
            let context = context(parsed, start)?;
            keyword(&word, context).map(|row| explain(row.0, row.2, row.3))
        }
        Role::Type if highlight.modifiers & DEFAULT_LIBRARY != 0 => {
            keyword(&word, "type").map(|row| explain(row.0, row.2, row.3))
        }
        Role::Decorator => match model.item(&word) {
            Some(item) if matches!(item.decl, Decl::Metadata(_)) => Some(model.declaration(item)),
            // `@name` is an annotation: one that is not built in explains
            // nothing, rather than what the bare word means elsewhere. A
            // bare decorator is a key a metadata declaration names.
            _ if annotated(parsed, start) => {
                keyword(&format!("@{word}"), "").map(|row| explain(row.0, row.2, row.3))
            }
            _ => keyword(&word, context(parsed, start)?).map(|row| explain(row.0, row.2, row.3)),
        },
        Role::Property => model.member(parsed, start),
        Role::Type | Role::Struct | Role::Enum | Role::Class | Role::Interface | Role::Function => {
            model.item(&word).map(|item| model.declaration(item))
        }
        _ => None,
    }?;
    Some((start, end, markdown))
}

/// The byte range of the `$*` whose `$` or `*` covers `offset`.
fn star_at(parsed: &Parsed, offset: u32) -> Option<(u32, u32)> {
    let significant: Vec<_> = parsed
        .tokens
        .iter()
        .filter(|token| !token.kind.is_trivia())
        .collect();
    significant.windows(2).find_map(|pair| {
        let (dollar, star) = (pair[0], pair[1]);
        (dollar.kind == TokenKind::Dollar
            && star.kind == TokenKind::Star
            && dollar.span.start <= offset
            && offset <= star.span.end)
            .then_some((dollar.span.start, star.span.end))
    })
}

/// Whether the identifier starting at `start` follows an `@`.
fn annotated(parsed: &Parsed, start: u32) -> bool {
    let at = parsed
        .tokens
        .partition_point(|token| token.span.start < start);
    at > 0 && parsed.tokens[at - 1].kind == TokenKind::At
}

/// A keyword's hover: the word, what it means, and an example.
fn explain(word: &str, about: &str, example: &str) -> String {
    format!("**{}**\n\n{about}\n\n{}", code(word), fence(example))
}

/// Where the keyword starting at `start` is: `declaration` for the word
/// that starts a declaration, `example` inside an operation's example, and
/// otherwise the kind of declaration around it.
fn context(parsed: &Parsed, start: u32) -> Option<&'static str> {
    let item = parsed
        .file
        .items
        .iter()
        .find(|item| item.span.start <= start && start < item.span.end)?;
    // Annotations, then modifiers, then the declaring word.
    let after = item
        .modifiers
        .last()
        .map(|modifier| modifier.span.end)
        .or_else(|| {
            item.annotations
                .last()
                .map(|annotation| annotation.span.end)
        })
        .unwrap_or(item.span.start);
    let first = parsed.tokens[parsed.tokens.partition_point(|t| t.span.start < after)..]
        .iter()
        .find(|token| token.kind == TokenKind::Ident);
    if first.is_some_and(|token| token.span.start == start) {
        return Some("declaration");
    }
    let in_example = |entries: &[OpEntry]| {
        entries.iter().any(|entry| {
            matches!(entry, OpEntry::Example(example)
                if example.span.start <= start && start < example.span.end)
        })
    };
    Some(match &item.decl {
        Decl::Service(_) => "service",
        Decl::Protocol(_) => "protocol",
        Decl::Resource(_) => "resource",
        Decl::Operation(operation) if in_example(&operation.entries) => "example",
        Decl::Operation(_) => "operation",
        Decl::Template(_) => "template",
        Decl::Structure(_) => "structure",
        Decl::Union(_) => "union",
        Decl::Error(_) => "error",
        Decl::Enum(_) => "enum",
        Decl::Scalar(_) => "scalar",
        Decl::List(_) => "list",
        Decl::Map(_) => "map",
        Decl::Metadata(_) => "metadata",
        Decl::Rule(_) => "rule",
        Decl::Except(_) => "except",
        Decl::Migration(_) => "migration",
        Decl::Conversion(_) => "conversion",
    })
}

/// The files a hover can draw declarations from, the open one first.
struct Model<'a> {
    files: Vec<&'a Parsed>,
}

/// A member found by its name, with what holds it.
struct Found<'a> {
    member: &'a Member,
    /// `structure \`City\``, `the input of \`GetCity\``.
    container: String,
    /// The resource its `$members` draw from.
    resource: Option<&'a str>,
    union: bool,
    /// A `for` view's or an output's member: a representation of the
    /// resource, where a property's own `?` applies and an identifier takes
    /// its `as` spelling (decisions 34e, 34f).
    representation: bool,
    /// A member of a `structure .. for` view, the only place `$*` expands.
    view: bool,
}

impl<'a> Model<'a> {
    /// The declaration named `name`.
    fn item(&self, name: &str) -> Option<&'a Item> {
        self.files
            .iter()
            .flat_map(|parsed| &parsed.file.items)
            .find(|item| {
                analysis::describe(&item.decl).is_some_and(|(_, _, ident)| ident.name == name)
            })
    }

    /// A declaration's hover: its source form, what it is, its documentation
    /// and the types it is made of.
    fn declaration(&self, item: &Item) -> String {
        let mut out = vec![fence(&render_item(item))];
        out.extend(self.summary(item));
        if let Some(doc) = &item.doc {
            out.push(documentation(&doc.text));
        }
        match &item.decl {
            Decl::List(list) => out.extend(self.related("Items", &list.member.name)),
            Decl::Map(map) => {
                out.extend(self.related("Keys", &map.key.name));
                out.extend(self.related("Values", &map.value.name));
            }
            _ => {}
        }
        join(out)
    }

    /// Plain sentences about a declaration that its source form leaves to
    /// the reader: what its items are, what its constraints allow.
    fn summary(&self, item: &Item) -> Vec<String> {
        let mut out = Vec::new();
        match &item.decl {
            Decl::List(list) => {
                out.push(format!(
                    "A list of {} ({}).",
                    code(&list.member.name),
                    self.kind_of(&list.member.name)
                ));
                out.extend(constraint_sentence(
                    &[],
                    &list.constraints,
                    None,
                    Some(ITEMS),
                ));
            }
            Decl::Map(map) => {
                out.push(format!(
                    "A map from {} ({}) to {} ({}).",
                    code(&map.key.name),
                    self.kind_of(&map.key.name),
                    code(&map.value.name),
                    self.kind_of(&map.value.name)
                ));
                out.extend(constraint_sentence(
                    &[],
                    &map.constraints,
                    None,
                    Some(ENTRIES),
                ));
            }
            Decl::Scalar(scalar) => out.extend(constraint_sentence(
                &[],
                &scalar.constraints,
                None,
                unit_of_kind(&scalar.kind.name),
            )),
            Decl::Enum(enumeration) => {
                let base = enumeration
                    .base
                    .as_ref()
                    .map_or("string", |base| base.name.as_str());
                let count = enumeration.values.len();
                out.push(format!(
                    "A{} {base} enum with {count} value{}.",
                    if base == "integer" { "n" } else { "" },
                    if count == 1 { "" } else { "s" }
                ));
            }
            Decl::Structure(structure) => {
                if let Some(resource) = &structure.resource {
                    out.push(format!(
                        "A view of resource {}: its `$members` bind the resource's identifiers and properties.",
                        code(&resource.name)
                    ));
                }
            }
            Decl::Error(error) => {
                let mut facts = Vec::new();
                for entry in &error.entries {
                    match entry {
                        ErrorEntry::Status(status) => {
                            let fault = match status.text.chars().next() {
                                Some('4') => ", a client fault",
                                Some('5') => ", a server fault",
                                _ => "",
                            };
                            facts.push(format!("Sent with status {}{fault}.", status.text));
                        }
                        ErrorEntry::Retryable { throttling, .. } => facts.push(
                            if *throttling {
                                "Retryable; it means throttling, so clients back off first."
                            } else {
                                "Retryable."
                            }
                            .to_owned(),
                        ),
                        ErrorEntry::Member(_) => {}
                    }
                }
                if !facts.is_empty() {
                    out.push(facts.join(" "));
                }
            }
            Decl::Operation(operation) => {
                if let Some(attachment) = &operation.attachment {
                    out.push(format!(
                        "Attached to resource {} with the {} role{}.",
                        code(&attachment.resource.name),
                        code(&attachment.role.name),
                        if attachment.collection {
                            ", on its collection"
                        } else {
                            ""
                        }
                    ));
                }
            }
            _ => {}
        }
        out
    }

    /// A type a declaration or member is made of, shown in full when the
    /// package declares it.
    fn related(&self, label: &str, name: &str) -> Vec<String> {
        let Some(item) = self.item(name) else {
            return Vec::new();
        };
        let mut out = vec![
            format!(
                "**{label}:** {}, {}",
                code(name),
                with_article(&self.kind_of(name))
            ),
            fence(&render_item(item)),
        ];
        out.extend(self.summary(item));
        out
    }

    /// What kind of type `name` is: `structure`, `string`, `built-in` ...
    fn kind_of(&self, name: &str) -> String {
        if BUILTIN_TYPES.contains(&name) {
            return "built-in".to_owned();
        }
        match self.item(name) {
            Some(item) => {
                analysis::describe(&item.decl).map_or_else(String::new, |(keyword, _, _)| keyword)
            }
            None => "not declared".to_owned(),
        }
    }

    /// The constraints declared on a type, if it is one that has any.
    fn constraints_of(&self, name: &str) -> &'a [Constraint] {
        match self.item(name).map(|item| &item.decl) {
            Some(Decl::Scalar(scalar)) => &scalar.constraints,
            Some(Decl::List(list)) => &list.constraints,
            Some(Decl::Map(map)) => &map.constraints,
            _ => &[],
        }
    }

    /// What a `length` on `name` counts.
    fn unit_of(&self, name: &str) -> Option<Unit> {
        if BUILTIN_TYPES.contains(&name) {
            return unit_of_kind(name);
        }
        match &self.item(name)?.decl {
            Decl::Scalar(scalar) => unit_of_kind(&scalar.kind.name),
            Decl::List(_) => Some(ITEMS),
            Decl::Map(_) => Some(ENTRIES),
            _ => None,
        }
    }

    /// The hover for the member or resource entry whose name starts at
    /// `start` in `parsed`.
    fn member(&self, parsed: &'a Parsed, start: u32) -> Option<String> {
        for item in &parsed.file.items {
            if let Decl::Resource(resource) = &item.decl
                && let Some(entry) = resource
                    .entries
                    .iter()
                    .find(|entry| entry.name.span.start == start)
            {
                return Some(self.resource_entry(&resource.name.name, entry));
            }
        }
        let found = find_member(parsed, start)?;
        let member = found.member;
        let mut out = vec![fence(&render_member(member))];
        match &member.kind {
            MemberKind::Field {
                optional,
                ty,
                default,
                ..
            } => {
                out.push(if found.union {
                    match ty {
                        Some(_) => format!("A member of {}.", found.container),
                        None => format!("A member of {}; it carries nothing.", found.container),
                    }
                } else if let Some(default) = default {
                    format!(
                        "A member of {}, defaulting to {} when omitted.",
                        found.container,
                        code(&render_value(default))
                    )
                } else if *optional {
                    format!("An optional member of {}.", found.container)
                } else {
                    format!("A required member of {}.", found.container)
                });
                if let Some(ty) = ty {
                    out.extend(self.constraints(&ty.constraints, &ty.name.name));
                }
                if let Some(doc) = &member.doc {
                    out.push(documentation(&doc.text));
                }
                if let Some(ty) = ty {
                    out.extend(self.type_sections(&ty.name.name));
                }
            }
            MemberKind::Elided {
                name,
                alias,
                optional,
            } => {
                let resources = insight::resources(&self.files);
                let entry = found
                    .resource
                    .and_then(|resource| insight::resource_entry(&resources, resource, &name.name));
                // In a representation a property's own `?` applies too.
                let loosened = found.representation
                    && entry.is_some_and(|(_, entry)| {
                        entry.kind == ResourceEntryKind::Property && entry.optional
                    });
                let presence = if *optional || loosened {
                    "An optional"
                } else {
                    "A required"
                };
                match entry {
                    Some((owner, entry)) => {
                        let keyword = match mutability(entry) {
                            "" => String::new(),
                            keyword => format!("{} ", code(keyword)),
                        };
                        out.push(format!(
                            "{presence} member of {}, bound to {keyword}{} {} of resource {}: {}.",
                            found.container,
                            entry_kind(entry),
                            code(&name.name),
                            code(&owner.name.name),
                            code(&render_type(&entry.ty))
                        ));
                        let own = found.resource == Some(owner.name.name.as_str());
                        if let Some(spelled) = entry.alias.as_ref().filter(|_| {
                            found.representation
                                && own
                                && alias.is_none()
                                && entry.kind == ResourceEntryKind::Id
                        }) {
                            out.push(format!(
                                "Named {} here: {}'s spelling of the identifier in its views and outputs.",
                                code(&spelled.name),
                                code(&owner.name.name)
                            ));
                        }
                        out.extend(self.constraints(&entry.ty.constraints, &entry.ty.name.name));
                        if let Some(doc) = member.doc.as_ref().or(entry.doc.as_ref()) {
                            out.push(documentation(&doc.text));
                        }
                        out.extend(self.type_sections(&entry.ty.name.name));
                    }
                    None => {
                        out.push(format!(
                            "{presence} member of {}, bound to {} of its resource.",
                            found.container,
                            code(&name.name)
                        ));
                        if let Some(doc) = &member.doc {
                            out.push(documentation(&doc.text));
                        }
                    }
                }
            }
            MemberKind::Include(_) | MemberKind::All(_) => return None,
        }
        Some(join(out))
    }

    /// `$*` at `start`: what it means, and what it includes here.
    fn star(&self, parsed: &'a Parsed, start: u32) -> Option<String> {
        let found = find_member(parsed, start)?;
        let row = keyword("$*", "structure")?;
        let mut out = vec![explain(row.0, row.2, row.3)];
        let resources = insight::resources(&self.files);
        match found
            .resource
            .filter(|_| found.view)
            .and_then(|name| resources.get(name))
        {
            Some(resource) => {
                let ids = resource
                    .entries
                    .iter()
                    .filter(|e| e.kind == ResourceEntryKind::Id);
                let properties = resource.entries.iter().filter(|e| {
                    e.kind == ResourceEntryKind::Property
                        && !e.modifiers.iter().any(|m| m.name == "writeonly")
                });
                let lines: Vec<String> = ids
                    .chain(properties)
                    .filter(|e| !e.name.is_missing())
                    .map(|e| {
                        let name = e.alias.as_ref().unwrap_or(&e.name);
                        let optional = if e.optional { "?" } else { "" };
                        format!("{}{optional}: {}", name.name, render_type(&e.ty))
                    })
                    .collect();
                if lines.is_empty() {
                    out.push(format!(
                        "Here it includes nothing: resource {} declares no identifier or readable property.",
                        code(&resource.name.name)
                    ));
                } else {
                    out.push(format!(
                        "Here it includes, from resource {}:",
                        code(&resource.name.name)
                    ));
                    out.push(fence(&lines.join("\n")));
                }
            }
            None => out.push(format!(
                "Here it is in {}, which is not a `for` view, so it includes nothing.",
                found.container
            )),
        }
        Some(join(out))
    }

    fn resource_entry(&self, resource: &str, entry: &ResourceEntry) -> String {
        let mut out = vec![fence(&render_entry(entry))];
        let what = match entry.kind {
            ResourceEntryKind::Id => format!("An identifier of resource {}.", code(resource)),
            ResourceEntryKind::Property => format!(
                "{} property of resource {}{}.",
                if entry.optional {
                    "An optional"
                } else {
                    "A required"
                },
                code(resource),
                match mutability(entry) {
                    "readonly" => ", assigned by the service",
                    "immutable" => ", set by `creates` or `puts` and fixed afterwards",
                    "writeonly" => ", written by inputs and never returned",
                    "immutable writeonly" => ", set once at creation and never returned",
                    _ => ", set at creation and changed by `updates`",
                }
            ),
        };
        out.push(what);
        if let Some(alias) = entry
            .alias
            .as_ref()
            .filter(|_| entry.kind == ResourceEntryKind::Id)
        {
            out.push(format!(
                "Shown as {} in {}'s own views and outputs; inputs bind it as {}.",
                code(&alias.name),
                code(resource),
                code(&entry.name.name)
            ));
        }
        out.extend(self.constraints(&entry.ty.constraints, &entry.ty.name.name));
        if let Some(doc) = &entry.doc {
            out.push(documentation(&doc.text));
        }
        out.extend(self.type_sections(&entry.ty.name.name));
        join(out)
    }

    /// The constraints that apply to a member of type `ty`: the member's
    /// own laid over its type's, each replacing the type's of its name.
    fn constraints(&self, own: &[Constraint], ty: &str) -> Option<String> {
        constraint_sentence(own, self.constraints_of(ty), Some(ty), self.unit_of(ty))
    }

    /// A member's type in full, and the item, key and value types of a
    /// list or map.
    fn type_sections(&self, name: &str) -> Vec<String> {
        let mut out = self.related("Type", name);
        match self.item(name).map(|item| &item.decl) {
            Some(Decl::List(list)) => out.extend(self.related("Items", &list.member.name)),
            Some(Decl::Map(map)) => {
                out.extend(self.related("Keys", &map.key.name));
                out.extend(self.related("Values", &map.value.name));
            }
            _ => {}
        }
        out
    }
}

/// A property's mutability keywords as written, or `` for read-write.
fn mutability(entry: &ResourceEntry) -> &'static str {
    let has = |word: &str| entry.modifiers.iter().any(|m| m.name == word);
    match (has("readonly"), has("immutable"), has("writeonly")) {
        (true, _, _) => "readonly",
        (false, true, true) => "immutable writeonly",
        (false, true, false) => "immutable",
        (false, false, true) => "writeonly",
        (false, false, false) => "",
    }
}

/// `identifier` or `property`.
fn entry_kind(entry: &ResourceEntry) -> &'static str {
    match entry.kind {
        ResourceEntryKind::Id => "identifier",
        ResourceEntryKind::Property => "property",
    }
}

/// `a structure`, `an enum`, `built-in`.
fn with_article(kind: &str) -> String {
    match kind {
        "built-in" | "not declared" => kind.to_owned(),
        // `a union`: only these vowels take `an` among the kinds.
        _ if kind.starts_with(['a', 'e', 'i', 'o']) => format!("an {kind}"),
        _ => format!("a {kind}"),
    }
}

/// The member whose name starts at `start`, in any member list of `parsed`.
fn find_member(parsed: &Parsed, start: u32) -> Option<Found<'_>> {
    let at = |member: &Member| match &member.kind {
        MemberKind::Field { name, .. } => name.span.start == start,
        MemberKind::Elided { name, alias, .. } => {
            name.span.start == start || alias.as_ref().is_some_and(|a| a.span.start == start)
        }
        MemberKind::All(span) => span.start == start,
        MemberKind::Include(_) => false,
    };
    fn bodies<'p>(
        entries: &'p [OpEntry],
        owner: String,
        at: &dyn Fn(&Member) -> bool,
    ) -> Option<(&'p Member, String, BodyKind)> {
        entries.iter().find_map(|entry| match entry {
            OpEntry::Body(Body { kind, members, .. }) => members.iter().find(|m| at(m)).map(|m| {
                let part = match kind {
                    BodyKind::Input => "input",
                    BodyKind::Output => "output",
                };
                (m, format!("the {part} of {owner}"), *kind)
            }),
            _ => None,
        })
    }
    for item in &parsed.file.items {
        let found = match &item.decl {
            Decl::Structure(structure) => structure.members.iter().find(|m| at(m)).map(|m| Found {
                member: m,
                container: format!("structure {}", code(&structure.name.name)),
                resource: structure.resource.as_ref().map(|r| r.name.as_str()),
                union: false,
                representation: structure.resource.is_some(),
                view: structure.resource.is_some(),
            }),
            Decl::Union(union) => union.members.iter().find(|m| at(m)).map(|m| Found {
                member: m,
                container: format!("union {}", code(&union.name.name)),
                resource: None,
                union: true,
                representation: false,
                view: false,
            }),
            Decl::Error(error) => error.entries.iter().find_map(|entry| match entry {
                ErrorEntry::Member(m) if at(m) => Some(Found {
                    member: m,
                    container: format!("error {}", code(&error.name.name)),
                    resource: None,
                    union: false,
                    representation: false,
                    view: false,
                }),
                _ => None,
            }),
            Decl::Operation(operation) => {
                bodies(&operation.entries, code(&operation.name.name), &at).map(
                    |(m, container, kind)| Found {
                        member: m,
                        container,
                        resource: operation
                            .attachment
                            .as_ref()
                            .map(|attachment| attachment.resource.name.as_str()),
                        union: false,
                        representation: kind == BodyKind::Output,
                        view: false,
                    },
                )
            }
            Decl::Template(template) => bodies(
                &template.entries,
                format!("template {}", code(&template.role.name)),
                &at,
            )
            .map(|(m, container, _)| Found {
                member: m,
                container,
                resource: None,
                union: false,
                representation: false,
                view: false,
            }),
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

/// What a `length` counts, singular and plural.
type Unit = (&'static str, &'static str);
const ITEMS: Unit = ("item", "items");
const ENTRIES: Unit = ("entry", "entries");

fn unit_of_kind(kind: &str) -> Option<Unit> {
    match kind {
        "string" => Some(("character", "characters")),
        "blob" => Some(("byte", "bytes")),
        _ => None,
    }
}

/// One sentence listing what constraints allow. `own` are a member's,
/// `inherited` its type's (`ty`); a member's constraint replaces its type's
/// of the same name, as the IR's effective constraints do.
fn constraint_sentence(
    own: &[Constraint],
    inherited: &[Constraint],
    ty: Option<&str>,
    unit: Option<Unit>,
) -> Option<String> {
    let both = !own.is_empty() && !inherited.is_empty();
    let from = |text: String| match ty {
        Some(ty) if both => format!("{text} (from {})", code(ty)),
        _ => text,
    };
    let mut parts = Vec::new();
    for constraint in inherited {
        match own.iter().find(|c| c.name.name == constraint.name.name) {
            Some(replacing) => parts.push(format!(
                "{} (the member's, replacing {}{})",
                describe(replacing, unit),
                code(&render_constraint(constraint)),
                ty.map(|ty| format!(" from {}", code(ty)))
                    .unwrap_or_default()
            )),
            None => parts.push(from(describe(constraint, unit))),
        }
    }
    for constraint in own {
        if !inherited
            .iter()
            .any(|c| c.name.name == constraint.name.name)
        {
            parts.push(if both {
                format!("{} (the member's)", describe(constraint, unit))
            } else {
                describe(constraint, unit)
            });
        }
    }
    if parts.is_empty() {
        return None;
    }
    let lead = match ty {
        Some(ty) if own.is_empty() => format!("Constraints from {}", code(ty)),
        _ => "Constraints".to_owned(),
    };
    Some(format!("{lead}: {}.", parts.join("; ")))
}

/// A constraint in words: `1 to 64 characters`, `matches ^[a-z]+$`.
fn describe(constraint: &Constraint, unit: Option<Unit>) -> String {
    let name = constraint.name.name.as_str();
    match (name, &constraint.value) {
        ("length", ConstraintValue::Range { min, max }) => match unit {
            Some(unit) => bounds(min.as_ref(), max.as_ref(), Some(unit)),
            None => format!("length {}", bounds(min.as_ref(), max.as_ref(), None)),
        },
        ("range", ConstraintValue::Range { min, max }) => {
            let text = bounds(min.as_ref(), max.as_ref(), None);
            if min.is_some() && max.is_some() && !text.starts_with("exactly") {
                format!("{text}, inclusive")
            } else {
                text
            }
        }
        ("pattern", ConstraintValue::Value(Value::Str(pattern))) => {
            format!("matches {}", code(&pattern.value))
        }
        ("unique", ConstraintValue::Flag) => "no duplicate items".to_owned(),
        ("format", ConstraintValue::Value(Value::Ident(format))) => {
            format!("written as {}", code(&format.name))
        }
        ("mediaType", ConstraintValue::Value(Value::Str(media))) => {
            format!("media type {}", code(&media.value))
        }
        _ => code(&render_constraint(constraint)),
    }
}

fn bounds(min: Option<&NumLit>, max: Option<&NumLit>, unit: Option<Unit>) -> String {
    // Singular only for one exact count: `at most 1 item`, `0 to 1 items`.
    let counted = |number: &str, single: bool| match unit {
        Some((one, _)) if single && number == "1" => format!("{number} {one}"),
        Some((_, many)) => format!("{number} {many}"),
        None => number.to_owned(),
    };
    match (min, max) {
        (Some(min), Some(max)) if min.text == max.text => {
            format!("exactly {}", counted(&min.text, true))
        }
        (Some(min), Some(max)) => format!("{} to {}", min.text, counted(&max.text, false)),
        (Some(min), None) => format!("at least {}", counted(&min.text, true)),
        (None, Some(max)) => format!("at most {}", counted(&max.text, true)),
        (None, None) => "any".to_owned(),
    }
}

// ---- source forms ----------------------------------------------------------

/// A declaration as source, without documentation or examples: what it is,
/// at a glance.
fn render_item(item: &Item) -> String {
    let mut out = String::new();
    for annotation in &item.annotations {
        out.push_str(&render_annotation(annotation));
        out.push('\n');
    }
    for modifier in &item.modifiers {
        out.push_str(&modifier.name);
        out.push(' ');
    }
    out.push_str(&match &item.decl {
        Decl::Service(block) => braces(
            &format!("service {}", block.name.name),
            block.clauses.iter().map(render_clause).collect(),
        ),
        Decl::Protocol(block) => braces(
            &format!("protocol {}", block.name.name),
            block.clauses.iter().map(render_clause).collect(),
        ),
        Decl::Resource(resource) => {
            let mut head = format!("resource {}", resource.name.name);
            if let Some(parent) = &resource.parent {
                head.push_str(&format!(" in {}", parent.name));
            }
            braces(
                &head,
                resource
                    .entries
                    .iter()
                    .map(render_entry)
                    .chain(resource.permissions.iter().map(render_permission))
                    .collect(),
            )
        }
        Decl::Operation(operation) => {
            let mut head = format!("operation {}", operation.name.name);
            if let Some(attachment) = &operation.attachment {
                head.push_str(&format!(
                    " {} {}",
                    attachment.role.name, attachment.resource.name
                ));
                if attachment.collection {
                    head.push_str(" collection");
                }
            }
            braces(&head, render_entries(&operation.entries))
        }
        Decl::Template(template) => braces(
            &format!("template {}", template.role.name),
            render_entries(&template.entries),
        ),
        Decl::Structure(structure) => {
            let mut head = format!("structure {}", structure.name.name);
            if let Some(resource) = &structure.resource {
                head.push_str(&format!(" for {}", resource.name));
            }
            braces(&head, structure.members.iter().map(render_member).collect())
        }
        Decl::Union(union) => braces(
            &format!("union {}", union.name.name),
            union.members.iter().map(render_member).collect(),
        ),
        Decl::Error(error) => braces(
            &format!("error {}", error.name.name),
            error
                .entries
                .iter()
                .map(|entry| match entry {
                    ErrorEntry::Status(status) => format!("status {}", status.text),
                    ErrorEntry::Retryable { throttling, .. } => if *throttling {
                        "retryable throttling"
                    } else {
                        "retryable"
                    }
                    .to_owned(),
                    ErrorEntry::Member(member) => render_member(member),
                })
                .collect(),
        ),
        Decl::Enum(enumeration) => {
            let mut head = format!("enum {}", enumeration.name.name);
            if let Some(base) = &enumeration.base {
                head.push_str(&format!(": {}", base.name));
            }
            braces(
                &head,
                enumeration.values.iter().map(render_enum_value).collect(),
            )
        }
        Decl::Scalar(scalar) => format!(
            "{} {}{}",
            scalar.kind.name,
            scalar.name.name,
            render_constraints(&scalar.constraints)
        ),
        Decl::List(list) => format!(
            "list {}: {}{}",
            list.name.name,
            list.member.name,
            render_constraints(&list.constraints)
        ),
        Decl::Map(map) => format!(
            "map {}: {} -> {}{}",
            map.name.name,
            map.key.name,
            map.value.name,
            render_constraints(&map.constraints)
        ),
        Decl::Metadata(metadata) => {
            let targets: Vec<String> = metadata
                .targets
                .iter()
                .map(|words| names(words, " "))
                .collect();
            let mut out = format!("metadata {} on {}", metadata.name.name, targets.join(", "));
            match &metadata.ty {
                Some(MetaType::Type(ty)) => out.push_str(&format!(": {}", render_type(ty))),
                Some(MetaType::Enum(values)) => out.push_str(&format!(
                    ": enum {{ {} }}",
                    values
                        .iter()
                        .map(render_enum_value)
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
                None => {}
            }
            if !metadata.conflicts.is_empty() {
                out.push_str(&format!(" conflicts {}", names(&metadata.conflicts, ", ")));
            }
            if let Some(retention) = &metadata.retention {
                out.push_str(&format!(" retention {}", retention.name));
            }
            out
        }
        Decl::Rule(rule) => {
            let mut out = format!("rule {}", rule.name.name);
            for flag in &rule.flags {
                out.push(' ');
                out.push_str(&flag.name);
            }
            out
        }
        Decl::Except(except) => format!(
            "except {} for {} because {}",
            except.rule.name,
            except.target.name,
            quote(&except.reason.value)
        ),
        Decl::Migration(migration) => format!(
            "migration {} from {}",
            quote(&migration.version.value),
            quote(&migration.from.value)
        ),
        Decl::Conversion(conversion) => format!(
            "conversion {} -> {} by {}hook",
            conversion.from.name,
            conversion.to.name,
            if conversion.hook.is_async {
                "async "
            } else {
                ""
            }
        ),
    });
    out
}

fn names(idents: &[Ident], separator: &str) -> String {
    idents
        .iter()
        .map(|ident| ident.name.as_str())
        .collect::<Vec<_>>()
        .join(separator)
}

/// `head { .. }` with each line indented, or `head {}`.
fn braces(head: &str, lines: Vec<String>) -> String {
    if lines.is_empty() {
        return format!("{head} {{}}");
    }
    let mut out = format!("{head} {{\n");
    for line in lines {
        for piece in line.lines() {
            out.push_str("    ");
            out.push_str(piece);
            out.push('\n');
        }
    }
    out.push('}');
    out
}

/// An operation's or template's entries, without examples: they are values
/// to read in the source, not part of the shape.
fn render_entries(entries: &[OpEntry]) -> Vec<String> {
    entries
        .iter()
        .filter_map(|entry| match entry {
            OpEntry::Http(http) => Some(format!(
                "http {} {}{}",
                http.method.name,
                quote(&http.uri.value),
                http.status
                    .as_ref()
                    .map(|status| format!(" -> {}", status.text))
                    .unwrap_or_default()
            )),
            OpEntry::Call(call) => Some(format!("{}({})", call.name.name, render_args(&call.args))),
            OpEntry::Body(body) => {
                let mut head = String::new();
                for modifier in &body.modifiers {
                    head.push_str(&modifier.name);
                    head.push(' ');
                }
                head.push_str(match body.kind {
                    BodyKind::Input => "input",
                    BodyKind::Output => "output",
                });
                Some(braces(
                    &head,
                    body.members.iter().map(render_member).collect(),
                ))
            }
            OpEntry::Errors(errors, _) => Some(format!("errors [{}]", names(errors, ", "))),
            OpEntry::Example(_) => None,
            OpEntry::Requirement(requirement) => Some(render_requirement(requirement)),
        })
        .collect()
}

fn render_requirement(requirement: &Requirement) -> String {
    match &requirement.kind {
        RequirementKind::None(_) => "requires none".to_owned(),
        RequirementKind::Pending(reason) => {
            format!("requires pending because {}", quote(&reason.value))
        }
        RequirementKind::Filters {
            permission,
            resource,
        } => format!("filters {} on {}", permission.name, resource.name),
        RequirementKind::Requires {
            permission,
            target,
            when,
            handler,
        } => {
            let mut out = format!("requires {}", permission.name);
            if let Some(target) = target {
                out.push_str(&format!(" on {}", target.resource.name));
                if let Some(bindings) = &target.bindings {
                    let bindings: Vec<String> = bindings
                        .iter()
                        .map(|b| {
                            let value = match &b.value {
                                BindingValue::All(_) => "*".to_owned(),
                                BindingValue::Label(label) => format!("${}", label.name),
                                BindingValue::Path(path) => names(&path.parts, "."),
                            };
                            match &b.name {
                                Some(name) => format!("{}: {value}", name.name),
                                None => value,
                            }
                        })
                        .collect();
                    out.push_str(&format!("({})", bindings.join(", ")));
                }
            }
            if let Some(when) = when {
                out.push_str(&format!(" when {}", render_condition(when)));
            }
            if handler.is_some() {
                out.push_str(" by handler");
            }
            out
        }
    }
}

fn render_condition(condition: &Condition) -> String {
    let group = |c: &Condition| match c.kind {
        ConditionKind::And(_) | ConditionKind::Or(_) => format!("({})", render_condition(c)),
        _ => render_condition(c),
    };
    match &condition.kind {
        ConditionKind::Has(path) => format!("has {}", names(&path.parts, ".")),
        ConditionKind::Equals(path, value) => {
            format!("{} == {}", names(&path.parts, "."), render_value(value))
        }
        ConditionKind::In(path, value) => {
            format!("{} in {}", names(&path.parts, "."), render_value(value))
        }
        ConditionKind::And(parts) => parts.iter().map(group).collect::<Vec<_>>().join(" and "),
        ConditionKind::Or(parts) => parts.iter().map(group).collect::<Vec<_>>().join(" or "),
        ConditionKind::Not(inner) => format!("not {}", group(inner)),
    }
}

fn render_permission(entry: &PermissionEntry) -> String {
    let mut out = render_prefix(&entry.annotations, &entry.modifiers);
    out.push_str(&format!(
        "permission {} = {}",
        entry.name.name,
        quote(&entry.value.value)
    ));
    out
}

fn render_clause(clause: &Clause) -> String {
    match &clause.value {
        ClauseValue::Args(args) => format!("{}({})", clause.name.name, render_args(args)),
        ClauseValue::Block(inner) => {
            braces(&clause.name.name, inner.iter().map(render_clause).collect())
        }
        ClauseValue::Value(value) => format!("{} {}", clause.name.name, render_value(value)),
    }
}

fn render_entry(entry: &ResourceEntry) -> String {
    let mut out = render_prefix(&entry.annotations, &entry.modifiers);
    out.push_str(&format!(
        "{} {}{}: {}",
        match entry.kind {
            ResourceEntryKind::Id => "id",
            ResourceEntryKind::Property => "property",
        },
        entry.name.name,
        if entry.optional { "?" } else { "" },
        render_type(&entry.ty)
    ));
    if let Some(alias) = &entry.alias {
        out.push_str(&format!(" as {}", alias.name));
    }
    out
}

/// A member on one line, as written: annotations, modifiers, then the member.
fn render_member(member: &Member) -> String {
    let mut out = render_prefix(&member.annotations, &member.modifiers);
    match &member.kind {
        MemberKind::Include(name) => out.push_str(&format!("include {}", name.name)),
        MemberKind::All(_) => out.push_str("$*"),
        MemberKind::Elided {
            name,
            alias,
            optional,
        } => {
            out.push('$');
            out.push_str(&name.name);
            if let Some(alias) = alias {
                out.push_str(&format!(" as {}", alias.name));
            }
            if *optional {
                out.push('?');
            }
        }
        MemberKind::Field {
            name,
            optional,
            ty,
            default,
        } => {
            out.push_str(&name.name);
            if *optional {
                out.push('?');
            }
            if let Some(ty) = ty {
                out.push_str(&format!(": {}", render_type(ty)));
            }
            if let Some(default) = default {
                out.push_str(&format!(" = {}", render_value(default)));
            }
        }
    }
    out
}

fn render_enum_value(value: &EnumValue) -> String {
    let mut out = render_prefix(&value.annotations, &value.modifiers);
    out.push_str(&value.name.name);
    if let Some(inner) = &value.value {
        out.push_str(&format!(" = {}", render_value(inner)));
    }
    out
}

fn render_prefix(annotations: &[Annotation], modifiers: &[Ident]) -> String {
    let mut out = String::new();
    for annotation in annotations {
        out.push_str(&render_annotation(annotation));
        out.push(' ');
    }
    for modifier in modifiers {
        out.push_str(&modifier.name);
        out.push(' ');
    }
    out
}

fn render_annotation(annotation: &Annotation) -> String {
    if annotation.args.is_empty() {
        format!("@{}", annotation.name.name)
    } else {
        format!(
            "@{}({})",
            annotation.name.name,
            render_args(&annotation.args)
        )
    }
}

fn render_args(args: &[Arg]) -> String {
    args.iter()
        .map(|arg| match &arg.key {
            Some(Key::Name(key)) => format!("{}: {}", key.name, render_value(&arg.value)),
            Some(Key::Text(key)) => format!("{} = {}", quote(&key.value), render_value(&arg.value)),
            None => render_value(&arg.value),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_type(ty: &TypeRef) -> String {
    format!("{}{}", ty.name.name, render_constraints(&ty.constraints))
}

fn render_constraints(constraints: &[Constraint]) -> String {
    if constraints.is_empty() {
        return String::new();
    }
    let inner: Vec<String> = constraints.iter().map(render_constraint).collect();
    format!(" {{ {} }}", inner.join("; "))
}

fn render_constraint(constraint: &Constraint) -> String {
    let name = &constraint.name.name;
    match &constraint.value {
        ConstraintValue::Flag => name.clone(),
        ConstraintValue::Range { min, max } => format!(
            "{name} {}..{}",
            min.as_ref().map_or("", |n| n.text.as_str()),
            max.as_ref().map_or("", |n| n.text.as_str())
        ),
        ConstraintValue::Value(value) => format!("{name} {}", render_value(value)),
    }
}

fn render_value(value: &Value) -> String {
    match value {
        Value::Str(s) => quote(&s.value),
        Value::Number(n) => n.text.clone(),
        Value::Ident(ident) => ident.name.clone(),
        Value::List(items, _) => format!(
            "[{}]",
            items
                .iter()
                .map(render_value)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Object(fields, _) if fields.is_empty() => "{}".to_owned(),
        Value::Object(fields, _) => format!(
            "{{ {} }}",
            fields
                .iter()
                .map(|field| {
                    let key = match &field.key {
                        Key::Name(name) => name.name.clone(),
                        Key::Text(text) => quote(&text.value),
                    };
                    format!("{key}: {}", render_value(&field.value))
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// A string literal as the source would write it.
fn quote(text: &str) -> String {
    let mut out = String::from('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ---- Markdown --------------------------------------------------------------

/// Documentation as Markdown, with a code fence it leaves open closed, so
/// it cannot swallow what the hover shows after it.
fn documentation(text: &str) -> String {
    let mut out = text.trim().to_owned();
    let mut open: Option<(char, usize)> = None;
    for line in out.lines() {
        let line = line.trim_start();
        let Some(mark) = line.chars().next().filter(|c| *c == '`' || *c == '~') else {
            continue;
        };
        let run = line.chars().take_while(|c| *c == mark).count();
        if run < 3 {
            continue;
        }
        match open {
            None => open = Some((mark, run)),
            Some((open_mark, open_run))
                if mark == open_mark && run >= open_run && line[run..].trim().is_empty() =>
            {
                open = None;
            }
            Some(_) => {}
        }
    }
    if let Some((mark, run)) = open {
        out.push('\n');
        out.push_str(&mark.to_string().repeat(run));
    }
    out
}

/// The longest run of backticks in `text`.
fn backticks(text: &str) -> usize {
    text.split(|c| c != '`').map(str::len).max().unwrap_or(0)
}

/// Inline code that survives backticks in `text`.
fn code(text: &str) -> String {
    let longest = backticks(text);
    let ticks = "`".repeat(longest + 1);
    if longest == 0 {
        format!("{ticks}{text}{ticks}")
    } else {
        format!("{ticks} {text} {ticks}")
    }
}

/// A code block tagged [`FENCE`], fenced longer than any backtick run in it.
fn fence(body: &str) -> String {
    let ticks = "`".repeat(backticks(body).max(2) + 1);
    format!("{ticks}{FENCE}\n{body}\n{ticks}")
}

fn join(paragraphs: Vec<String>) -> String {
    paragraphs
        .into_iter()
        .filter(|paragraph| !paragraph.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hover at the `nth` whole-word occurrence of `word` in `source`.
    fn hover_at(source: &str, word: &str, nth: usize) -> Option<String> {
        let parsed = crucible_lang::parse(source);
        let index = Index::new(std::slice::from_ref(&parsed));
        let offset = whole_words(source, word).nth(nth)?;
        hover(&parsed, source, &index, &[], offset).map(|(_, _, markdown)| markdown)
    }

    fn whole_words<'s>(source: &'s str, word: &'s str) -> impl Iterator<Item = usize> + 's {
        source
            .match_indices(word)
            .map(|(at, _)| at)
            .filter(move |at| {
                let before = source[..*at].chars().last();
                let after = source[at + word.len()..].chars().next();
                let part = |c: char| c.is_alphanumeric() || c == '_';
                !before.is_some_and(part) && !after.is_some_and(part)
            })
    }

    /// Every fence is tagged and closed, and no prose sentence is inside
    /// one: code blocks hold source, and everything else is outside them.
    fn assert_fenced(markdown: &str) {
        let mut open = false;
        for line in markdown.lines() {
            if line.starts_with("```") {
                if open {
                    assert_eq!(line, "```", "{markdown}");
                } else {
                    assert_eq!(line, format!("```{FENCE}"), "{markdown}");
                }
                open = !open;
            }
        }
        assert!(!open, "unclosed fence:\n{markdown}");
    }

    #[test]
    fn every_keyword_row_parses_and_is_what_its_word_hovers_to() {
        for (word, contexts, about, example) in KEYWORDS {
            let parsed = crucible_lang::parse(example);
            assert!(
                parsed.diagnostics.is_empty(),
                "`{word}`'s example does not parse: {:?}\n{example}",
                parsed.diagnostics
            );
            let bare = word.trim_start_matches('@');
            let index = Index::new(std::slice::from_ref(&parsed));
            let reached = whole_words(example, bare).any(|at| {
                hover(&parsed, example, &index, &[], at)
                    .is_some_and(|(_, _, markdown)| markdown.contains(about))
            });
            assert!(
                reached,
                "`{word}` ({contexts:?}) does not hover to its row in its own example:\n{example}"
            );
        }
    }

    #[test]
    fn a_word_means_one_thing_per_context() {
        for (index, (word, contexts, _, _)) in KEYWORDS.iter().enumerate() {
            for (other, other_contexts, _, _) in &KEYWORDS[index + 1..] {
                if word != other {
                    continue;
                }
                assert!(
                    !contexts.is_empty() && !other_contexts.is_empty(),
                    "`{word}` has a row for anywhere and another"
                );
                assert!(
                    !contexts.iter().any(|c| other_contexts.contains(c)),
                    "`{word}` has two rows for one context"
                );
            }
        }
    }

    #[test]
    fn keywords_explain_themselves_with_a_fenced_example() {
        let source = r#"
@since("1")
readonly operation GetCity reads City {
    http GET "/cities/{cityId}"
    input {
        $cityId
        @query("v") verbose?: boolean
    }
}
metadata owner on operation: string
string Code { length 1..3 }
"#;
        let structure = hover_at(source, "operation", 0).unwrap();
        assert!(structure.starts_with("**`operation`**\n\nDeclares an operation"));
        assert!(structure.contains("```crucible\nreadonly operation GetCity reads City {"));
        assert_fenced(&structure);

        let reads = hover_at(source, "reads", 0).unwrap();
        assert!(reads.contains("implies `readonly`"), "{reads}");
        let query = hover_at(source, "query", 0).unwrap();
        assert!(query.starts_with("**`@query`**"), "{query}");
        assert!(query.contains("```crucible\noperation ListCities {"));
        assert!(
            hover_at(source, "since", 0)
                .unwrap()
                .contains("The version that added it.")
        );
        assert!(
            hover_at(source, "readonly", 0)
                .unwrap()
                .contains("only reads")
        );
        assert!(
            hover_at(source, "length", 0)
                .unwrap()
                .contains("inclusive at both ends")
        );
        assert!(
            hover_at(source, "string", 1)
                .unwrap()
                .contains("Unicode characters")
        );
        // `on` attaches an operation, but lists a metadata key's targets.
        let metadata = hover_at(source, "on", 0).unwrap();
        assert!(metadata.contains("What the key applies to"), "{metadata}");
        let attach = hover_at("operation Fix on City {}", "on", 0).unwrap();
        assert!(attach.contains("Attaches any other operation"), "{attach}");
        // The keyword starting a declaration, and the same word in an example.
        let examples = "operation A {\n    example \"x\" {\n        error NotFound {}\n    }\n}\nerror NotFound { status 404 }\n";
        assert!(
            hover_at(examples, "error", 0)
                .unwrap()
                .contains("The error the example's input produces")
        );
        assert!(
            hover_at(examples, "error", 1)
                .unwrap()
                .contains("Declares an error")
        );
        // A name that is only a word here explains nothing.
        assert_eq!(hover_at("operation A { http GET \"/\" }", "GET", 0), None);
    }

    const MODEL: &str = r#"
/// One audit event.
structure AuditEventData {
    id: AuditId
    at: timestamp
    note?: string { length ..200 }
}

/// Identifies an audit event.
string AuditId { length 1..64; pattern "^ae_[a-z0-9]+$" }

/// A page of events.
list AuditEventList: AuditEventData { length ..100 }

list Tags: Tag { length 1..10; unique }
string Tag { length 1..1 }

map Labels: LabelKey -> integer { length ..50 }
string LabelKey { pattern "^[a-z]+$" }

enum Size { SMALL = "small", LARGE }
enum Level: integer { LOW = 1 }

integer Count { range 0.. }
integer Percent { range 0..100 }

structure Page {
    /// The events on this page.
    events: AuditEventList
    tags?: Tags
    id: AuditId { length 1..32 }
    count: Count = 0
    size?: Size
    labels: Labels
}

union Shape { circle: string, empty }

error Throttled {
    status 429
    retryable throttling
    message: string
}

resource City {
    /// The city's code.
    id cityId: AuditId
    property name: string { length 1..40 }
}

structure CityData for City {
    $cityId as id
    $name?
}

readonly operation GetCity reads City {
    http GET "/cities/{cityId}"
    input { $cityId }
    output { city: CityData }
}
"#;

    #[test]
    fn a_list_shows_its_items_and_both_their_constraints() {
        // At the declaration and at a use, the same full shape.
        for nth in [0, 1] {
            let list = hover_at(MODEL, "AuditEventList", nth).unwrap();
            assert_fenced(&list);
            assert!(
                list.starts_with(
                    "```crucible\nlist AuditEventList: AuditEventData { length ..100 }\n```"
                ),
                "{list}"
            );
            assert!(
                list.contains("A list of `AuditEventData` (structure)."),
                "{list}"
            );
            assert!(list.contains("Constraints: at most 100 items."), "{list}");
            assert!(list.contains("A page of events."), "{list}");
            assert!(
                list.contains("**Items:** `AuditEventData`, a structure"),
                "{list}"
            );
            assert!(
                list.contains(
                    "```crucible\nstructure AuditEventData {\n    id: AuditId\n    at: timestamp\n    note?: string { length ..200 }\n}\n```"
                ),
                "{list}"
            );
        }
        let tags = hover_at(MODEL, "Tags", 0).unwrap();
        assert!(
            tags.contains("Constraints: 1 to 10 items; no duplicate items."),
            "{tags}"
        );
        // The item's own constraints, in words, under its declaration.
        assert!(
            tags.contains("```crucible\nstring Tag { length 1..1 }\n```"),
            "{tags}"
        );
        assert!(tags.contains("Constraints: exactly 1 character."), "{tags}");
    }

    #[test]
    fn a_map_shows_its_key_and_value() {
        let map = hover_at(MODEL, "Labels", 0).unwrap();
        assert_fenced(&map);
        assert!(map.contains("```crucible\nmap Labels: LabelKey -> integer { length ..50 }\n```"));
        assert!(
            map.contains("A map from `LabelKey` (string) to `integer` (built-in)."),
            "{map}"
        );
        assert!(map.contains("at most 50 entries"), "{map}");
        assert!(map.contains("**Keys:** `LabelKey`, a string"), "{map}");
        assert!(map.contains("matches `^[a-z]+$`"), "{map}");
    }

    #[test]
    fn simple_types_show_their_constraints_in_words() {
        let id = hover_at(MODEL, "AuditId", 1).unwrap();
        assert!(id.starts_with(
            "```crucible\nstring AuditId { length 1..64; pattern \"^ae_[a-z0-9]+$\" }\n```"
        ));
        assert!(
            id.contains("Constraints: 1 to 64 characters; matches `^ae_[a-z0-9]+$`."),
            "{id}"
        );
        assert!(id.contains("Identifies an audit event."));
        assert!(
            hover_at(MODEL, "Count", 0)
                .unwrap()
                .contains("Constraints: at least 0.")
        );
        assert!(
            hover_at(MODEL, "Percent", 0)
                .unwrap()
                .contains("Constraints: 0 to 100, inclusive.")
        );
        let size = hover_at(MODEL, "Size", 0).unwrap();
        assert!(
            size.contains("```crucible\nenum Size {\n    SMALL = \"small\"\n    LARGE\n}\n```")
        );
        assert!(size.contains("A string enum with 2 values."));
        assert!(
            hover_at(MODEL, "Level", 0)
                .unwrap()
                .contains("An integer enum with 1 value.")
        );
        let throttled = hover_at(MODEL, "Throttled", 0).unwrap();
        assert!(
            throttled.contains("Sent with status 429, a client fault."),
            "{throttled}"
        );
        assert!(throttled.contains("retryable throttling"));
    }

    #[test]
    fn a_structure_shows_every_member_with_its_constraints() {
        let page = hover_at(MODEL, "Page", 0).unwrap();
        assert_fenced(&page);
        assert!(page.contains(
            "structure Page {\n    events: AuditEventList\n    tags?: Tags\n    id: AuditId { length 1..32 }\n    count: Count = 0\n    size?: Size\n    labels: Labels\n}"
        ), "{page}");
        let view = hover_at(MODEL, "CityData", 0).unwrap();
        assert!(view.contains("structure CityData for City {\n    $cityId as id\n    $name?\n}"));
        assert!(view.contains("A view of resource `City`"));
    }

    #[test]
    fn a_member_shows_its_type_presence_and_constraints() {
        // A member constraint replaces its type's of the same name.
        let id = hover_at(MODEL, "id", 1).unwrap();
        assert_fenced(&id);
        assert!(
            id.starts_with("```crucible\nid: AuditId { length 1..32 }\n```"),
            "{id}"
        );
        assert!(
            id.contains("A required member of structure `Page`."),
            "{id}"
        );
        assert!(id.contains(
            "Constraints: 1 to 32 characters (the member's, replacing `length 1..64` from `AuditId`); matches `^ae_[a-z0-9]+$` (from `AuditId`)."
        ), "{id}");
        assert!(id.contains("**Type:** `AuditId`, a string"), "{id}");

        // The first two are in documentation.
        let events = hover_at(MODEL, "events", 2).unwrap();
        assert!(events.contains("The events on this page."));
        assert!(
            events.contains("Constraints from `AuditEventList`: at most 100 items."),
            "{events}"
        );
        // The list, then its items.
        assert!(
            events.contains("**Type:** `AuditEventList`, a list"),
            "{events}"
        );
        assert!(
            events.contains("**Items:** `AuditEventData`, a structure"),
            "{events}"
        );

        let count = hover_at(MODEL, "count", 0).unwrap();
        assert!(
            count.contains("A member of structure `Page`, defaulting to `0` when omitted."),
            "{count}"
        );
        let size = hover_at(MODEL, "size", 0).unwrap();
        assert!(
            size.contains("An optional member of structure `Page`."),
            "{size}"
        );
        assert!(size.contains("enum Size {"), "{size}");
        let note = hover_at(MODEL, "note", 0).unwrap();
        assert!(
            note.contains("Constraints: at most 200 characters."),
            "{note}"
        );
        let empty = hover_at(MODEL, "empty", 0).unwrap();
        assert!(
            empty.contains("A member of union `Shape`; it carries nothing."),
            "{empty}"
        );
    }

    #[test]
    fn resource_bindings_show_the_entry_they_bind() {
        let entry = hover_at(MODEL, "cityId", 0).unwrap();
        assert!(
            entry.starts_with("```crucible\nid cityId: AuditId\n```"),
            "{entry}"
        );
        assert!(
            entry.contains("An identifier of resource `City`."),
            "{entry}"
        );
        assert!(entry.contains("The city's code."));
        assert!(entry.contains("Constraints from `AuditId`"), "{entry}");

        let view = hover_at(MODEL, "cityId", 1).unwrap();
        assert!(
            view.starts_with("```crucible\n$cityId as id\n```"),
            "{view}"
        );
        assert!(view.contains(
            "A required member of structure `CityData`, bound to identifier `cityId` of resource `City`: `AuditId`."
        ), "{view}");
        assert!(view.contains("The city's code."));

        let name = hover_at(MODEL, "name", 1).unwrap();
        assert!(
            name.contains("An optional member of structure `CityData`, bound to property `name`"),
            "{name}"
        );
        assert!(name.contains("Constraints: 1 to 40 characters."), "{name}");

        let input = hover_at(MODEL, "cityId", 3).unwrap();
        assert!(
            input.contains("A required member of the input of `GetCity`"),
            "{input}"
        );
    }

    #[test]
    fn entries_show_their_keywords_presence_and_spelling() {
        let source = "resource Inbox {
    id inboxId: InboxId as id
    immutable property address: Address
    property note?: Note
    writeonly property secret: Secret
}
structure InboxData for Inbox {
    $*
}
structure Loose {
    $*
}
";
        let id = hover_at(source, "inboxId", 0).unwrap();
        assert!(
            id.starts_with(
                "```crucible
id inboxId: InboxId as id
```"
            ),
            "{id}"
        );
        assert!(
            id.contains(
                "Shown as `id` in `Inbox`'s own views and outputs; inputs bind it as `inboxId`."
            ),
            "{id}"
        );
        let address = hover_at(source, "address", 0).unwrap();
        assert!(
            address.starts_with(
                "```crucible
immutable property address: Address
```"
            ),
            "{address}"
        );
        assert!(
            address.contains("A required property of resource `Inbox`, set by `creates` or `puts` and fixed afterwards."),
            "{address}"
        );
        let note = hover_at(source, "note", 0).unwrap();
        assert!(
            note.starts_with(
                "```crucible
property note?: Note
```"
            ),
            "{note}"
        );
        assert!(note.contains("An optional property"), "{note}");

        // `$*` explains itself and lists what it includes here: the
        // identifier under its spelling, then the readable properties.
        let parsed = crucible_lang::parse(source);
        let index = Index::new(std::slice::from_ref(&parsed));
        let at = source.find("$*").unwrap();
        for offset in [at, at + 1, at + 2] {
            let (start, end, star) = hover(&parsed, source, &index, &[], offset).unwrap();
            assert_eq!((start as usize, end as usize), (at, at + 2));
            assert!(
                star.contains("Here it includes, from resource `Inbox`:"),
                "{star}"
            );
            assert!(
                star.contains(
                    "```crucible
id: InboxId
address: Address
note?: Note
```"
                ),
                "{star}"
            );
            assert_fenced(&star);
        }
        let loose = source.rfind("$*").unwrap();
        let (_, _, star) = hover(&parsed, source, &index, &[], loose).unwrap();
        assert!(star.contains("not a `for` view"), "{star}");
    }

    #[test]
    fn bindings_show_presence_and_spelling_where_they_apply() {
        let source = "resource Inbox {
    id inboxId: InboxId as id
    property note?: Note
}
structure InboxView for Inbox {
    $inboxId
    $note
}
operation EditInbox updates Inbox {
    input {
        $inboxId
        $note
        $*
    }
}
";
        // A view: the property's `?` applies, and the identifier its spelling.
        let view_note = hover_at(source, "note", 1).unwrap();
        assert!(
            view_note.contains("An optional member of structure `InboxView`"),
            "{view_note}"
        );
        let view_id = hover_at(source, "inboxId", 1).unwrap();
        assert!(view_id.contains("Named `id` here"), "{view_id}");
        // An input asks for what it needs, by identifier name.
        let input_note = hover_at(source, "note", 2).unwrap();
        assert!(
            input_note.contains("A required member of the input of `EditInbox`"),
            "{input_note}"
        );
        let input_id = hover_at(source, "inboxId", 2).unwrap();
        assert!(!input_id.contains("Named `id`"), "{input_id}");
        // `$*` in an input expands to nothing: lowering refuses it there.
        let parsed = crucible_lang::parse(source);
        let index = Index::new(std::slice::from_ref(&parsed));
        let at = source.find("$*").unwrap();
        let (_, _, star) = hover(&parsed, source, &index, &[], at).unwrap();
        assert!(star.contains("not a `for` view"), "{star}");
        assert!(!star.contains("Here it includes,"), "{star}");
    }

    #[test]
    fn code_spans_and_fences_survive_backticks() {
        assert_eq!(code("a`b"), "`` a`b ``");
        assert_eq!(fence("x"), "```crucible\nx\n```");
        assert_eq!(fence("```"), "````crucible\n```\n````");
        assert_eq!(quote("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }

    #[test]
    fn an_unclosed_fence_in_documentation_is_closed() {
        let source = "/// Like this:\n/// ```crucible\n/// list A: B\nlist Items: Item { length ..5 }\nstring Item { length 1..3 }\n";
        let items = hover_at(source, "Items", 0).unwrap();
        assert_fenced(&items);
        // What follows the documentation is still outside any fence.
        assert!(
            items.ends_with(
                "```crucible\nstring Item { length 1..3 }\n```\n\nConstraints: 1 to 3 characters."
            ),
            "{items}"
        );
        assert_eq!(documentation("a\n~~~\nb\n~~~"), "a\n~~~\nb\n~~~");
        assert_eq!(documentation("a\n````\nb"), "a\n````\nb\n````");
    }

    #[test]
    fn only_built_in_annotations_explain_themselves() {
        let source = "structure A {\n    @length(1) @internal @sensitive a: string\n}\nmetadata k on member conflicts sensitive\n";
        assert_eq!(hover_at(source, "length", 0), None);
        assert_eq!(hover_at(source, "internal", 0), None);
        assert_eq!(hover_at(source, "sensitive", 0), None);
        // A key a metadata declaration names is the modifier it means.
        let conflict = hover_at(source, "sensitive", 1).unwrap();
        assert!(conflict.contains("redacted from logs"), "{conflict}");
    }

    #[test]
    fn counts_are_singular_only_when_exact() {
        let source =
            "string A { length 0..1 }\nstring B { length ..1 }\nlist C: A { length 1..1 }\n";
        assert!(
            hover_at(source, "A", 0)
                .unwrap()
                .contains("0 to 1 characters.")
        );
        assert!(
            hover_at(source, "B", 0)
                .unwrap()
                .contains("at most 1 character.")
        );
        assert!(
            hover_at(source, "C", 0)
                .unwrap()
                .contains("exactly 1 item.")
        );
    }

    #[test]
    fn words_in_templates_explain_nothing_yet() {
        let source = "template lists {\n    paginated(items: items)\n}\n";
        assert_eq!(hover_at(source, "lists", 0), None);
        assert_eq!(hover_at(source, "paginated", 0), None);
        assert!(
            hover_at(source, "template", 0)
                .unwrap()
                .contains("Not supported by the compiler yet")
        );
    }
}
