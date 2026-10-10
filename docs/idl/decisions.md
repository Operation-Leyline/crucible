# IDL semantic decisions

One decision per entry, with the Smithy rule it replaces and why. The
language has no syntax yet; these fix what it means. Entries marked
**OPEN** need a product call; the rest are defaults a reviewer can
overturn by editing the line. Counts come from [inventory.md](inventory.md).

Vocabulary used below: *package* is a directory of model files compiled
together; *shape* is any named type; *metadata* is what Smithy calls a
trait when a project defines it.

## Compilation

1. **The language has its own AST, and the compiler lowers it directly to
   `ir::Service`.** The Smithy JSON AST is not an intermediate step.
   *Replaces:* the Java compiler, the JSON AST, and `crucible-smithy`'s
   parse and lowering for new-language packages. *Why:* the AST models the
   language's concepts (an `error` is an error, a resource binds its
   properties positively), so lowering is the mapping of those concepts
   onto IR fields rather than the untangling of trait interactions that
   makes `lower.rs` 2k lines. The semantics `lower.rs` settles (effective
   constraints, binding locations, pagination merge, timestamp defaults,
   audience projection) move into the new lowering; rules that hold of the
   IR itself (route conflicts, binding targets, pagination types, examples,
   name collisions) are checked once, in `crucible_smithy::validate`, which
   both front ends call. The inventory lists them and the fixtures pin them.
   The acceptance test is IR equality: the Smithy path (`Java` +
   `crucible-smithy`) and the new path must produce the same `ir::Service`,
   equal on modelled fields as decision 63 defines, for every model in the
   converted corpus.

2. **There is no converter. Leyline and Pigeon are rewritten by hand as
   new `.cru` models, and `crucible-smithy` stays as the Smithy-to-IR path
   for as long as anything still uses Smithy.** *Replaces:* the earlier
   plan to convert the corpus mechanically. *Why:* the rewrite is the
   point; a mechanical conversion would carry Smithy's shape of the models
   into the new language. Both front ends produce the same IR (decision
   63 says what "same" covers), so the weather model, which exists in both forms, is the equality test, and
   generator goldens cover the rest.

3. **The compiled artifact consumers exchange is a serialized IR with a
   versioned format, not source and not the Smithy AST.** Decision 62 sets
   the versions: `crucible.model/0` while the IR is unstable, then
   `crucible.model/1`. A model package publishes it; services, SDK packages and the CLI
   read it. Source compilation happens in the model package only.
   *Replaces:* `DEP_<NAME>_MODEL` carrying the Smithy AST, and the CLI's
   `model.json` input. *Why:* same layout as today, so
   `crucible_build::model(..)` keeps its shape. The IR gains `serde`
   derives and a stability rule: additive fields within a format version.
   `crucible-smithy` keeps producing the same IR from Smithy sources during
   migration and is removed when the last Smithy model is converted.

3a. **The IR sheds Smithy-shaped fields when the Smithy path is removed.**
   `protocol_tests`, `xml_*`, `renames`, `requirements` and the raw
   `traits` map keyed by Smithy shape ids are reviewed then; custom
   metadata (decision 43) keeps the `traits` map's role under a
   language-native key type. *Replaces:* nothing yet. *Why:* generators
   read these fields; changing them before the migration finishes doubles
   the work.

4. **Diagnostics name source positions and the language's own
   vocabulary.** A lowering diagnostic that mentions a Smithy trait is a
   bug in the compiler's mapping layer. *Replaces:* Smithy's shape-id-only
   diagnostics. *Why:* the first thing authors notice.

## Packages, files and names

5. **A package has one namespace, declared once in its manifest, not per
   file.** Files are containers; a shape is visible to every file in the
   package; declaration order and file order never matter. *Replaces:*
   `$version`, `namespace` and `use` in every file (245 headers in the
   consumers). *Why:* both consumers already treat files as containers and
   say so in comments. Shape ids stay `namespace#Name` in the IR, so
   generated names, policy exceptions and `crucible diff` are unaffected.

6. **The manifest is a `[model]` table in `crucible.toml`.** It names the
   namespace, the language version and imported packages. *Replaces:*
   `smithy-build.json` and the Gradle module. *Why:* one file already
   exists per service and the build helper already selects it.

7. **Crucible's standard shapes are an importable package, not
   built-ins.** `crucible.errors` is imported by name and declares what
   `smithy/errors.smithy` does: `ValidationException` (with its reason
   enum and field list), `InternalServerException`,
   `UnknownOperationException`, `ValidationError` and `InternalError`. It is
   the only bundled package so far; `crucible.auth` and any access-denied or
   throttling errors are added when a model needs them. *Replaces:* the `crucible-traits` model jar and
   `use crucible.errors#ValidationException`. *Why:* the same shapes,
   without a JVM artifact; the generator's "fill the validation error"
   rule keys on shape id as it does now.

8. **Names are the model.** Shape names are PascalCase, members camelCase,
   enum values as written; the compiler checks uniqueness and reserved
   words per target through the existing collision diagnostics. Casing is
   policy, not grammar. *Replaces:* the `CamelCase` linter. *Why:*
   `crucible-policy` already owns casing rules.

9. **Service-local renames are dropped.** *Replaces:* `rename` on
   `service`. *Why:* a single-namespace package has no collisions to
   rename; IR `renames` stays empty.

10. **Member order is source order.** *Replaces:* nothing; Smithy agrees.
    *Why:* generated output depends on it.

## Shapes

11. **First-class declarations: `service`, `resource`, `operation`,
    `structure`, `union`, `enum`, `error`, `list`, `map`, and named
    scalars.** `error` is a structure with a status; `enum` carries its
    values. *Replaces:* `structure` + `@error` + `@httpError`,
    `enum` + `@enumValue`. *Why:* 14 error shapes and 104 enums in the
    consumers; the IR already distinguishes them.

12. **Scalar kinds: `string`, `boolean`, `integer` (32), `long` (64),
    `short`, `byte`, `float`, `double`, `bigInteger`, `bigDecimal`,
    `timestamp`, `blob`, `document`.** All exist in the IR and every
    generator. *Replaces:* the prelude's simple shapes. *Why:* zero
    marginal cost to keep; dropping any would remove it from the IR too.

13. **Integer enums are `enum X: integer { A = 1 }`.** *Replaces:*
    `intEnum`. *Why:* one keyword; the IR keeps `IntEnum` as a separate
    shape.

