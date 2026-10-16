# Crucible language reference

Working reference for the internal modelling language. It describes what
`crucible-lang` parses today; [decisions.md](decisions.md) says why. The
language is internal, so this changes as implementation teaches.

## Files and packages

A package is a directory with a `crucible.toml` whose `[model]` table names
the namespace, the language version and the source directories. Every
`.cru` file under those directories belongs to the package's single
namespace. File and declaration order never matter.

A package imports others in its manifest: `imports = ["crucible.errors"]`
names a bundled package, and `imports = ["org.standards"]` with
`[model.dependencies]` `"org.standards" = { path = "../standards" }` loads
the package in that directory, which must declare the namespace. A package's
declarations and `metadata` are visible to the packages that import it, and
its rules apply to them (see Policy rules).

## Lexical structure

- **Whitespace and newlines** separate tokens and carry no meaning.
- **Comments.** `//` runs to the end of the line and is ignored. `///` is
  documentation: consecutive `///` lines are Markdown attached to the next
  declaration, member, enum value, rule, step or example.
- **Doc links.** In documentation, `[Name]` links to a declaration of the
  package or its imports, and `[Name.member]` to a member of a structure,
  union or error, a value of an enum, or a member of an operation's input
  (else its output): `[CreateApiKey]`, `[City.name]`, `[Color.RED]`,
  `[GetCity.cityId]`. The collapsed `[Name][]` and the full
  `[the city call][GetCity]` link the same way. A label is a capitalised
  identifier with an optional `.member`; other bracketed text (`[1]`,
  `[optional]`) is prose, brackets in code are code, and a `[Name]: url`
  definition in the same comment makes `[Name]` an ordinary link. A doc
  link that names nothing is an error at its label (decision 80). A
  prelude type (`[String]`) is accepted and renders as its name.
- **Identifiers.** A letter or `_`, then letters, digits, `_`, or `-`
  between two letters or digits (`date-time`). The name of a declaration,
  member or URI label cannot contain `-`. Keywords are contextual:
  `operation`, `input` and the rest are ordinary identifiers outside the
  position where they introduce something.
- **Strings.** Double-quoted with `\"`, `\\`, `\n`, `\t`, `\r` and
  `\u{XXXX}` escapes (hex digits only).
- **Byte-order mark.** A leading BOM is whitespace.
- **Numbers.** Optional `-`, digits, optional fraction and exponent. `1..5`
  is two integers around `..`, never a float.
- **Punctuation.** `{ } ( ) [ ] , ; : ? = . .. -> <- @ $ *` and the
  comparison operators `== != < <= > >=`.

## Grammar

EBNF. `{ x }` is zero or more, `[ x ]` optional, `|` alternation. `;` and
`,` are accepted between any two list elements and between body entries
but are never required.

```ebnf
file        = { item } ;
item        = [ doc ] { annotation } { modifier } declaration ;
modifier    = "readonly" | "idempotent" | "public" | "strict" | "internal" | "sensitive"
            | "sparse" ;
                                                           (* each position checks its own *)

declaration = service | resource | operation | structure | union | enum
            | error | scalar | list | map | metadata | rule | template
            | except | migration | protocol | conversion ;

annotation  = "@" ident [ "(" [ arg { "," arg } [ "," ] ] ")" ] ;
arg         = [ ( ident ":" ) | ( string "=" ) ] value ;
value       = string | number | ident | "[" [ value { "," value } ] "]"
            | "{" [ field { "," field } ] "}" ;
field       = ( ident | string ) ":" value ;

service     = "service" ident "{" { clause } "}" ;
                 (* clauses: version, title, protocol, auth, paginated(..),
                    operations, resources, errors, notFound Error *)
protocol    = "protocol" ident "{" { clause } "}" ;
clause      = ident ( "(" args ")" | "{" { clause } "}" | value ) ;

resource    = "resource" ident [ "in" ident ] "{" { resource_entry } "}" ;
                 (* annotations on a resource: @authorizationName("name") and the
                    documentation notes @deprecated, @since, @unstable, @externalDocs *)
resource_entry = [ doc ] { annotation } { entry_modifier } ( "id" | "property" )
                 ident [ "?" ] ":" type_ref [ "as" ident ]
               | permission_entry ;
                 (* `?` on a property, `as` on an identifier: its name in views *)
entry_modifier = "readonly" | "immutable" | "writeonly" ;
                 (* properties only; `immutable writeonly` is the one combination *)
permission_entry = [ doc ] { annotation } [ "internal" ] "permission" ident "=" string ;
                 (* annotations: @permissionOnly, @notGrantable, @aliasOf(symbol),
                    @deprecated, @since *)

operation   = "operation" ident [ role ident [ "collection" ] ] "{" { op_entry } "}" ;
role        = ident ;          (* creates | puts | reads | updates | deletes | lists | on;
                                  lowering reports any other word; collection follows on *)
op_entry    = http | clause_call | body | errors | example | requirement ;
http        = "http" ident string [ "->" number ] ;
clause_call = ident "(" args ")" ;                         (* paginated(..) *)
body        = [ doc ] { modifier } ( "input" | "output" ) "{" { member } "}" ;
errors      = "errors" "[" ident { "," ident } "]" ;
example     = [ doc ] "example" string "{" { example_part } "}" ;
example_part = ( "input" | "output" ) value | "error" ident value ;
requirement = [ doc ] "requires" ( "none"
                                 | "pending" "because" string
                                 | ident [ "on" req_target ] [ "when" condition ] [ "by" "handler" ] )
            | [ doc ] "filters" ident "on" ident ;
                 (* `none` and `pending` are never a permission's symbol. The
                    `///` of `requires none` or `pending` documents the model
                    only: the IR keeps a pending reason, not this text *)