14. **Sets and sparse collections are dropped.** A list may be `unique`.
    *Replaces:* `@uniqueItems`, `@sparse`, the deprecated `set`. *Why:*
    one `@uniqueItems` in Pigeon, no sparse anywhere; `Sparse` and `Sets`
    stop being capabilities.

15. **Primitives may be used inline as member types; naming a constrained
    scalar is policy, not grammar.** *Replaces:* Leyline's
    `NoPreludeMember` linter. *Why:* the template and the weather model
    use inline primitives; Leyline's rule becomes a `rule` in its policy
    (decision 46).

16. **A scalar declaration carries its constraints and documentation; a
    member may tighten them.** `length`, `range`, `pattern` (ECMA-262),
    `unique`. Range bounds are written `min..max`, `min..` or `..max` and
    are **inclusive at both ends**; the reference states this prominently
    because Rust readers expect `..` to exclude the upper bound. The
    vocabulary stays closed and is validated once, in the server runtime;
    a member's constraint replaces its type's, as in the IR, so the
    compiler warns about one that loosens the type's (`length ..40` on a
    `1..100` type drops the minimum; write `1..40` to keep it);
    an expression language (CEL-style) is deferred until a cross-field rule
    needs it. *Replaces:* `@length`, `@range`, `@pattern` on shapes and
    members with the member laid over the target. *Why:* identical to the
    IR's effective-constraint rule. Research: no API IDL uses range
    literals, so this is a readability bet with no precedent either way;
    Buf's move to CEL answered per-language validator drift, which a
    single-runtime design does not have.

17. **Unit is implicit.** An operation without `input` or `output`, and a
    union arm without a type, carry nothing. *Replaces:* `smithy.api#Unit`
    and `@unitType`. *Why:* the IR has `Shape::Unit`; nobody writes it.

18. **Timestamps default to RFC 3339 `date-time`; a timestamp type may
    declare `format http-date` or `format epoch-seconds`.** The format
    belongs to the type or the member, never to the protocol. *Replaces:*
    `@timestampFormat` plus the per-protocol defaults and binding-location
    rules in lowering. *Why:* every consumer declares one `DateTime`
    shape with `date-time`; AWS protocol defaults go with decision 35.

19. **`internal` and `sensitive` are modifiers on shapes, members and
    operations; `internal` also applies to enum values.** Sensitivity
    belongs to the whole enum: a value's name is public in every generated
    SDK, so `sensitive` on one value is refused. *Replaces:* `@internal`,
    `@sensitive`.
    *Why:* audience projection, MCP and log redaction already consume
    these flags from the IR.

## Nullability

20. **A member is required unless marked optional; a member has exactly
    one of three states.** `name: GraphName` must be present.
    `description?: GraphDescription` may be absent and has no default.
    `population: Count = 0` is non-null on both sides, a sender may omit it,
    and client builders treat it as optional. `?` combined with `= v` is a
    compile error. Requiredness is a validation-layer and type fact; the
    body reader never rejects a message for a missing member, the validator
    does, so a required member can later be loosened without a wire break.
    *Replaces:* `@required`, `@default`, `@clientOptional`, `@input` and
    `@output` and their four-way interaction. *Why:* 1,198 `@required`
    annotations against zero uses of the other three in the consumers.
    The IR fields `required`, `default`, `client_optional` and
    `optional_on_server` are all derivable: `client_optional` is always
    false, which is the client behaviour the TypeScript generator already
    chose deliberately. This is a deliberate difference from the Smithy
    front end, which makes every member of an `@input` structure
    client-optional; decision 63's comparison applies it before comparing. The converter must get the flipped default right
    in every file. Research: every human-authored REST IDL of the last
    decade (TypeSpec, Fern, Avro `T?`, Smithy 2.0 in effect) and every
    language that retrofitted null safety chose required-by-default types;
    protobuf's and Cap'n Proto's "required is forever" is about parser
    enforcement, which this decision avoids; GraphQL's nullable default
    produced its "nullable epidemic". The exposure is a required input
    later made optional: wire-compatible, but a source break in every
    typed SDK, which decision 47d makes the compatibility guard report. Smithy's creator was
    still revising Smithy's guidance on this case in July 2026 (smithy
    #3203), so expect to document it carefully.

21. **Defaults are literals of the member's type, applied by the server
    when the member is absent, and fillable by generators.** The compiler
    checks a default against the type and its constraints; an enum default
    is written as the value's symbol (`size: Size = SMALL`) and lowered to
    its wire value. Unions, structures and (for now) timestamps have no
    default. *Replaces:*
    `@default` with its client-vs-server rules. *Why:* one use in the
    corpus; keeping it costs nothing and the generators handle it.

## Operations and HTTP

22. **An operation declares `http METHOD "/uri/{label}" -> status` in its
    body.** Labels in the URI bind the input members of the same name;
    `{name+}` is greedy; literal query parameters in the URI are kept.
    *Replaces:* `@http` plus `@httpLabel` on 261 members. *Why:* Smithy
    already requires the label member to carry the URI's name, so the
    annotation is redundant; the IR is unchanged.

23. **Query and header bindings are member annotations: `@query("name")`,
    `@header("Name")`, `@payload`.** Output and error members may use
    `@header`. *Replaces:* `@httpQuery`, `@httpHeader`, `@httpPayload`.
    *Why:* 176 uses; names differ from member names, so inference is
    impossible.

24. **Prefix headers, `httpQueryParams` and `httpResponseCode` have no
    syntax in v1.** IR fields remain. *Replaces:* those three traits.
    *Why:* zero uses; the runtimes keep supporting them for fixtures until
    a model needs them.