req_target  = ident [ "(" binding { "," binding } ")" ] ;
binding     = [ ident ":" ] ( "*" | "$" ident | dotted ) ;
                 (* an unnamed binding binds the resource's own identifier *)
condition   = cond_and { "or" cond_and } ;
cond_and    = cond_not { "and" cond_not } ;
cond_not    = "not" cond_not | cond_atom ;
cond_atom   = "(" condition ")" | "has" dotted | dotted "==" value | dotted "in" value ;
                 (* values: strings, numbers, true, false; `in` takes a list.
                    `has` or `not` before `.`, `==` or `in [` starts a member
                    path: a member may be named `has` *)
permission_value = segment { ":" segment } ;   (* the string of a permission entry *)
segment     = lower { lower | upper | digit } { "-" ( lower | digit ) { lower | upper | digit } } ;

structure   = "structure" ident [ "for" ident { "," ident } ] "{" { member } "}" ;
                 (* `for A, B`: one view of several resources (decision 34j) *)
union       = "union" ident "{" { member } "}" ;
error       = "error" ident "{" { error_entry } "}" ;
error_entry = "status" number | "retryable" [ "throttling" ] | member ;

member      = [ doc ] { annotation } { modifier } ( include | all | elided | field_member ) ;
                                                           (* modifiers: internal, sensitive *)
include     = "include" ident ;
all         = "$" "*" ;          (* in a `for` view only *)
elided      = "$" ident [ "as" ident ] [ "?" ] ;
field_member = ident [ "?" ] [ ":" type_ref ] [ "=" value ] ;   (* no type: unit union arm *)

type_ref    = ident [ constraints ] ;
constraints = "{" { constraint } "}" ;
constraint  = ident [ range | value ] ;   (* length 1..100, range 0.., pattern "..", unique,
                                           format date-time, mediaType "image/png" *)
range       = [ number ] ".." [ number ] ;

scalar      = scalar_kind ident [ constraints ] ;
scalar_kind = "string" | "boolean" | "byte" | "short" | "integer" | "long"
            | "float" | "double" | "bigInteger" | "bigDecimal"
            | "timestamp" | "blob" | "document" ;
list        = "list" ident ":" ident [ constraints ] ;   (* constraints are the list's *)
map         = "map" ident ":" ident "->" ident [ constraints ] ;
                 (* modifiers: internal, sensitive, sparse. A `sparse` list or map
                    allows null elements or values; the default is dense *)
enum        = "enum" ident [ ":" ident ] "{" { enum_value } "}" ;
enum_value  = [ doc ] { annotation } { modifier } ident [ "=" value ] ;
                         (* modifier: internal; value: a string, or a number in an integer enum *)

metadata    = "metadata" ident "on" target { "," target } [ ":" meta_type ]
              [ "conflicts" ident { "," ident } ]
              [ "retention" ( "source" | "runtime" ) ] ;   (* default runtime *)
target      = ident [ ident ] ;                            (* operation | enum value *)
meta_type   = "enum" "{" { enum_value } "}" | type_ref ;

template    = "template" ident "{" { op_entry } "}" ;

rule        = "rule" ident { ident } "{" { rule_clause } "}" ;   (* strict, exceptable, advice *)
rule_clause = "every" kind [ "where" expr ] { "must" expr }
            | "no" kind [ "where" expr ] ;
kind        = ident { ident } ;                            (* input member *)
expr        = and_expr { "or" and_expr } ;
and_expr    = not_expr { "and" not_expr } ;
not_expr    = "not" not_expr | atom ;
atom        = "(" expr ")"
            | "has" "one" "of" ref { "," ref }
            | "has" ref
            | "addresses" ident
            | path [ "is" ident { ident } | operator value ] ; (* bare path: a flag;
                                                              is inline primitive *)
operator    = "==" | "!=" | "<" | "<=" | ">" | ">=" | "in"
            | "startsWith" | "endsWith" | "matches" | "contains" ;
ref         = "@" ident | ident ;
path        = ref { "." ident } ;

except      = "except" ident "for" ident "because" string ;

migration   = "migration" string "from" string "{" { step } "}" ;
step        = [ doc ] ( rename | require | add | move | reshape | remove
                      | behavior | sunset | retype ) ;
rename      = "rename" dotted "->" ident ;
require     = "require" dotted ( "default" value | hook ) ;
add         = "add" dotted [ ident ] "downgrade" ident [ "{" { ident "<-" ident } "}" ] ;
move        = "move" ident "from" ident string ;
reshape     = "reshape" dotted [ "{" ident { "," ident } "}" ] "->" ident hook ;
remove      = "remove" "operation" ident ;
behavior    = "behavior" dotted ;
sunset      = "sunset" string ;
retype      = "retype" dotted "->" ident [ hook ] ;
hook        = "by" [ "async" ] "hook" ;
dotted      = ident { "." ident } ;

conversion  = "conversion" ident "->" ident hook ;
```

## Checks

Beyond the grammar, the compiler refuses a model that breaks the rules in
[decisions.md](decisions.md); one run reports every problem. The HTTP
rules of decision 35c (labels, query and header targets, header names,
bodiless responses, one payload, unique routes) and the checks on
pagination members, examples and name collisions run on the IR in
`crucible_ir::validate`. So do
the limits on recursive shapes, which are otherwise allowed (a
structure or union may reach itself through its members): a cycle must
pass through a structure or union, not lists and maps alone, and a value
must be finite, so a structure needs a way back to itself that is not all
required members, and a union a member that does not lead back, or leads
back only through a list, map or optional member. The
language's own checks include:

- a binding appears only where its part of the message exists (`@query`
  in an input; `@header` and `@payload` in an input, output or error), and
  a member has one binding;
- `@token` marks at most one input member, an optional string;
- map keys are strings or string enums;
- each constraint applies to its type and is given once, and a `pattern`
  must be runnable;
- defaults fit the member's type and constraints; an enum default names a
  value's symbol;
- enum value names, and integer enum values, are unique;
- a metadata declaration is checked once (targets, conflicts, retention,
  value type, no built-in annotation's name) and each use's value against
  its type;
- resources: the parent is a resource with no cycle, identifier names are
  unique across the chain, a property's name differs from every identifier
  the resource has (inherited ones too) and from its other properties,
  identifiers are strings or string enums and take no keyword or `?`, only
  `readonly`, `immutable`, `writeonly` and `immutable writeonly` apply to a
  property, and an identifier's `as` spelling is unique among the names the
  resource's own views show;
- `$name` names one of the resource's own identifiers or properties, or an
  inherited identifier; an ancestor's properties are not inherited;
- a view `for A, B` is lowered against `A`, and each `$name` again against `B`:
  the members must be identical, and `$*` shows what every resource declares;
- `$*` appears once, only in a `for` view, and an entry it includes is
  named again only as `$name?` after it, which loosens it for that view;
- property mutability (decision 34a): an `updates` input does not bind a
  `readonly` or `immutable` property, a `creates` or `puts` input does not
  bind a `readonly` one, some `creates`, `puts` or `on .. collection` input
  binds every `immutable` property, a view or output never shows a `writeonly` one,
  and `@mutability` on a member that binds a property agrees with its
  keyword. Bindings under `@query` and the inputs of `reads`, `lists`,
  `deletes` and `on` operations are not writes;
- roles: each lifecycle role is held once per resource, instance roles
  bind every identifier and collection roles only the parent's, and a
  role's implied keyword is not contradicted;
- service, operation, error and example clauses, list entries, object
  keys and annotations are given once; a service declares its version.
- permissions ([permissions.md](../permissions.md) §5, checks 1-18): they
  run on the IR in `crucible_ir::validate::permissions`, and each
  diagnostic starts with its code
  (`permissions.undeclared` and the like), for the operations and
  resources the service does not reach too, unless another service of the
  package reaches them. A resource that declares permissions declares an
  identifier of its own (`permissions.identifier`), so `*` never binds a
  parent's. A resource's permission symbols
  and values are unique, `none` and `pending` are no symbol, values follow `permission_value` and do not
  differ only in case, and authorization names are lowerCamelCase and
  unique; a permission is in one tier (`internal`, `@permissionOnly`,
  `@notGrantable`), and a `@permissionOnly` one is required by nothing;
  an alias is `@deprecated`, names one primary of its resource and tier, and
  is never required; a `@permissionTypes` enum is unique and of the
  service's own package, every value is
  the authorization name of a resource with permissions, and every
  resource with grantable permissions is a value or excluded;
  `requires p on R` names a permission `R` declares; a public operation
  declares no requirement, `requires none` and `requires pending` stand
  alone, and a pending reason is nonempty; every identifier of the target
  is bound once, by a member of the identifier's own type (a different
  string type with the same constraints is a warning), `$x` names an
  identifier the input binds, `*` binds only the resource's own
  identifier, and no binding passes through a list or map;
  a path through an optional member or union arm is guarded by `when`,
  which reads input members (`has` may test a list or map member, nothing
  reads into one) and compares like with like (a number of the
  member's own type; never a `bigInteger` or `bigDecimal`); a clause
  clients see (its permission, operation and service are not `internal`)
  binds and tests no `internal` member and compares with no `internal`
  enum value (`permissions.internal-member`); `filters`
  appears once, on a `lists` or `on .. collection` operation, and names a
  permission of the resource the operation is bound to, the one the
  listing returns; `by handler`
  takes no bindings or `when`; `@authorizationName` is given once, on a
  resource that declares permissions; the service's `notFound` error is an error of
  its own package, a client error with status 404
  (`permissions.not-found-error`), and without one a requirement needs
  `crucible.errors` imported (`permissions.not-found`); and once a service declares any permission,
  every authenticated operation declares `requires`, `filters`, `requires
  none` or `requires pending` (`permissions.undeclared`).

Warnings never withhold the IR. Permissions add these: a public
permission nothing requires (`permissions.unrequired`), each `requires
pending` with its reason (`permissions.pending`), `when has x` on a member
that is always present, a `filters` beside a `requires` on the same
resource, an operation that declares a 404 error besides the
service's not-found error (`permissions.second-not-found`), and a
clause on the operation's own resource or an ancestor that binds one of
its identifiers from another member than the operation is bound by, with
no clause checking the bound instance (`permissions.other-instance`): the
check would authorize an instance the handler does not load. An input
member that binds another permissioned resource's identifier that no
requirement covers is *advice* (`permissions.unchecked-reference`). The
compiler also warns about a plain member
whose name is a resource identifier or property ("did you mean
`$name`?", decision 33), a property that is neither `readonly` nor
`immutable` and that no `updates`, `puts` or instance `on` input binds
(declare it `immutable`, or `readonly` when nothing writes it, or an
identifier's `as id` for a `property id` that repeats it; decision 34a), a
`for` view used as a member type in a writing input, which binds nothing, a
resource with `reads`, `lists` or `creates` operations whose outputs show
none of its properties, a structure without `for` that restates a
resource's identifier and a property (decision 34h), an operation attached
to a resource the service does not reach, a member constraint that loosens its type's (it replaces
it, so `length ..40` on a `1..100` type drops the minimum), and a `///`
comment that documents nothing. A `[Name]` doc link that names nothing is
an error, not a warning (decision 80).

## Permissions

A resource lists the permissions it accepts; an operation says what it
requires and on which resource. Crucible never decides: the service's
`Authorizer` does ([permissions.md](../permissions.md)). Permissions are
declared in `.cru`.

```
resource Inbox in Workspace {
    id inboxId: InboxId
    /// Send, reply and forward from this address.
    permission mailSend = "mail:send"
    /// Renamed to `mail:send`.
    @deprecated @aliasOf(mailSend)
    permission emailSend = "email:send"
}

operation SendMessage on Message collection {
    http POST "/v1/workspaces/{workspaceId}/inboxes/{inboxId}/messages"
    requires mailSend on Inbox
    /// The rendered wording is what the recipient reads.
    requires read on Template(content.template.templateId) when has content.template
    input { $workspaceId; $inboxId; content: MessageContent }
}
```

- **Values and names.** The symbol (`mailSend`) is what generated code
  calls it; the value (`"mail:send"`) is its identity in grants, always
  written, compared case-sensitively. A resource's authorization name is
  its shape name in lowerCamelCase (`ApiKey` is `apiKey`) unless
  `@authorizationName("..")` says otherwise; the pair (name, value)
  identifies a permission in grants and audit records.
- **Tiers.** `internal permission` reaches the server only;
  `@permissionOnly` is required by no operation (a policy checks it);
  `@notGrantable` is a bundle the service confers whole. `@aliasOf(p)`
  keeps a renamed permission's old value; requirements name `p`.
- **Targets.** `requires p` alone is on the operation's own resource
  (instance operations only); `on R` binds each identifier of `R` from the
  input member that binds it (`$name`); `R(*)` binds the resource's own
  identifier to every instance; `R(path)` binds it from an input member
  path; `R(id: $x)` names one. A path never crosses a list or map.
- **`$x` in a binding** names an identifier the input binds: `x` is an
  identifier of the operation's own resource (inherited ones included),
  and the binding is the input member that binds it, `$x` or its `as`
  spelling, else a member named `x`. An operation bound to no resource
  binds no identifier, so there `$x` is an error; any other member is
  bound by its path (`R(id: other)`).
- **`when`** holds the clause to the input: `has a.b` is true when the
  optional member is present or the union arm selected.
- **`filters p on R`** answers a listing with only the instances the
  caller holds `p` on; **`by handler`** declares a check the handler
  discharges; **`requires none`** says no permission is needed;
  **`requires pending because ".."`** suppresses the strict default during
  migration. Each operation with a `requires` or `filters` clause gets
  the service's not-found error in its errors, unless every clause is on a
  root resource with `*`: `crucible.errors#NotFoundException` (import
  `crucible.errors`), or the service's own, which a `requires pending`
  operation gets too.
- **`notFound E`** in the service designates the service's own 404, `E`,
  for a service with errors of its own that cannot import
  `crucible.errors`. `E` is an `error` of the package with `status 404`;
  its members are the service's (the runtime asks the service's
  `not_found_factory` to build it, and handlers answer a genuine miss with
  the same constructor), and every response carrying it is sent with
  `Cache-Control: no-store`.

  ```
  service Pigeon {
      version "2026-08-01"
      protocol json
      auth bearer
      notFound ResourceNotFoundException
      errors [AccessDeniedException, InternalServerException, ValidationException]
  }
  ```
- **`@permissionTypes(exclude: [R])`** on the hand-written enum of
  resource types a grant names checks it against the resources both ways.

The IR carries them as `Service::permissions`, `Service::permission_types`,
`Service::not_found` and `Operation::requirements` (permissions.md §6.1). Removing a
permission or alias, changing a value or an authorization name, and moving
a permission to `internal` are breaking changes to the release guard;
adding a requirement is reported as an authorization change.

## Role templates

A `template` declares what every operation of a role carries, once, in the
service or in a standards package:

```
template lists {
    paginated(inputToken: nextToken, outputToken: nextToken, pageSize: maxResults)
    input {
        @query
        nextToken?: NextToken
        @query
        maxResults?: MaxResults
    }
    output { nextToken?: NextToken }
}
```

Each operation whose role is `lists` gets the `paginated` arguments, the
`errors`, and the `input` and `output` members after its own (an operation
with no body of that side gets one). The operation adds what is its own:
`paginated(items: graphs)`. Restating a template's member or a `paginated`
key is an error, so the standard cannot drift. A role has one template; only
`input`, `output`, `errors` and `paginated` belong in one. The members are
lowered where they are written, so a problem in one is reported in the
template, and a rule can tell them with `template`.

## Policy rules

A `rule` is a check every compile applies to the package's model; an
`except` excuses one violation, with a reason ([decisions.md](decisions.md),
45-47). Rules run on the lowered model once it compiles without errors, and
look at the elements the compiled package declares. Rules written in
`policy/` read as the package's standards:

```
/// Every operation is charged to one request-rate class.
rule Classified strict {
    every operation
    must has @operationClass
}

/// Only a control-plane write needs to say it works while suspended.
rule SuspensionOnlyOnControlWrite strict {
    every operation where has @permittedWhileSuspended
    must @operationClass == controlWrite
}

except NoDocument for CypherValue because "Cypher parameters are open JSON; the service bounds their size."
```

A clause is `every <kind> [where <condition>] must <condition> ...`, where
each `must` is a requirement, or `no <kind> [where <condition>]`, which
forbids what it matches. A violation prints the rule's `///` text, the
element, and the `must` it failed, at the element's declaration.

**Levels.** `strict`: a violation fails the compile and no package can except
it. No level: it fails the compile, and the package that declares the rule
may except it. `exceptable`: as no level, and a package that imports the rule
may except it too. `advice`: a violation is advice and never fails the
compile; it has nothing to except.

**Exceptions.** `except Rule for Name because "reason"` names a shape, an
operation or a resource; for an operation it also covers the members of its
input and output. It is recorded in the compiled model
(`Service::exceptions`, in the JSON as `exceptions`) and shown by reports. An
exception that excuses no violation is an error, so a fix removes its entry;
so is one with a blank reason, an unknown rule, a strict or advice rule, or a
rule the package may not except.

**Kinds.** `service`, `resource`, `operation`, `error`, `structure`, `union`,
`enum`, `list`, `map`, `string`, `boolean`, `number`, `byte`, `short`,
`integer`, `long`, `float`, `double`, `bigInteger`, `bigDecimal`,
`timestamp`, `blob`, `document`, `shape` (any of the shape kinds), `member`,
`input member` and `output member`. `string` and `number` include the string
and integer enums; `enum` is both. A member is a member of a structure, union,
list (`member`), or map (`key`, `value`).

**Conditions.** Joined with `and`, `or`, `not` and parentheses. Each kind
has the ones that apply to it, and a rule that uses another is an error.

| Condition | On | Meaning |
|---|---|---|
| `documented` | every kind | has non-blank `///` documentation |
| `internal`, `sensitive` | shapes, operations, members | the modifier is present |
| `readonly`, `idempotent`, `public`, `paginated` | operation | the modifier or clause is present |
| `enum`, `referenced` | shapes | an enum; something reaches it (an operation, a resource entry, metadata, `include`) |
| `optional`, `required`, `template` | members | the member is optional, required, or came from a role template |
| `addresses R` | operation | bound to resource `R` or one below it |
| `name`, `role` | any, operation | the element's name; its role word (`reads`, `lists`, ...) |
| `http.method`, `http.uri`, `http.status` | operation | the HTTP binding, the URI as written |
| `type` | members | the target shape's name; `type is string`, `type is inline primitive` |
| `type.length.max`, `type.range.min`, ... | members | the member's effective constraints |
| `length.max`, `range.min`, `pattern` | strings, numbers, lists, maps, blobs | the shape's constraints |
| `has length`, `has range`, `has pattern` | the same, and members | the constraint is present |
| `input contains sensitive`, `output ...` | operation | the body reaches a sensitive member or shape |
| `@key`, `has @key`, `@key == value` | any element the key is declared `on` | metadata, including `retention source` keys |
| `has @token`, `@query`, `@header`, `@payload`, `@deprecated` | members (`@deprecated`: any element) | the built-in annotation is present |
| `has one of @a, @b` | | any of the keys is present |

Operators: `==`, `!=`, `in [a, b]`, `<`, `<=`, `>`, `>=` on numbers, and
`startsWith`, `endsWith`, `contains` and `matches` (a regular expression) on
strings. A word on the right (`controlRead`, `GET`) compares as a string; an
enum key is checked against its values. A shape no operation reaches was
never lowered, so a condition that needs more than its name, kind,
documentation and `referenced` skips it.

Every condition is checked when the rule is read: an unknown key, a value the
key does not have, or an operator its value cannot take is an error in the
rule, not a rule that silently matches nothing.

## Semantics, briefly

The meaning of each construct is in [decisions.md](decisions.md): members
are required unless marked `?` (20), inputs and outputs are anonymous (25),
resources, roles and representations (31-34i), protocols and families
(35-35d), metadata (43), rules, templates and exceptions (45-47f),
migrations (51-59).

## In the editor

`crucible-lsp` explains each keyword, built-in type, constraint and
built-in annotation on hover from one table, `KEYWORDS` in
`crates/crucible-lsp/src/hover.rs`, written from this reference and
[decisions.md](decisions.md). Change it with them: its test parses every
example in it and hovers the word it explains.