25. **An operation's input and output are always anonymous bodies
    declared inside the operation; they are never named structures and
    cannot be referenced as types.** The IR and generated type names
    follow a package-wide naming rule in the manifest, default
    `<Operation>Input` and `<Operation>Output`; a package may choose
    `<Operation>Request` and `<Operation>Response` or any other suffix
    pair. Members are shared with `include` (decision 40). *Replaces:*
    `input:`/`output:` targets, `@input`, `@output`, and 183 separately
    named structures. *Why:* an input is not a type, it is the operation's
    parameter list; naming it is a convention, and conventions belong in
    one place. Changing the rule renames every generated input and output
    type, which `crucible-policy` reports as an SDK-source break. The
    converter inlines a Smithy model's input and output structures, using
    `include` where a structure was shared or used directly as an output
    (Leyline's `output: Graph`); those outputs change generated type name,
    which is the one migration-visible rename.

26. **Behaviour keywords on an operation: `readonly`, `idempotent`,
    `public`.** `public` disables authentication for that operation.
    Absence of both `readonly` and `idempotent` means a mutation that is
    not safe to repeat; there is no `mutable` keyword, because clients
    need three states, not two, to decide retries. Under the `http`
    family the compiler checks the keywords against the method: GET and
    HEAD must be `readonly`, PUT and DELETE must be `idempotent` or
    `readonly`, POST and PATCH may be anything. An input with a `@token`
    member makes the operation idempotent without the keyword.
    *Replaces:* `@readonly`, `@idempotent`, `@auth([])`, and Smithy's
    `HttpMethodSemantics` validator (which Leyline promotes to an error).
    *Why:* 99, 70 and 5 uses; `@auth` with a non-empty list has no other
    use; RFC 9110 defines safe and idempotent methods, and the retry
    contract in every runtime reads exactly these flags.

27. **`@token` marks the input member a client fills with a fresh
    idempotency token.** It is an annotation, not a keyword, by the rule in
    decision 27a. *Replaces:* `@idempotencyToken`. *Why:* 40 uses, same
    semantics.

27a. **Keywords versus annotations follow one rule.** A keyword is a
    modifier from a closed set the compiler validates and the IR models as
    a flag: `readonly`, `idempotent`, `public`, `strict`, `internal`,
    `sensitive`, `unique`, `retryable`. An `@annotation(..)` carries a
    value or binds a member to something outside the type system: HTTP
    bindings, `@token`, documentation notes, and all project metadata.
    Annotation arguments are type-checked. *Replaces:* Smithy's "everything
    is a trait". *Why:* research found no comparative evidence for either
    spelling (one 332-developer study saw no consensus), but TypeSpec's
    documented pain is untyped decorator arguments, so the rule fixes the
    boundary and types the arguments rather than picking a side.

28. **Pagination is a block on the operation naming input token, output
    token, items and page size; a service may declare defaults that
    operations inherit.** *Replaces:* `@paginated` on operation and
    service with merge rules. *Why:* 45 uses; the IR's `Paginated` and the
    merge rule are kept exactly.

29. **Errors are listed on the operation and on the service; an
    operation's errors are its own then the service's.** An `error`
    declares its status; fault is client for 4xx and server for 5xx;
    `retryable` and `retryable throttling` are modifiers. Every error
    response carries a wire discriminator independent of status (Crucible
    JSON already sends the shape name in `X-Crucible-Error` and `__type`),
    so two errors with one status stay distinguishable and the compiler
    does not need to forbid shared statuses. Deriving fault from status is
    recorded as an HTTP assumption that a future non-HTTP protocol would
    have to map. *Replaces:* `errors: [..]`, `@error("client"|"server")`,
    `@httpError`, `@retryable`. *Why:* status determines fault in every
    consumer error; the IR's `ErrorInfo` is unchanged. Research: Avro
    (`error`) and Thrift (`exception`) are keyword precedents; Fern's
    name-discriminated errors and Zalando's RFC 9457 guidance both end at
    "status plus a machine-readable name".

30. **Examples are an `example` block on an operation with input, output
    or error values.** *Replaces:* `@examples`. *Why:* lowering validates
    them against the shapes already and OpenAPI and docs emit them.

## Resources

31. **A resource declares only what it is: identifiers, properties and its
    parent.** `resource Graph in Organization { id graphId: GraphId;
    property name: GraphName; ... }`. A child resource names its parent
    with `in` and inherits the parent's identifiers, so `Graph` is
    addressed by `organizationId` and `graphId` without restating the
    first. Root resources are listed on the service as today. A resource
    does not list its operations. *Replaces:* `identifiers`, `properties`,
    the lifecycle slots, `operations`, `collectionOperations`, the
    parent's `resources: [..]` list, and Smithy's rule that a child
    restates every parent identifier. *Why:* both consumers keep one file
    per operation in a folder per resource; a resource-side list means
    every new operation edits two distant files, and Leyline's `Graph`
    would list 13 operations and 4 children in its block.

32. **An operation attaches itself to a resource with a role verb:
    `creates`, `puts`, `reads`, `updates`, `deletes`, `lists` for the
    lifecycle, `on` for any other instance operation, and `on ... collection`
    for any other collection operation.** `operation GetGraph reads Graph
    { .. }`, `operation RevertGraph on Graph { .. }`, `operation
    BatchGetGraph on Graph collection { .. }`. Each lifecycle role may be
    held by one operation per resource. The role implies behaviour and is
    checked: `reads` and `lists` imply `readonly`; `puts`, `updates` and
    `deletes` imply `idempotent`; writing the implied keyword is allowed
    and redundant. The role fixes which identifiers the input must bind:
    instance roles (`reads`, `updates`, `deletes`, `puts`, `on`) bind every
    identifier including inherited ones; collection roles (`creates`,
    `lists`, `on .. collection`) bind only the parent's, and `creates`
    binding the resource's own identifier is an error (that is `puts`).
    An operation belongs to at most one resource; the converter reports a
    Smithy model that binds one operation to several. *Replaces:* the
    resource's lifecycle slots and operation lists, and Smithy's
    lifecycle validators. *Why:* the operation file says everything about
    the operation; the compiler, docs and LSP assemble the resource view.
    This reverses the earlier default ("resource lists its operations")
    after porting Leyline.

33. **Input members attach to the resource only by elision, never by name
    matching.** An operation attached to a resource has an input that is
    implicitly `for` that resource. Inside it, `$graphId` binds the
    identifier and `$name` binds the property, taking type and
    documentation from the resource; the member may add docs, `?`, and
    binding annotations (`@query`), but not change the type. `$graphId as
    id` binds under a different member name. Every member written in full
    (`nextToken?: NextToken`) is a plain input member and nothing more. A
    full member whose name equals a property or identifier is a warning
    ("did you mean `$name`?"), never an implicit binding. *Replaces:*
    `@notProperty` (323 uses), `@resourceIdentifier` (32 uses),
    `@property`, and Smithy's rule that every top-level member must bind or
    be excluded. *Why:* binding is positive and visible at the member;
    Smithy's name-matching inference is what forced the negative
    annotations and produced its worst diagnostics (smithy #2294, #2968).

34. **Outputs and shared structures carry properties through `for`
    types.** `structure GraphData for Graph { $graphId as id; $name;
    $description? }` declares a view of the resource; any output member
    whose type is a `for Graph` structure, or a list or map of one, carries
    those bindings, so `output { graph: GraphData }` and `output { graphs:
    GraphList }` bind with no annotation. An output may also elide
    properties at top level. *Replaces:* `@nestedProperties` (67 uses) and
    the `for` elision on structures. *Why:* list-output binding becomes
    first-class, which Smithy still gets wrong (smithy #2968).

34a. **The IR exposes derived per-property mutability.** A property set by
    the `creates` or `puts` input and not by `updates` is create-only; set
    by `updates` is read-write; appearing only in outputs is read-only;
    appearing in inputs but never in an output is write-only. A
    `@mutability(full | create | createAndRead | read | write)` member
    annotation overrides the derivation; the override is the typed
    `ir::Member::mutability`, which the Smithy front end also fills from
    `@cfnMutability` and the infrastructure generators read. The derivation
    itself lands with infrastructure lowering in the language. *Replaces:*
    Smithy's `aws.cloudformation` derivation and `@cfnMutability`. *Why:* CDK, Terraform and CloudFormation all need
    exactly this list; generators read it instead of recomputing it.

34b. **`references Resource` on a member or structure is kept.**
    *Replaces:* `@references`. *Why:* Pigeon's `WorkspaceScoped` uses it;
    infrastructure lowering reads it.

## Protocols, auth and transport

35. **Protocols are declared data, not keywords. A `protocol` declaration
    names a binding family and its settings; a service lists one or more
    protocols; a runtime registers an implementation under the same id.**
    The standard package `crucible.protocols` declares `json` (family
    `http`). The AWS protocols (restJson1, awsJson 1.0/1.1, RPC v2
    JSON/CBOR, restXml, awsQuery, ec2Query) become declarations in an
    `aws.protocols` package that the runtimes may or may not implement;
    the existing capabilities check reports an unsupported protocol at
    generation time. *Replaces:* `@crucible.protocols#json`,
    `aws.protocols#*` and `@protocolDefinition`. The IR keeps
    `ir::Protocol` as the closed list of protocols the generators and
    runtimes implement: every protocol needs wire-format code, so an open
    id would only trade exhaustive matches for string comparisons. A
    language `protocol` declaration names one of them by id, and its
    family is a property of the implemented protocol
    (`uses_http_bindings`). Settings stay in `ProtocolSettings`. *Why:* one protocol in real
    use, and whether to keep the AWS runtimes stops being a language
    question: dropping one is deleting a declaration and a runtime module.
    The product call on the AWS runtimes stays deferred to after the
    hardening pass.

35a. **Binding families are the fixed vocabularies the runtimes implement;
    the language ships `http` and `rpc`.** `http` provides the operation
    clause `http METHOD "uri" -> status`, member annotations `@query`,
    `@header`, `@payload`, and `status` on errors. `rpc` provides nothing:
    the operation name is the route and every member is body. An operation
    must carry a clause for every family its service's protocols use; a
    clause or member binding for a family no declared protocol uses is an
    error; under `rpc` alone, `@query` and `@header` are errors. Adding a
    family is language plus runtime work, by design. *Replaces:* Smithy's
    rule that HTTP binding traits are permitted everywhere and ignored by
    RPC protocols, and the server's `uses_http_bindings()` special case.
    *Why:* this is how the runtime is already structured (`OperationInfo`
    locations read by whichever `Protocol` claims the request; invoke and
    MCP read none), made explicit and checked. Nothing is silently
    ignored, which is the rule the research found behind every inference
    failure.

35b. **Fault is the core fact about an error; `status` is its `http`
    binding.** A non-HTTP family maps status (RPC v2 honours it) or
    ignores it. *Replaces:* nothing; records the HTTP assumption behind
    decision 29.

35c. **A binding family is syntax plus validity rules plus a runtime; its
    rules are compiler errors, not policy.** The `http` family's rules:
    - GET and HEAD requests carry no body: every input member must be
      bound by URI label, `@query` or `@header`, and an unbound member is
      an error naming the fix, never silently moved to the query string.
      Browser `fetch` throws on a GET or HEAD with a body, so this breaks
      the TypeScript client outright. DELETE follows the same rule by
      default, since RFC 9110 gives DELETE content no defined semantics.
    - HEAD responses, `-> 204` and `-> 304` carry no body: output members
      may only be headers.
    - Success statuses are 2xx; error statuses are 4xx or 5xx.
    - Label members are required scalars or enums (no lists, structures,
      maps or blobs); at most one greedy label per URI.
    - Query members are scalars, enums or lists of them.
    - Header members are scalars (enums and timestamps included, blobs
      and documents not) or lists of them, named by a valid header token; a string with a media
      type travels base64-encoded; framing and hop-by-hop headers
      (`Content-Length`, `Transfer-Encoding`, `Host`, `Connection` and
      similar) and the service's auth header (`Authorization` under bearer
      auth) cannot be modelled members.
    - At most one `@payload`; with a payload, no other member is body.
    - A member has one binding, and a binding appears only where its part
      of the message exists: `@query` in an input, `@header` and
      `@payload` in an input, output or error.
    - Routes are unique per method and path shape.

    Everything above except the request-body rule is checked on the IR by
    `crucible_smithy::validate`, so the Smithy and `.cru` front ends refuse
    the same models. The GET, HEAD and DELETE request-body rule is checked
    by the `.cru` front end only; Smithy models keep the generators' own
    diagnostics for it.

    The `rpc` family's rules: no member bindings at all, operation names
    unique within the service, and the streaming restrictions the protocol
    declares. *Why:* these are correctness facts about the transport. A
    style rule that only usually holds stays in policy: `readonly` should
    use GET is a policy rule, because batch reads legitimately use POST
    (Leyline suppresses Smithy's method-semantics warning on
    `BatchGetGraph`, `BatchGetCapability` and
    `EstimateGraphHistoryQuery`).

35d. **A protocol declaration can narrow its family, never widen it.**
    Settings may restrict which shapes the protocol carries (for example
    "no `document`" for an XML protocol, "no raw streaming blob" for RPC
    v2) and the compiler reports a model that uses a restricted shape
    under that protocol. A protocol cannot add bindings its family lacks.
    *Why:* keeps protocol declarations as data while every rule that
    needs code lives in the family.


36. **`strict` is a structure modifier.** *Replaces:*
    `@crucible.protocols#strict`. *Why:* same semantics, server only.

37. **Auth schemes are declared data like protocols: an `auth scheme`
    names a family the runtimes implement and its settings. A service
    lists its schemes; `public` on an operation opts out; `scopes [..]` on
    service or operation is kept.** The standard package declares
    `bearer`; API key, Basic, Digest and SigV4 are declarations the
    runtimes may implement, checked by capabilities. *Replaces:*
    `@httpBearerAuth`, `@auth`, `@optionalAuth`, `@httpApiKeyAuth`,
    `@httpBasicAuth`, `@httpDigestAuth`, `aws.auth#sigv4`,
    `@crucible.auth#scopes`. *Why:* bearer is the only scheme in use;
    scopes are already enforced by the runtime at no model cost; the
    declaration pattern matches decision 35 so the language has one
    extension mechanism, not several.

38. **Streaming payloads, event streams, host prefixes, endpoint rule
    sets, endpoint discovery, waiters, checksums, unsigned payloads and
    CORS have no syntax in v1.** IR fields remain. *Replaces:* those
    traits. *Why:* zero consumer uses. Each is a later additive syntax
    decision or, with decision 35, a removal from the IR.

39. **Media types are a `mediaType` constraint on blob and string types:**
    `blob Photo { mediaType "image/jpeg" }`. A member cannot set one.
    *Replaces:* `@mediaType`. *Why:* one use; payload content types depend
    on it.

## Reuse

40. **Structures share members with `include Other`; included members are
    copied with their docs and annotations and may be followed by further
    members. Mixins and `apply` are gone.** *Replaces:* `@mixin`, `with
    [..]`, `apply` (93 uses and 27 uses). *Why:* every mixin in the
    consumers is a member bundle on a structure; every `apply` is a
    `@notProperty` workaround that decision 31 removes. Inclusion is
    flattened by the compiler and never reaches the IR, like
    `flattenAndRemoveMixins` today.

## Documentation

41. **`///` comments are Markdown documentation on the following
    declaration; `[Name]` resolves to a shape or operation in the package
    and a dangling link is reported.** It is a warning for now, so a port in
    progress still compiles; it becomes an error once the ported models are
    clean. *Replaces:* `@documentation`, and
    Smithy's unchecked links. *Why:* 5.5k doc lines with cross-links feed
    14 SDKs and the reference site; a broken link should fail at compile
    time, not in a rendered page.

42. **`title` on a service; `@deprecated(since: "..", message: "..")`,
    `@since("..")`, `@unstable` and `@externalDocs("Name" = "https://..")`
    as annotations anywhere documentation attaches.** *Replaces:* `@title`, `@deprecated`,
    `@since`, `@unstable`, `@externalDocumentation`. *Why:* `DocNotes` is
    already rendered by every generator.

## Custom metadata

43. **A package declares metadata keys with a target kind list and a value
    type; application is `@key(value)`; keys reach the IR's raw `traits`
    map as `namespace#key` with their JSON value.** `metadata
    operationClass on operation: enum { controlRead, controlWrite,
    dataPlane, asyncJob }`. Target kinds: `service`, `resource`,
    `operation`, `structure`, `union`, `error`, `enum`, `scalar`, `list`,
    `map`, `member`, `enum value` (annotations on resources are not lowered
    yet). Conflicts are kept, written `conflicts a, b` (decision 65); a
    conflict may name another key, a built-in annotation or a modifier
    (`conflicts sensitive`). Selectors are not. A key's declaration is
    checked once (targets, conflicts, retention, value type, no built-in
    annotation's name) and each use's value against the declared type. *Replaces:* `@trait(selector:
    "..", conflicts: [..])` on a shape used as the value type. *Why:*
    Leyline's six traits need exactly this; `trait_as`, `has_trait` and
    `when_trait` read the same map and keep working unchanged. Research
    adds two optional refinements with precedent: a `retention` flag
    (protobuf custom options) so a key can be compile-time only and never
    reach the runtime, and a per-key `compatibility` rule (Smithy
    `breakingChanges`) so `crucible-policy` knows whether changing the
    key's value is breaking. Both default to "runtime, non-breaking".
    Retention is written `metadata review on operation: string retention
    source`; a source-only key is validated and never written to the IR.

44. **A metadata value type is a scalar, an enum, or a structure of
    those.** *Replaces:* any shape as a trait. *Why:* all six consumer
    traits fit; structured values are what `trait_as` deserializes.

## Guards and policy

> **Deferred (2026-10-10).** Policy stays in `crucible.toml`, read by
> `crucible-policy` for both front ends: a `.cru` package keeps it beside
> `[model]` in its own manifest, each check may set its own severity, and
> the language server reports findings where they occur
> ([api-policy.md](../api-policy.md)). Several rule syntaxes were drafted
> and judged harder to write than the TOML they would replace, so the
> `rule`, `except` and `advice` declarations below remain unimplemented
> (the compiler reports them as not supported yet). Decisions 45-47f record
> the intent should custom rules be needed.

The goal: a service's rules are written once, and a person or a language
model adding an operation or a member cannot produce a model that breaks
them. A violating model does not compile, so no artifact, generated SDK or
server binding exists for it.

45. **Guards are part of the language and cannot be turned off.** Rules are
    `rule` declarations in the package's sources (by convention under
    `policy/`), compiled with the model on every entry point: the build
    helper, `crucible check`, `generate`, `diff` and the language server.
    There is no flag, severity setting or source annotation that disables
    a rule; a violation is a compile error. *Replaces:* `smithy-linters`,
    `metadata validators`, `severityOverrides`, `suppressions`,
    `@suppress`, the opt-in `.policy(..)` on the build helper, and the
    per-group `severity = "off"` in `crucible.toml`. *Why:* a check that
    can be skipped will be skipped by an author under pressure, human or
    model. Advisory rules exist (`advice`), print warnings, and never
    block; a rule is one or the other by declaration, not by the consumer's
    configuration.

46. **Rules use one declarative form over a fixed, typed model surface:
    `every <kind> [where <condition>] must <condition>`, plus `no <kind>
    [where <condition>]`.** A rule may hold several clauses. Kinds:
    `service`, `resource`, `operation`, `error`, `structure`, `union`,
    `enum`, `member`, `input member`, `output member`, and each scalar,
    `list`, `map`. Conditions read: keywords (`readonly`, `public`,
    `idempotent`, `optional`), resource role and scope (`role == lists`,
    `addresses Organization`), metadata (`has @operationClass`,
    `@operationClass == controlRead`, `has one of @a, @b`), types
    (`type == DateTime`, `type is timestamp`, `type.length.max >= 64`),
    names (`name startsWith "List"`, `name matches ".."`), HTTP (`http.method`,
    `http.uri startsWith ".."`), shape (`has length`, `has range`,
    `paginated`, `documented`, `input has member clientToken`, `output
    contains sensitive`), joined with `and`, `or`, `not`. There is no
    general-purpose code, no plugin layer and no network access; rules are
    deterministic. The compiler type-checks rules: an unknown metadata key,
    an enum value the key does not have, or a condition that does not
    apply to the kind is an error in the rule. A rule's `///`
    documentation is the explanation every violation prints. *Replaces:*
    Smithy selectors and `EmitEachSelector`, and the fixed rule groups in
    `crucible.toml` (naming, paths, documentation), which become a
    standard rule package written in this form. *Why:* every rule in
    Leyline's `validators.smithy` and every MUST in the api-standards
    pagination, idempotency, input-validation and common-terms documents
    has this shape; the research shows plugin layers shrink the rule
    ecosystem of native tools, and untyped rules match nothing silently.

47. **Rules flow down from imported packages and cannot be weakened.** A
    package's rules apply to every package that imports it, so an
    organisation standards package written once applies to Leyline and
    Pigeon. An importing package may add rules; it cannot remove, relax or
    shadow an imported rule. An exception is `except Rule for <target>
    because "<reason>"`; it is allowed only in the package that owns the
    rule, or in an importer when the rule is declared `exceptable`. A
    `strict` rule has no exceptions anywhere. Model files cannot contain
    exceptions or suppressions. Every exception is recorded in the
    compiled artifact and listed in every report. *Replaces:* per-repo
    suppressions and `[[exceptions]]` in `crucible.toml`. *Why:* this is
    what makes "impossible" hold against a contributor with write access
    to the service repository: the standard lives in its own versioned
    package (the natural home is the `api-standards` repository, whose
    prose MUSTs become rules), and the service can only pin a version of
    it. Changing a service's own rules or exceptions means editing its
    `policy/` files, which a CODEOWNERS entry can route to the people who
    own the policy.

47a. **Role templates make the common rules true by construction.** A
    service or standards package may declare what every operation of a
    role carries: `template lists { paginated(inputToken: nextToken,
    outputToken: nextToken, pageSize: maxResults); input { @query
    nextToken?: NextToken; @query maxResults?: MaxResults } output {
    nextToken?: NextToken } }`, `template creates { input { @token
    clientToken?: ClientToken } }`. Every operation with that role receives
    the template's members and clauses; restating one is an error and an
    operation cannot remove one. The language server and `crucible
    describe` show the expanded operation. *Why:* a rule rejects a missing
    `clientToken`; a template makes it impossible to forget and removes
    text a model has to get right. *Trade-off:* those members are not
    visible in the operation file, which tooling has to compensate for.

47b. **Diagnostics are written for a repair loop.** Every violation
    carries the rule id, the rule's documentation, the source position of
    the violating element and, where the fix is mechanical, a concrete
    edit. `crucible check --format json` emits them; the language server
    offers the edits as code actions. *Why:* a model adding an operation
    converges on a valid model in one or two iterations only when each
    error states exactly what to change; this is the interface an LLM
    actually uses.

47c. **Guarantees reach generated code.** A metadata key that a `strict`
    rule requires on every operation is generated as a non-optional typed
    accessor (`op.operation_class() -> OperationClass` rather than
    `trait_as(..) -> Option<..>`), so service middleware has no missing
    case to handle because none can exist. *Why:* closes the gap between
    the model guard and the runtime guard; Leyline's rate-limit and audit
    middleware re-check presence at startup today.

47d. **Compatibility is a guard.** *(Implemented, with the baseline a
    frozen copy of the package's sources made by `crucible freeze` rather
    than a compiled artifact: the IR has no serialized form yet.)* The manifest names the baseline
    artifact (the last release); `crucible check` compares against it on
    every compile, not only when someone runs `diff`. Findings are keyed
    to the consumer class they break: wire, JSON document, or generated
    SDK source. A required input becoming optional is wire-compatible and
    an SDK-source break. A breaking change compiles only with an
    `accept breaking <rule> for <target> because ".."` in the owning
    package, recorded in the artifact. *Replaces:* opt-in `crucible diff`
    in CI. *Why:* the api-standards first tenet is "never break backward
    compatibility"; a check that depends on remembering to run it is not a
    guard.

47e. **What guards cannot do.** Rules see the model, not handler code: a
    guard such as "every organization operation authorizes against the
    organization in its path" is split into a model rule (the path binds
    `organizationId`) and runtime middleware that reads it, with 47c making
    the runtime side total. Whoever can edit the standards package can
    change the standard; that is repository governance (CODEOWNERS,
    branch protection, version pins), not language. Rules narrow the space
    of valid models; they do not judge whether a name is good or a
    resource is the right one, which still needs review.

47f. **An unreferenced shape is a compiler warning**; a standards package
    makes it an error with `no shape where not referenced`. *Replaces:*
    the `UnreferencedShape` linter.

48. **Validity checks the compiler owns because Smithy did them and
    lowering assumes them:** shape-id resolution and uniqueness, trait and
    metadata target kinds, enum value uniqueness, HTTP label and URI
    consistency, route uniqueness, header name validity, error status
    ranges, pagination member existence and types, resource identifier and
    lifecycle consistency, recursion through required members, and
    constraint applicability to the type. *Replaces:* Smithy's core
    validators. *Why:* every one of these is a silent assumption in
    `lower.rs` today; the inventory lists them. Those that are facts about
    the IR (routes, bindings, header names, pagination types, examples,
    name and wire-code collisions) live in `crucible_smithy::validate` and
    run for both front ends; the rest are in the language's lowering.

## Still Smithy, on purpose

49. **The IR is unchanged by this language until the Smithy path is
    removed (decision 3a).** Any field the language cannot express stays
    at its default. *Why:* generators, policy, docs, MCP and the server
    read the IR; the language is a frontend that must produce the same IR
    the Smithy path produces for the same model.

50. **Multi-service packages remain legal; generation selects a
    service.** *Replaces:* nothing. *Why:* Leyline has two.

## API versions

Goal: make a backward-incompatible change without breaking clients already
in production, with old SDKs continuing to work unchanged. Prior art: Stripe
(date versions, one implementation at the latest version, per-change
request/response transforms), Cadwyn (the same model for FastAPI), TypeSpec
`@typespec/versioning` (`@added`/`@removed` annotations in one source, still
a preview library at TypeSpec 1.0), Azure `api-version`, Google major
versions in the package. These are from general knowledge, not the research
pass in `reports/`.

51. **Only breaking changes create versions; additive changes apply to
    every version.** A new operation, a new optional input member, a new
    output member or a new error mapped for old versions (decision 55) is
    available to all supported versions at once. A change the
    compatibility guard (decision 47d) reports as breaking compiles only
    as part of a declared version. *Replaces:* 47d's `accept breaking`,
    which becomes "declare a version". *Why:* keeps the number of versions
    proportional to the number of breaking decisions, not releases.

52. *(Implemented for the compile-time check: a migration from the
    baseline's version must declare every breaking change, and each step
    must match one; see [api-policy.md](../api-policy.md). The baseline is
    a frozen copy of the sources. Runtime transforms, decisions 53-59, are
    not built yet.)* **The source is always the latest model; history is a chain of frozen
    artifacts plus declared changes.** Each released version's compiled
    artifact is checked in under `releases/` and never edited. A
    `migration "2026-11-01" from "2026-09-04" { .. }` block declares how to
    serve the previous version from the current one, in a closed set of
    step verbs (`rename`, `require`, `add`, `move`, `reshape`, `remove
    operation`, `behavior`, `sunset`); `by hook` names a pure function and
    `by async hook` one that may do I/O, so old requests that cost an
    extra read are visible in the source. The compiler verifies that applying the declared
    changes to the previous frozen artifact yields the current model, so
    an undeclared breaking change is a compile error and a declared change
    that did not happen is too. *Replaces:* nothing in Smithy (it models
    one version). *Why:* the source stays readable as the current API,
    unlike annotating every member with its history; the frozen artifacts
    are the same baselines decision 47d already needs.

53. **One handler per operation, at the latest version; old versions are
    served by transforms the framework runs around it.** A request at an
    old version is decoded and validated against that version's frozen
    metadata, upgraded change by change to the latest input, handled, and
    the output or error is downgraded change by change to the requested
    version before encoding. Requests at the latest version run no
    transforms. *Why:* N versions do not mean N handlers; each change is
    written once and composes, which is the property that lets Stripe
    support every version since 2011. It is also the only design in which
    a bug fix lands once.

54. **Change kinds are a closed set; most convert automatically, the rest
    generate a typed hook.** Automatic: member renamed; member removed from
    an output whose value old versions can do without (`default` literal);
    optional input made required with a `default` for old requests;
    required output made optional with a `default` for old responses;
    enum value added to an output with a declared mapping for old
    versions; route changed (old route kept as an alias at old versions).
    Hooked: type changed, member split or merged, value computed from
    other state. A hooked change generates a trait such as
    `fn downgrade(&self, out: CreateGraphOutput) -> v2026_09_04::CreateGraphOutput`
    that the service must implement; the generated registry refuses to
    build without it, as it does for a missing operation today. *Why:* the
    compiler can prove the automatic cases; the hooked cases are where
    judgement is needed, and the type system makes them impossible to
    skip.

54a. **The step set is complete by construction, and grows only as
    shortcuts.** Three steps can serve any change between two versions,
    from most specific to most general: `reshape Op.input` /
    `reshape Op.output` with a hook (any shape change whose data is
    derivable, written as one function over the whole old and new types),
    `remove operation` with a legacy handler (anything at all, including
    splitting or merging operations, streaming changes and protocol-level
    changes), and `behavior` (changes with no shape difference). Every
    other verb is a special case the compiler can write itself. A new
    migration type is therefore never a capability gap, only a new
    automatic shortcut for a hook people keep writing; it ships in a
    language minor version, and the compiler then offers to replace
    matching hooks with it. Verbs are additive across language versions,
    so released migrations never need rewriting. A repeated type change is
    declared once as a reusable conversion, `conversion MicroUsd -> Money
    by hook`, and every `retype` step between those types uses it, without
    a new verb. What no migration can fix is a version that needs data the
    current model no longer keeps; that is a product decision before it is
    a modelling one. Hooks are checked structurally (their types must match
    the declared change) but their logic is checked only by the frozen
    version's examples and vectors (decision 59). *Why:* a closed verb set
    is what lets the compiler prove the replay; the general steps keep it
    from ever being a dead end.

55. **Errors and operations are versioned too.** A new error declares how
    old versions see it (mapped to an existing error, or the internal
    error). An operation removed at a version keeps serving old versions
    through a version-scoped handler, the one place two handlers exist.
    *Why:* old clients have exhaustive error enums and generated methods;
    both must keep meaning what they meant.

56. **Behaviour changes branch in the handler, as a last resort.** A change
    to behaviour rather than shape (a list that now excludes deleted
    records) cannot be a transform. The handler reads
    `self.context().api_version()` and compares it with generated version
    constants (`if version < V2026_11_01 { .. }`). The migration declares
    it with `behavior Operation.name`, which generates the constant and a
    changelog entry; a rule can require every version comparison to name a
    version that declares one. *Why:* this is the "branch on
    version" case; it exists, but making it the default would multiply
    handlers and bugs.

57. **The version travels in a protocol-declared carrier; generated
    clients always send theirs.** Crucible JSON uses a request header
    (`Crucible-Version`); the protocol declaration (decision 35) names the
    carrier. A request with no version resolves through a framework hook
    (`Builder::api_version(resolver)`, like `request_id`), so a service can
    pin each account to the version it first used, Stripe-style, or
    default to the oldest supported version. Each SDK release targets
    exactly one API version and sends it on every call; old SDKs keep
    working because the server converts. *Why:* "clients natively handle
    it" without shipping every historical version in 14 SDKs; the SDK's
    major version follows source breaks in its own API version.

57a. **Every supported version keeps its own SDK line, and additive
    changes reach all of them.** Because additive changes apply to every
    version (decision 51), the compiler can produce the current model as
    seen at any supported version by running the migrations backwards,
    which is what the server already does per request. Generators use it:
    an SDK major line exists per supported API version, and its minor
    releases gain new operations, members and fixes without moving to a
    newer API version. Upgrading to a newer API version is an SDK major
    release, accompanied by a generated per-language upgrade guide listing
    every changed symbol, taken from the migration steps and the
    SDK-source compatibility findings. *Why:* a consumer changes code only
    when they choose to move versions, and only where a breaking step
    touches what they use; until then they still receive new features.

58. **Path versions are reserved for major redesigns.** Leyline's `/v1`
    stays a major version; a `/v2` would be a new service in the package,
    not a date version. *Why:* date versions cover evolution; a major
    version is a different API with its own resources.

59. **Versions have a declared support window.** `sunset "2028-11-01"`
    in a migration makes the server send `Deprecation` and `Sunset`
    headers to callers at that version, and refuse after the date if the
    service's policy says so. A standards rule can bound the number of
    supported versions or the minimum window. Every frozen version's
    examples and protocol test vectors run against the server through its
    transform chain as generated regression tests. *Why:* without sunset
    discipline the transform chain only grows; old-version tests come for
    free from the frozen artifacts.

## Implementation decisions

The language is internal while Leyline and Pigeon are its only users, so
these are working choices, changed freely as implementation teaches.

60. **Imports resolve from three places, no network in v1.** Packages
    bundled in the compiler (`crucible.errors`, `crucible.protocols`,
    `crucible.auth`); `[model.dependencies]` entries in `crucible.toml`
    naming a local path; and, under Cargo, model packages found through
    their `links` metadata as today. The dependency entry pins the
    version, so imports name packages without a version suffix. Git and
    registry fetching wait for a second organisation to need them. Built so
    far: bundled packages only (`crucible.errors`). `[model.dependencies]`
    and Cargo discovery are not implemented, and the manifest check refuses
    a `dependencies` key rather than ignore it.

61. **Source positions live in a source map beside the IR.** Lowering
    returns `ir::Service` plus a map from shape and member ids to spans.
    Validity checks run on the resolved model, where spans are at hand;
    rules and compatibility run on the IR and look positions up in the
    map. The same rule engine therefore also checks Smithy-path models
    during migration, reporting shape ids where no span exists. Built:
    `crucible_lang::Compiled::source_map` maps the service, operations,
    shapes, bodies and members to their file and span.

62. **The artifact is serialized IR, explicitly unstable for now.** v1
    will write `ir::Service` with serde under `format: "crucible.model/0"`;
    not built yet, since the IR has no serde derives.
    Freezing artifacts for migrations (decision 52) waits for a stable
    `crucible.model/1`, defined after the IR cleanup of decision 3a, so
    nothing frozen carries Smithy-shaped fields.

62a. **The IR's raw `traits` maps carry project metadata only for
    language models.** Every fact generators used to read from a raw Smithy
    trait is a typed field: `Structure::strict`, `Structure::input`,
    `Member::idempotency_token`, `sensitive` on members and shapes,
    `Member::mutability`, and `Service::is_bearer` for auth scheme ids. The
    Smithy front end fills them and keeps its raw traits, which the
    OpenAPI `x-smithy-traits` export still prints; the language's lowering
    strips every standard trait after using it internally. *Why:* step 1 of
    the IR remodel; generators no longer depend on Smithy's vocabulary.

62b. **Still Smithy-shaped, to remove when the Smithy front end retires:**
    prelude shape ids (`smithy.api#String`) as member targets, `renames`,
    `client_optional`, the XML and AWS-query fields, Smithy protocol tests,
    endpoint rule sets and discovery, and auth scheme ids spelled as Smithy
    trait ids. Each is still produced by the Smithy front end that Leyline
    and Pigeon build with today.

63. **Acceptance is IR equality on modelled fields plus generated-output
    equality.** For every model that exists in both forms (the weather
    model first, then any fixture worth porting by hand), the two paths
    produce equal typed IR fields; raw trait entries for traits the IR
    already models (`smithy.api#http` and the like) are excluded, custom
    metadata must match exactly. End to end, the Rust, TypeScript,
    OpenAPI and docs generators produce byte-identical output from both
    paths; any raw Smithy trait a generator still reads is synthesized by
    the new lowering until that generator reads a typed field instead.
    Two differences are deliberate and applied before comparing:
    `client_optional` is always false from `.cru` (decision 20), and raw
    traits carry project metadata only (decision 62a,
    `Service::project_metadata_only`).
    Like Smithy's `lower`, the `.cru` compile produces the contract IR and
    leaves `Service::resources` empty; resource views come with the
    documentation and infrastructure lowerings, which the language does not
    have yet.

64. **The compiler front end is hand-written, in a new `crucible-lang`
    crate.** A lexer that keeps every byte (whitespace and comments are
    tokens, so the token texts concatenate to the source), a
    recursive-descent parser with recovery at item and member boundaries,
    and a typed syntax tree with byte spans. No parser generator and no
    new dependencies. The lossless token stream is what the formatter,
    language server and fix-its build on.

65. **Grammar choices that the examples left open.** Newlines carry no
    meaning; `;` and `,` are optional separators. Identifiers may contain
    `-` between letters (`date-time`, `epoch-seconds`), but the name of a
    declaration, member or URI label may not: it becomes a symbol in every
    generated language. Metadata conflicts
    are written `conflicts a, b`. `@query` takes an optional name that
    defaults to the member name; `@header` always names its header.
    Pagination merges operation clause over template over service
    default, and two explicit values for one key are an error. Files use
    `.cru`. The grammar is in [reference.md](reference.md).

## Settled in the syntax pass

- Annotations versus keywords: decision 27a. Optionality is `name?:`.
- `include` may target any structure, including one used as a type
  (research: TypeSpec spread allows it with no reported friction).
- `//` is an ordinary comment, `///` is documentation, `////` and longer
  are ordinary comments, as in Rust. No block comments.
