# IDL semantic decisions

One decision per entry, with the Smithy rule it replaces and why (Smithy is no longer an input; decision 66). The
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
   `renames`, `requirements` and the raw
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

14. **Sets are dropped; sparse collections stay.** A list may be
    `unique`. *Replaces:* `@uniqueItems` and the deprecated `set`. *Why:*
    one `@uniqueItems` in Pigeon; the IR, generators and runtimes dropped
    sets with Smithy. Sparse lists and maps (null elements) stay in the IR
    and runtimes because Leyline's result rows hold nulls.

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

24. **Prefix headers, `httpQueryParams` and `httpResponseCode` do not
    exist.** *Replaces:* those three traits. *Why:* zero uses; dropped from
    the IR, generators and runtimes with Smithy.

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
    member makes the operation idempotent without the keyword, and so do
    the `puts` and `deletes` roles (decision 32); `updates` does not, so an
    `updates` operation on PUT writes `idempotent`.
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
    `sensitive`, `unique`, `retryable`, and on properties `readonly` (34a),
    `immutable` and `writeonly`. An `@annotation(..)` carries a value or
    binds a member to something outside the type system: HTTP
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
    first. It inherits identifiers only, as in Smithy, not the parent's
    properties: a child is addressed by its parents but described by its
    own state. Identifier names are therefore unique across the chain,
    since an instance operation binds every one of them; a property name
    must differ from every identifier the resource has, its own and
    inherited, and from its other properties, while properties of
    different resources in a chain are independent (Pigeon's `Workspace`
    and its child `Domain` both have `name` and `createdAt`). `$name` in an
    operation attached to a resource, or in a `for` view of it, names one
    of its own identifiers or properties or an inherited identifier, never
    an ancestor's property. Root resources are listed on the service as
    today. A resource does not list its operations. *Replaces:*
    `identifiers`, `properties`, the lifecycle slots, `operations`,
    `collectionOperations`, the parent's `resources: [..]` list, and
    Smithy's rule that a child restates every parent identifier. *Why:*
    both consumers keep one file per operation in a folder per resource; a
    resource-side list means every new operation edits two distant files,
    and Leyline's `Graph` would list 13 operations and 4 children in its
    block. A namespace shared with every ancestor's properties made Pigeon
    rename `Workspace`'s `name` so that no resource below it could have one.

    *Known limit:* identifiers a collection operation binds come only from
    a parent (decision 32). Smithy allows a root resource whose own
    identifiers include its scope's: Pigeon's `Grant` is a root resource
    identified by `workspaceId`, `principalType` and `principalId`, and
    `ListGrants` binds `workspaceId` alone. The language cannot say that;
    the port writes `resource Grant in Workspace`, which makes `Grant` a
    child in the resource tree where Smithy has it as a root. The
    operation contract is the same; the lowered resource tree (decision 67)
    is not: `Grant` is among `Workspace`'s `resources`, not
    `Service::resource_roots`, and inherits `workspaceId` instead of
    declaring it.

32. **An operation attaches itself to a resource with a role verb:
    `creates`, `puts`, `reads`, `updates`, `deletes`, `lists` for the
    lifecycle, `on` for any other instance operation, and `on ... collection`
    for any other collection operation.** `operation GetGraph reads Graph
    { .. }`, `operation RevertGraph on Graph { .. }`, `operation
    BatchGetGraph on Graph collection { .. }`. Each lifecycle role may be
    held by one operation per resource. The role implies behaviour and is
    checked: `reads` and `lists` imply `readonly`; `puts` and `deletes`
    imply `idempotent`; writing the implied keyword is allowed and
    redundant. `updates` implies nothing about repeating and cannot be
    `readonly`: Smithy's lifecycle requires idempotency of put and delete
    only, and a real update is often not safe to repeat (Pigeon's
    `UpdateTemplate` creates a new version per call), so an update that is
    idempotent says `idempotent`, and one served by PUT must (decision 26).
    The role fixes which identifiers the input must bind:
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
    ("did you mean `$name`?"), never an implicit binding. The warning is
    only for a member `$name` could replace: not one of another type than
    the entry (Pigeon's `SendMessage` takes `to: RecipientList` where the
    `Message` property is `to: AddressList`), and not an identifier of the
    resource itself in a collection operation's input, which decision 32
    forbids binding (a `ListGrants` that filters on `principalType`).
    A member typed by a `for` view binds nothing in an input: the view's
    members are a representation, not writes (34a). *Replaces:*
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

> **Revised 2026-10-11.** 34a and 34c-34i follow three independent reviews
> of a proposal to make a resource its own full representation, which they
> rejected as written. Built: 34a's keywords, checks and lowering, `$*`
> (34d), property presence (34e), identifier spelling (34f) and 34h's two
> warnings. Not built: `references` (34b) and 34c's default-view shorthand;
> 34i is open. The compiler still refuses annotations on resource entries.
> Resources and their bindings are lowered (decision 67).

34a. **Property mutability is declared on the property, from a closed set
    of keywords, and the compiler checks bindings against it.**
    `property name: GraphName` is read-write; `readonly property status:
    DomainStatus` is assigned by the service; `immutable property
    homeRegion: GraphHomeRegion` (Leyline) is set by `creates` or `puts`
    and read-only afterwards; `writeonly property password: Password` is
    written and never returned; `immutable writeonly` is set on create and
    never returned. `immutable writeonly` is the only combination:
    `readonly` combines with neither, as AIP-203 refuses `OUTPUT_ONLY`
    with `INPUT_ONLY`. Read-write, `readonly`, `immutable`, `writeonly` and
    `immutable writeonly` lower to the `ir::Mutability` variants `Full`,
    `Read`, `CreateAndRead`, `Write` and `Create`; identifiers are
    immutable and take no keyword. What was a derivation is now validation:
    - an `updates` input binding a `readonly` or `immutable` property is an
      error, and so is a `creates` or `puts` input binding a `readonly` one;
    - an `immutable` property that no `creates`, `puts` or `on ..
      collection` input binds is an error: nothing could ever set it;
    - a property that is neither `readonly` nor `immutable` and that no
      `updates`, `puts` or instance `on` input binds (outside `@query`) is a
      warning ("declare it `immutable`"): left read-write, the
      infrastructure generators would offer an in-place update nothing
      implements, where CloudFormation treats a missing update handler as
      replacement (Pigeon's `ApiKey`,
      `Thread` and `Domain`, Leyline's `ImportJob` and `QueryJob` have no
      `updates`);
    - a `puts` that changes an `immutable` value on an existing resource
      replaces the resource, the meaning of PUT (RFC 9110) and of a
      create-only change in CloudFormation, CDK and Terraform; a handler
      that cannot replace reports a conflict, which is its error, not a
      model fact;
    - a `writeonly` property in a `for` view or an output elision is an
      error, and `$*` (34d) leaves it out;
    - a binding under `@query`, or in a `reads`, `lists` or `deletes`
      input, selects or filters; it is not a write and is not checked;
    - an `on` operation's input is not checked: a custom action may change
      state the lifecycle cannot (a suspend sets a `readonly` status). Its
      bindings still count. An instance `on` binding is a change after
      creation, so a property a custom action changes is not create-only
      (Leyline's `groupId` changes only through `PutGraphGroupMembership`).
      An `on .. collection` binding is a create, since a custom collection
      action makes instances (Pigeon's `CreateApiKey`, 34g): it sets an
      `immutable` property and carries mutability like a `creates` input;
    - a `for` view used as a member type in a `creates`, `puts`, `updates`
      or `on` input binds nothing and its keywords are not checked; the
      compiler warns, since a write is bound with `$name` where the check
      can see it.

    Lowering writes the property's mutability to `ir::Member::mutability`
    on every member that carries the property's value: view and output
    members and the bindings in `creates`, `puts`, `updates` and `on ..
    collection` inputs.
    Filter bindings carry none, since the infrastructure generators hide
    a `Write` or `Create` member and every shape containing one
    (`write_only_shape`), and a filter is not the property. Smithy fills
    the field only from `@cfnMutability`, so this is a deliberate
    front-end difference (decision 63), and `ir::Member::mutability`'s doc
    comment ("`None` leaves it to the resource lifecycle") changes with
    it. The `@mutability(full | create | createAndRead | read | write)`
    member annotation, whose lower-case spellings are the annotation's and
    not the IR's, stays for members that bind no property (Pigeon's
    `localPart`, a create-time parameter of `CreateInbox` from which the
    service derives the `readonly` address, is `create`); on a member that
    binds a property it is an error when it disagrees with the keyword.
    *Replaces:* the earlier 34a, which derived mutability from which inputs
    and outputs bind a property, and Smithy's `aws.cloudformation`
    derivation and `@cfnMutability`. *Why:* the derivation turns circular
    once a view can expand (34d): `$*` takes the readable properties,
    readability was inferred from appearing in outputs, so adding an
    operation could change an existing representation. It also could not
    separate create-only from write-only, which are two facts, and it
    called everything a `puts` sets create-only though a repeated put
    replaces state. A declared keyword is one fact that validation, `$*`,
    and the CDK, Terraform and CloudFormation generators all read, as
    AIP-203 field behaviours and OpenAPI `readOnly`/`writeOnly` declare it.
    The two binding checks keep what the derivation got right: a property
    no update writes is not offered as updatable. Mutability describes
    the contract, not authorization: who may change a field stays in
    handlers and middleware.

34b. **`references Resource` on a member or structure is kept.**
    *Replaces:* `@references`. *Why:* Pigeon's `WorkspaceScoped` uses it;
    infrastructure lowering reads it. *Known limit:* decided but not built:
    the grammar has no `references` yet, so a port drops it (it changes no
    wire contract) until infrastructure lowering arrives with it.

34c. **A resource is not a representation: it declares identity and the
    property schema, and `for` views remain the representations.** A
    resource's name is never a type; outputs keep naming views
    (`InboxData`), so no generated type is renamed. *Replaces:* the
    proposal to make the resource the full representation (`output {
    inbox: Inbox }`) with views only for subsets. *Why:* one resource has
    several representations (a list summary, the detail, a create result
    that adds a secret, 34g), and mutability cannot tell a public summary
    from a privileged detail, since both are reads. Renaming `InboxData` to
    `Inbox` changes a public symbol and every member target naming it in
    every SDK, which the compatibility guard (47d) reports though the JSON
    is unchanged. And a resource used as its own representation grows with
    every property, the exposure the proposal rejected for subtractive
    views. The duplication it set out to remove goes with `$*` (34d)
    instead. *Deferred:* a bare resource name as shorthand for a declared
    default view (`inbox: Inbox` meaning `InboxData`, keeping the view's
    SDK name), once a resource can say which view is its default.

34d. **`$*` in a `for` view expands to the resource's own identifiers and
    every readable property.** `structure InboxData for Inbox { $*;
    $lastOtp? }`. Identifiers come first under their representation name
    (34f), then every property that is not `writeonly`, in property
    declaration order, each with the property's presence (34e). An entry
    `$*` includes may be named again only as `$x?` after `$*`, which
    overrides its presence in this view and keeps its position; `$x`
    without `?`, `$x as y`, or any mention before `$*` is an error
    ("already included by `$*`"). Every other entry lands where it is
    written.
    Inherited parent identifiers are not included: a view that carries
    `workspaceId` writes `$workspaceId`. `$*` appears at most once, and
    only in a `for` view: an input binds what the operation takes, not the
    resource's state. Subset views stay opt-in (`structure InboxSummary for
    Inbox { $inboxId; $address }`), and there is no subtractive form.

    Exposure: a full view grows with every property, so declaring a
    property is the opt-in to exposing it wherever `$*` is used; a new
    property appears in each full view and in no subset. A subtractive
    view would put it into every summary, public ones included, without
    anyone deciding that; opt-in subsets keep it a decision per view. The
    risk left is a list output whose items are a full view, where one new
    property reaches every item of every page (34h).

    Order and SDKs: the compiler expands `$*`, so the IR sees ordinary
    members in `$*` order (decision 10). Adopting `$*` in an existing view
    whose list ordered members differently keeps the wire and documents
    unchanged but reorders positional constructors in the SDKs that
    generate them, so an existing view may keep its explicit list, which
    stays valid. The compatibility guard (47d) does not report a reorder
    today; adding it to the SDK-source class would make this mechanical.
    *Replaces:* listing every property a second time in its view (Pigeon's
    `MessageData` restates all 25 of `Message`'s). *Why:* most resources
    have one full representation; `$*` keeps one list, and opt-in subsets
    keep exposure a decision. TypeSpec offers the same pair: `Read<T>` for
    the full read shape, `PickProperties` for a subset.

34e. **A property declares its presence: `property lastMessageAt?:
    DateTime`.** It is the property's default in `$*` and in every view, so
    `$lastMessageAt` in a view is optional without the `?`. `$x?` loosens a
    required property for one view; nothing tightens an optional one, so a
    property present in every view but one is required and loosened there.
    Input elisions keep per-site presence (decision 33): `$x` in an input is
    required and `$x?` optional whatever the property says, because a
    create may require what reads leave optional. *Replaces:* the `?` on
    view entries (Pigeon's `$lastMessageAt?`, `$lastOtp?`). *Why:* whether
    state can be absent is a fact about the resource; whether an operation
    asks for it is a fact about the operation.

34f. **An identifier may name its representation spelling: `id inboxId:
    InboxId as id`.** In views and output elisions of `Inbox` the
    identifier appears as `id`: `$inboxId` written there, and `$*`, produce
    the member `id` without a per-view `as`. Inputs still bind it as
    `inboxId`, the URI label's name. The spelling belongs to the declaring
    resource's own representations only: in a child's views an inherited
    identifier appears under its identifier name unless written with `as`,
    so Leyline's `Graph` and its child `Index` can both show `id` (the
    graph's and the index's), as can Pigeon's `AuditEventData` under
    `Workspace`. A representation name must therefore be unique among the
    entries the resource's own views can show (its own entries and
    inherited identifiers under their identifier names), not across the
    chain, which holds only for identifier names (31). Composite
    identifiers keep their own names: Pigeon's `Grant` (`principalType`,
    `principalId`, `resourceType`, `resourceId`) has no single `id` to
    invent. *Replaces:* declaring an identifier beside a `property id` of
    the same type (six Pigeon resources, each with a comment explaining it)
    and Leyline's per-view `$graphId as id` (decision 34's example);
    Smithy's `@resourceIdentifier` (32 Leyline uses) mapped the same pair
    from the member side. *Why:* `id` and `inboxId` are one value; two
    declarations can drift in type or docs and each needs a mutability.

34g. **Values the server generates and returns once are not properties.**
    An API key's secret and a webhook's signing secret are made by the
    service and shown only by the create; they stay plain members of that
    output beside the view (`output { key: ApiKeySecret; apiKey:
    ApiKeyData }`). They are not `writeonly`, which is a value the client
    writes. Decision 32 puts no constraint on a `creates` output, so such
    an operation can hold the role; Pigeon's `CreateApiKey` and
    `CreateWebhook` are `on .. collection`, a choice their comments trace
    to Smithy's `create` slot. *Replaces:* the proposal's write-only
    `secret` property on `ApiKey`. *Why:* nothing reads, updates or
    filters them; they have no life as resource state.

34h. **Two warnings catch a resource modelled beside its
    representation:** a resource holding `reads`, `lists` or `creates`
    (32) whose operations' outputs bind none of its properties, through a
    `for` view or a top-level elision (34), and a structure without
    `for` that restates, by name and type, an identifier and a property of
    one resource ("did you mean `structure GrantData for Grant`?").
    *Replaces:* nothing; Smithy's property validator forced bindings only
    negatively. *Why:* a plain structure works on the wire, so nothing
    fails when a resource declares no properties; it silently loses `$`
    binding, mutability and infrastructure lowering. The first warning
    finds five Pigeon resources, `ApiKey`, `AuditEvent` and `Webhook` with
    no properties and `Grant` and `Member` with one, whose outputs use plain
    structures left from the Smithy model. Considered, not decided: an
    advisory warning on a `lists` output whose items are a `$*` view.

34j. **A view may serve several resources: `structure JobData for
    QueryJob, GraphHistoryQueryJob`.** Two resources that hold the same
    state (a live job and its retained history, a record and its archive)
    show it through one view, and operations of either use it as their
    output. The view is lowered against the first resource, and each
    `$name` is lowered again against every other; the two members must be
    identical (name and spelling, target and constraints, presence,
    default, mutability), or the compiler names the fields that differ. An
    identifier spelled differently (`as id` on one) is such a difference,
    settled with `$jobId as id`. `$*` shows what every listed resource
    declares as an own entry of the same kind and leaves out the rest, and
    a member that is not identical still fails. An included view keeps
    checking its own resources, and the including view's when both are views
    of the same first resource. A resource of the list counts for the
    lifecycle warnings (34h) as any `for` view of it does. *Why:* two views
    kept equal by hand drift, and the one view for the second resource left
    the first with a "no output shows a property" warning. *Rejected:* a
    view that takes the union or intersection of differing members
    silently; a shared member that means two things is a model error.

34i. **OPEN: update semantics.** Clearing an optional property, merge
    versus replace, and field masks are left to their own decision. `$x?`
    in an `updates` input says the member may be absent, not what absence
    does. Decision 20 defines required, optional and defaulted members but
    not what an explicit null means for an optional input: the Rust wire
    layer keeps absent, null and value apart (`Field`), clients keep an
    explicit null distinct from absence ([clients.md](../clients.md)), and
    a protocol that omits null members reads one as absence (`NullAbsent`),
    so the model cannot yet say "leave unchanged" apart from "clear".
    Pigeon's `UpdateTemplate` shows both in one input: an omitted `name` is
    kept, while giving any of `subject`, `text` or `html` replaces all
    three, so an omitted `html` removes it; only its documentation says so.
    Candidates are a `merge`/`replace` declaration per operation or member,
    an explicit clear (JSON Merge Patch's null, which would need every
    protocol to keep null distinct), and an update mask (AIP-134); each
    needs runtime support. *Why open:* the choice changes every SDK's update builders, and 34a's
    validation holds whichever is chosen.

## Protocols, auth and transport

35. **Protocols are declared data, not keywords. A `protocol` declaration
    names a binding family and its settings; a service lists one or more
    protocols; a runtime registers an implementation under the same id.**
    The standard package `crucible.protocols` declares `json` (family
    `http`). `json` is the only protocol; a Smithy model declaring
    any other is refused with "protocol X is not supported". *Replaces:* `@crucible.protocols#json`,
    `aws.protocols#*` and `@protocolDefinition`. The IR keeps
    `ir::Protocol` as the closed list of protocols the generators and
    runtimes implement: every protocol needs wire-format code, so an open
    id would only trade exhaustive matches for string comparisons. A
    language `protocol` declaration names one of them by id, and its
    family is a property of the implemented protocol. Settings stay in `ProtocolSettings`. *Why:* one protocol in real
    use. The AWS and Smithy RPC v2 runtimes were deleted rather than
    kept behind a declaration.

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
    binding.** A non-HTTP family maps status or
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
    unique within the service. *Why:* these are correctness facts about the transport. A
    style rule that only usually holds stays in policy: `readonly` should
    use GET is a policy rule, because batch reads legitimately use POST
    (Leyline suppresses Smithy's method-semantics warning on
    `BatchGetGraph`, `BatchGetCapability` and
    `EstimateGraphHistoryQuery`).

35d. **A protocol declaration can narrow its family, never widen it.**
    Settings may restrict which shapes the protocol carries (for example
    "no `document`") and the compiler reports a model that uses a restricted shape
    under that protocol. A protocol cannot add bindings its family lacks.
    *Why:* keeps protocol declarations as data while every rule that
    needs code lives in the family.


36. **`strict` is a structure modifier.** *Replaces:*
    `@crucible.protocols#strict`. *Why:* same semantics, server only.

36a. **`sparse` is a list and map modifier.** `sparse list Row: Cell`
    lets an element be null, `sparse map Params: Name -> Cell` a value;
    without it a collection is dense and refuses a null. *Replaces:*
    Smithy's `@sparse`. *Why:* a Cypher result row holds nulls, and a
    dense list makes the server fail the response. Changing a collection
    between dense and sparse is a compatibility finding (a response
    that gains nulls breaks clients built for dense rows; the generated
    element type changes for SDKs).

37. **Auth schemes are declared data like protocols: an `auth scheme`
    names a family the runtimes implement and its settings. A service
    lists its schemes; `public` on an operation opts out; `scopes [..]` on
    service or operation is kept.** The standard package declares
    `bearer`, the only scheme; API key, Basic, Digest, SigV4 and optional
    authentication were dropped with Smithy. *Replaces:*
    `@httpBearerAuth`, `@auth`, `@crucible.auth#scopes`. *Why:* bearer is the only scheme in use;
    scopes are already enforced by the runtime at no model cost; the
    declaration pattern matches decision 35 so the language has one
    extension mechanism, not several.

37a. **Typed permissions: resources declare them, operations require
    them, a service `Authorizer` decides.** *(Implemented: language, IR
    and checks; runtime and generators follow. The design and its prior-art
    survey are [permissions.md](../permissions.md).)* A resource lists
    `permission symbol = "value"` entries; an operation lists `requires p
    on Target [when ..] [by handler]`, `filters p on R`, `requires none` or
    `requires pending because ".."`. The IR carries `Service::permissions`,
    `Service::permission_types` and `Operation::requirements`, every field
    `#[serde(default)]`. *Replaces:* hand-written `guard::allow` calls and
    copies of the permission registry. *Why:* both consumers authorize
    every request against a permission on a typed resource, and the model
    could not say which. The settled questions follow as 37b-37k.

37b. **Implication stays in service policy** (permissions.md decision 1).
    No implication syntax and no `*` permission; the per-resource enums
    make a role table an exhaustive match. A `*` is only ever an instance
    wildcard in a binding, `R(*)`, and only for the resource's own
    identifier.

37c. **The wire `ResourceType` enum stays hand-written and
    `@permissionTypes` checks it both ways** (decision 2); permissions are
    strings on the wire; a rename keeps the old value as a `@deprecated
    @aliasOf(primary)` permission, and requirements name primaries.

37d. **No freshness syntax** (decision 3): the Authorizer receives the
    operation's facts and decides.

37e. **Strict default** (decision 4): once a service declares any
    permission, an authenticated operation without `requires`, `filters`,
    `requires none` or `requires pending` is the error
    `permissions.undeclared`, not a configurable policy finding. A pending
    reason is required and warned about on every build.

37f. **Hidden is the framework `crucible.errors#NotFoundException`**
    (decision 5), which lowering adds to the errors of every operation with
    a requirement, unless every clause is on a root resource with `*`; an
    operation that also declares its own 404 is warned about.

37g. **`by handler` is declared in the model** (decision 6): it names the
    permission and resource, takes no bindings or `when`, and becomes an
    obligation the handler discharges.

37h. **No bindings into lists or maps** (decision 7): a batch operation
    authorizes on a shared parent or `by handler`; per-element checks are
    deferred.

37i. **Names are declared or derived once** (decision 8): the value is
    always written and follows a grammar (lowercase-led segments joined by
    `:`, hyphens inside a segment); the authorization name is the shape
    name in lowerCamelCase unless `@authorizationName` sets it; changing
    either, or removing a permission or an alias, breaks the release
    guard (`compatibility.permissions`).

37j. **Four tiers** (decision 9): public, `@permissionOnly`,
    `@notGrantable`, and `internal`, which the client audience projection
    drops with its aliases and the requirements naming it.

37k. **Scopes stay out of permission syntax and the order is fixed**
    (decisions 10 and 11): adding a requirement to a released operation is
    reported as an authorization change (`compatibility.authorization`, a
    warning), not refused, because it is how holes get closed. Permissions
    are declared in `.cru` only: the Smithy front end lowers none and
    refuses a model that uses a `crucible.auth` permission trait, so
    Leyline, the one Smithy consumer, migrates to `.cru` before adopting
    them. `crucible_smithy::validate::permissions` checks the IR.

38. **Streaming payloads, event streams, host prefixes, endpoint rule
    sets, endpoint discovery, waiters, checksums, unsigned payloads and
    CORS do not exist.** *Replaces:* those traits. *Why:* zero consumer
    uses; removed from the IR, generators and runtimes with Smithy.

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

    Members are copied with their own docs and annotations, but the
    included structure's documentation is not: it describes the bundle,
    not the structure that includes it. Smithy's mixin flattening copies a
    mixin's doc onto every user unless the user has its own, which is
    usually wrong there and is a deliberate difference (decision 63).

    HTTP bindings and `@token` on an included member are checked where the
    member lands. A structure nothing uses as a member's type (or as a list
    or map member, a metadata value or a resource entry's type) is a bundle
    of members, not a nested shape, so its `@query`, `@header`, `@payload`
    and `@token` members are not judged on the structure alone: included
    into an operation input they bind there, which is how shared parameters
    such as `PaginationInput { @query nextToken?: string; @query limit?:
    integer }` are written. Where a binding lands with no place for it (a
    `@query` included into an output or error, any binding included into
    a structure that is used as a type, or in a structure used as a
    member's type itself) it would be ignored, and it is an error at the
    member, naming the structure that included it. *Why:* Pigeon shares
    its pagination parameters across list operations this way; Smithy
    allows it through mixins.

## Documentation

41. **`///` comments are Markdown documentation on the following
    declaration; `[Name]` resolves to a shape or operation in the package
    and a dangling link is reported.** It was a warning while ports were in
    progress, and is an error since decision 80, which also says what a
    link resolves to and how each output renders it. *Replaces:* `@documentation`, and
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
    `map`, `member`, `enum value` (metadata on resources is not lowered
    yet; their documentation notes are, decision 67). Conflicts are kept, written `conflicts a, b` (decision 65); a
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

> **Implemented (2026-10-15).** `rule` and `except` compile, with the
> vocabulary, levels and exception checks of decisions 46, 47 and 47g, and
> run on the lowered model on every compile. The policy in `crucible.toml`
> (`[rules.naming]` and the rest, read by `crucible-policy`; see
> [api-policy.md](../api-policy.md)) remains for the checks it already
> covers; a package may use both. Imported packages' rules (47) and role
> templates (47a) are implemented too. The text of 45 on rule severities now
> reads `strict`, none, `exceptable` and `advice`.

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

47a. **Role templates make the common rules true by construction.** *(Implemented: `crates/crucible-lang/src/templates.rs`.)* A
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

47d. **Compatibility is a guard.** *(Implemented. The baseline is a
    snapshot of the compiled model made by `crucible freeze`, checksummed
    in `releases/crucible.lock`; see [api-policy.md](../api-policy.md#releases).)* The manifest names the baseline
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

47g. **How rules and exceptions behave, as built.** A rule's level is
    `strict` (never excepted), none (the declaring package may except it),
    `exceptable` (an importer may too) or `advice` (reported, never fails,
    nothing to except). Rules run once the model compiles without errors,
    over the elements the compiled package declares: an imported package's
    shapes were judged when that package compiled. An exception names a
    shape, operation or resource by name, and for an operation it covers the
    members of its input and output; it is recorded in the compiled model
    (`Service::exceptions`). An exception that excuses nothing is an error,
    as is one for an unknown rule, a blank reason, a repeat, or a rule the
    package may not except. Each condition is checked against its kind when
    the rule is read, and `retention source` metadata is readable by rules and
    never reaches the IR. A shape no operation reaches is never lowered; it
    is a subject for `referenced`, `documented`, `name` and its kind, and any
    other condition skips it rather than guess. *Why:* a rule that matches
    nothing silently, or an exception nobody removes, is how a policy rots.

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
    must match one; see [api-policy.md](../api-policy.md). Each release is
    a snapshot of its compiled model, never recompiled. Decisions 53-56 are built for Rust
    services, with the version carried in a header (57); see
    [api-versions.md](../api-versions.md). Clients send their release
    (57, phase 5a). Projections and SDK lines per release (57a) are
    settled in decisions 68-79; the projections are computed in
    crucible-ir, not yet checked or served; declared `conversion`s and
    example replay (59) are not built yet.)* **The source is always the latest model; history is a chain of frozen
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
    splitting or merging operations and protocol-level
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

57a. *(Settled for implementation in decisions 68-79: releases are
    projections of the current model, checked against their frozen
    releases.)* **Every supported version keeps its own SDK line, and additive
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
    in a migration makes the server send a `Sunset` header (RFC 8594) to
    callers at that version (a `Deprecation` header needs a deprecation
    date the model does not record yet), and refuse after the date if the
    service's policy says so (not built yet). A standards rule can bound the number of
    supported versions or the minimum window. Every frozen version's
    examples run against the server through its
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
    far: bundled packages (`crucible.errors`) and path dependencies:
    `"org.standards" = { path = "../standards" }` names the package by its
    namespace (the directory must declare it) and a package that lists it in
    `imports` loads it, and what it imports in turn, in the namespace of its
    own. Version pins and Cargo discovery are not implemented; the manifest
    check refuses an entry with any key but `path`.

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
    `client_optional`, and auth scheme ids spelled as Smithy
    trait ids. Each is still produced by the Smithy front end that Leyline
    and Pigeon build with today. *Closed by 62c.*

62c. **The built-in vocabulary is `crucible.api`; nothing in the IR is
    Smithy-shaped.** The built-in types are `crucible.api#String` and the
    rest, the bearer scheme is `crucible.api#httpBearerAuth`, and the
    standard traits the lowering records while it works (`crucible.api#http`
    and the like) share the namespace; `project_metadata_only` strips them,
    with `crucible.protocols`, before the IR leaves the compiler.
    `Service::renames` and `Member::client_optional` are gone: `.cru` has no
    rename and a member's optionality is one fact for client and server
    (decision 20). `project_metadata_only` no longer treats `aws.*` and
    `smithy.test` as standard, since only Smithy models used them; a
    package in such a namespace keeps its own traits. `crucible.api` is
    reserved, and so is `smithy.api`, which older snapshots, infrastructure
    bindings (a provider context `target`, mapped on read) and the
    `when_trait` shim still use. Release snapshots move to format 5:
    reading an older one maps each `smithy.api#..` id to its `crucible.api`
    id in memory as it deserializes the IR, so exactly the positions the
    IR's types declare as ids move and free-form text (a version, a title,
    a default, an example, documentation, a raw trait's value) never does,
    and drops
    `client_optional` (a Smithy-era release reads its client-optional
    members with the server's optionality); the file, and so its lock
    digest, is never rewritten, and the compatibility guard compares the
    mapped release with the current model. OpenAPI and JSON Schema
    extensions are `x-crucible-*`, and `describe.json` keeps
    `clientOptional`, always `false`, for `crucible.describe/1`'s readers.
    The server runtime's `when_trait("smithy.api#readonly")` and
    `"smithy.api#idempotent"` (and their `crucible.api` spellings) keep
    working for one minor, deprecated in favour of `.when(|op| op.readonly)`
    and `.when(|op| op.idempotent)`; any other built-in id selects nothing,
    with a warning. When the shim goes, a built-in id passed to `when_trait`
    panics as the scope is built.
    *Why:* `.cru` is the only modelling language since the Smithy front end
    was removed, and its own vocabulary should not name another IDL. The
    lowering still keeps standard facts as raw `crucible.api` traits while it
    works; replacing that scaffolding with typed values is internal to
    `crucible-lang` and changes no output.

63. **Acceptance is IR equality on modelled fields plus generated-output
    equality.** For every model that exists in both forms (the weather
    model first, then any fixture worth porting by hand), the two paths
    produce equal typed IR fields; raw trait entries for traits the IR
    already models (`smithy.api#http` and the like) are excluded, custom
    metadata must match exactly. End to end, the Rust, TypeScript,
    OpenAPI and docs generators produce byte-identical output from both
    paths; any raw Smithy trait a generator still reads is synthesized by
    the new lowering until that generator reads a typed field instead.
    These front-end differences are deliberate, and the comparison applies
    or avoids them before comparing:
    - `client_optional` is always false from `.cru` (decision 20);
    - raw traits carry project metadata only (decision 43; decision 62a,
      `Service::project_metadata_only`);
    - `api_version` is the service's `version` from `.cru`, the release its
      clients send, and absent from Smithy;
    - a `@token` member makes an operation idempotent without the keyword
      (decision 26), where Smithy needs `@idempotent` written;
    - `include` does not copy the included structure's documentation onto
      the includer (decision 40), where Smithy's mixin flattening does;
    - every member that carries a property's value has `mutability` set
      from the property's keyword (decision 34a), where Smithy sets it only
      from `@cfnMutability`; the weather comparison's normalization
      (`as_cru_lowers_it` in `weather_ir.rs`) fills it on the members
      `weather.cru` binds to `City`'s properties.
    Like Smithy's `lower`, the `.cru` compile first produced the contract
    IR and left `Service::resources` empty; it lowers resources since
    decision 67, without adding a shape to the contract.

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

66. **The Smithy front end is removed (supersedes decisions 2, 3, 3a and 49
    where they keep a Smithy path).** Leyline and Pigeon are on `.cru`, so
    the Smithy JSON AST front end, `Model::from_json`, the JVM/Gradle build
    driver, the traits jar and the `smithy-to-cru` converter are gone, and
    `.cru` is the only input. `crucible-smithy` keeps the IR, the snapshot
    format and the shared validators (it is renamed `crucible-ir` later).
    Features nothing used left with it: `@crucible.auth#scopes` and OAuth
    scopes, `rename`, timestamp defaults, enum `mediaType` and 2xx error
    statuses. *Why:* two front ends doubled every language change for no
    consumer. The acceptance test of decision 63 is retired with the second
    front end; the generator goldens and the `tests/models` packages carry
    it. The documentation, CDK, Terraform and Explorer generators need a
    `.cru` package with resource lowering (decision 63 notes the language has
    none yet), so their resource-dependent tests are `#[ignore]`d until it
    lands (decision 67 landed it, and they run again). The remaining
    mentions of Smithy in this file are history.

67. **Every compile lowers the service's resources, and a type only a
    resource names stays out of the shapes.** Each resource the service
    reaches, a root it lists in `resources [..]` or a descendant through
    `in`, becomes an `ir::Resource`: its identifiers, its ancestors' and its
    own (decision 31), and its own properties, each keyed by name with its
    resolved type;
    its children the service reaches, sorted; its doc comment; and the
    documentation notes `@deprecated`, `@since`, `@unstable` and
    `@externalDocs`, which a resource now takes beside
    `@authorizationName` (other annotations and metadata on a resource are
    still refused). The roles of the operations attached to it (decision
    32) fill the lifecycle: `creates`, `puts`, `reads`, `updates`,
    `deletes` and `lists` the slots of those names, `on` its `operations`
    and `on .. collection` its `collection_operations`, each sorted. Only
    operations that lowered count. `Service::resource_roots` is the
    service's list, sorted, so audience projection, which keeps the
    resources reachable from the roots and clears `Operation::resource`
    for the rest, keeps every one. `resource_references` stays empty
    until `references` (34b) is in the grammar.

    Bindings are per attached operation and positive, as decisions 33 and
    34 define them: member lowering records which entry each `$name`
    lowers to, and the paths name the lowered members, so `$bookId as
    book` binds `bookId` at `book` and an identifier spelled `as id` (34f)
    binds at `id` in a view or output. An input binds its own elisions,
    those of the structures it includes among them; a member typed by a
    view binds nothing there. An output binds its own elisions, then
    `[member, viewMember]` for a member typed by a structure that carries
    the resource's entries, and the element's bindings under
    `collections[member]` for a list or map of one. A view `for A, B`
    (34j) binds for each resource it lists. An entry several members
    carry binds at the first public one, so the external projection keeps
    the path, and the compiler warns about the others; the projection
    drops a binding whose path it removes. Nothing binds by name, so,
    unlike Smithy, a `lists` output whose items are a plain structure
    binds no collection, and a
    collection operation's elided properties bind as an instance
    operation's do.

    The targets of identifiers and properties are not visited, and there
    is one lowering: no documentation or infrastructure variant. A type
    that no operation reaches, only a property bound by no member, is
    named by `Resource::properties` but absent from `Service::shapes`, so
    every SDK and server, and the compatibility guard, sees the shape set
    it saw before resources were lowered. *Replaces:* Smithy's
    `lower_documentation` and `lower_infrastructure`, which visited those
    targets and so made the shape set depend on the output. *Why:* one IR
    serves the server, the SDKs, the reference and the provisioning
    generators, which therefore agree; the reference and the Explorer read
    the resource maps by id, and the CDK and Terraform bindings name
    member paths, whose targets are always reached. Release snapshots
    written since carry the resources; one frozen before has none and
    still reads, and the guard does not compare resources; `crucible
    freeze` can no longer restore the deleted file of such a release, since
    the snapshot it writes differs from the digest its lock records. Since a resource cannot be `internal`, neither may an
    identifier or property have an internal type: the compiler refuses it,
    so the external contract never names a type it leaves out. And since a
    resource cannot be `internal`, an internal operation in a lifecycle
    slot of a public resource fails external projection
    (`audience.resource_lifecycle`), as it did from Smithy; an internal
    custom action (`on`) is removed with its bindings.

## API versions across releases

Phase 5 of [api-versions-clients.md](../api-versions-clients.md): older
releases as projections of the current model (57a), one SDK line per
release, and a reference that spans releases. Clients sending their
release (5a) is built; these settle what blocked the rest. Line
references are to the code as of the design.

68. **A projection is checked with the guard's comparison, at error.**
    `diff(frozen r, projected r)`, both families at `error` whatever the
    manifest's `[compatibility]` says, with its `[[exceptions]]`, wherever
    the guard runs; plain `diff`, since the projection has put older names
    back. An error names the release and the migration that should have
    declared the break. Additions get no rule of their own: one
    `sdk_source` reports (`checker.rs:596-629`) was declared at its release
    by a step, which undo removes, or excepted, and undo keeps member
    order. Settings do not apply because an older line receives every
    change in a minor release, which the SDK release rule judges at error
    too; exceptions do, since frozen releases do not record the manifest's,
    and one stays while a release before its break is supported. Findings
    are deduplicated by rule and id. *Why:* one set of rules, and an undo
    gap cannot pass silently.

69. **A name an older release still uses cannot be reused.** An older name
    of a renamed member, a member name removed under an exception, a
    removed operation's name, a route a moved or removed operation had, and
    an id a restored definition reaches that is declared again as a
    structurally different shape are each an error at the new declaration,
    naming the release that uses the old one; every projection also passes
    `validate::service`. *Why:* an older request's name would mean two
    things, a projection would hold two operations with one id, and a
    re-added member shifts positional construction in the older line.

70. **What a step names on its older side must survive.** Every supported
    step is checked again against the two projections; removing a downgrade
    value, a downgrade error or a member it maps is an error at the step
    until the release is dropped. It is not restored: a restored value
    could be sent by older clients and read by no handler, and one
    conversion cannot invent the mapping. A step whose newer side was
    removed since under an exception is an error at the older name, naming
    that exception; excepting the older name too leaves the step inert,
    with a warning.

71. **A shape's identity is its id, in every release.** Steps resolve
    against projections: the new side in the newer projection, the old
    side in the frozen release. A shape renamed since a supported step
    named it is an error at the step, naming the missing id; a `rename` of
    a shape would be a later shortcut (54a). The generator's silent skips
    are diagnostics (built): the two an accepted model could reach are
    the compiler's, an operation dropped without `remove operation`
    (refused at the migration's `from` unless a `compatibility.operation`
    exception retires it, as decision 70 leaves an excepted step inert
    with a warning) and an enum value's rename (converted); the others
    can only be reached by an edited IR. Still to come:
    `crucible_lang::versions` collects every diagnostic with its span
    instead of stopping at the first. *Why:* the ids the server converts
    by must be the ones steps were checked against.

72. **Steps carry both sides, and undo composes them.** `Require` and
    `Hook::Require` gain their old side and older name, `AddEnumValue` its
    old side, `Move` its older route, `Reshape` its members;
    `Hook::Conversion` becomes a `Retype` by conversion; steps and
    migrations keep their `///`. `Service::versions` is never in a snapshot
    (`snapshot.rs:46`), so the format does not change, and a model inside an
    `ApiVersion` carries no `versions` of its own. Undo builds each
    element's older definition once, whole definitions first, then a
    member's definition, requiredness and name, regardless of declaration
    order, as the guard checks each step on its own. A `retype` restores
    the frozen member whole (target, constraints, timestamp format,
    default, nullability) at its current name and position. A restored
    definition's targets are the projection's where the id exists, else
    copied from the frozen release. *Why:* reverse declaration order breaks
    a `require` written before the `rename` it follows, and a retype's
    cover includes constraints and timestamps (`guard.rs:257-262`).

73. **Projection is an IR transform in crucible-ir, driven by
    crucible-lang.** Beside the audience projection (`audience.rs:17`), it
    recomputes reachable shapes, recursion groups and feature uses, and
    takes bindings, pagination, timestamp formats and examples from the
    frozen release for what it restores. An operation's authentication is
    its frozen release's when the release has it, the current model's when
    it was added since; permissions are the current model's, except that a
    restored operation keeps its release's requirements mapped to today's
    permissions. crucible-lang's `versions()` resolves the steps and runs
    the checks, for the build, the CLI and the editor alike. The audience
    projection projects `Service::versions` too. *Why:* a frozen release is
    never recompiled (52), so there is no lowering to finish; and each
    release's entry already authenticates with its own schemes
    (`authenticator_chain.rs:45-72`).

74. **Additions do not reach inside a reshape or a removal, or into a
    payload's message.** The older side of `reshape` and `remove
    operation` is the frozen definition; a member added since to that
    structure, or to a removed operation's own shapes, is not in releases
    before the step. Likewise, where a release sent a member of an
    operation's input or output (or an error) as the payload and its
    projection does again, a member added since that would travel in the
    body is not in that release: the message has no body to carry it. One
    bound to a label, query parameter or header still reaches it. The one
    exception to 51, stated in the reference (`history.missing`; the
    payload case is not dated there yet). *Why:* the hook converts the
    whole value, and the older definition has no place for what came later;
    a payload-bound message has no body, its older clients never sent the
    member, and the release's request must stay valid.

75. **The server generator serves projections, and their types grow.**
    Version modules, hook types and legacy handlers' types are projected.
    An operation added since that needs a hook direction a released
    migration lacked makes that migration's trait require the method, its
    documentation naming the operation; it is not hidden at older releases
    and gets no failing default. *Why:* hiding breaks 51, and a default that
    fails at run time is a silent skip.

76. **An SDK line is chosen with `model(..).api_version(..)`,
    `--api-version` or an SDK file's `api_version`, and records its release
    as `crucible.json`'s `serviceVersion`.** Rust and TypeScript write it in
    schema 2, the other languages in schema 1. The "`version` changed"
    signal never fires on a line; its releases are classified by the other
    signals, so an excepted break that reaches it is a major. A projection
    is the model whose `version` names one of its own `versions`; the Rust
    server generator refuses it. *Why:* every client already writes
    `crucible.json`.

77. **The reference dates every element from the release chain.**
    `history.added` is the release since which every release has the
    element, following renames (its latest addition, if it was dropped and
    added again), `null` when the oldest supported release has it;
    `history.changed` lists the releases whose steps name it;
    `history.missing` the releases that cannot see it because it sits
    inside a later reshape or removal. `crucible_ir::versions::history`
    derives them; `describe.rs` serializes. `@since` stays the author's
    fallback. *Why:* derived, it cannot drift; appearance and availability
    are different facts; `null` stays true when old releases are dropped.

78. **`describe.json` carries the release chain** in a `versions` block,
    always present (`null` without an `api_version`): the current release,
    the carrier, and per release its sunset, its migration's documentation
    and each step with what an older client sees instead, self-contained
    through `older.shapes`. Older definitions come from the frozen releases
    projected to the document's audience, and a step whose subjects are all
    internal is left out of an external document. It needs no projection
    and ships first. *Why:* one source for the reference, the upgrade
    guides and tools, and no leak of internal API.

79. **The current release is the primary reference; an older release's is
    a 5c output.** `generate docs --api-version` renders a projection, with
    current documentation except where undo restored a frozen definition,
    whose documentation is the release's (frozen releases keep `docs`;
    only `crucible-model.json` strips them, `snapshot.rs:68-155`). *Why:*
    new clients read the current release, and documentation fixes reach
    every line.

## Doc links

80. **`[Name]` doc links resolve once, to ids in a table beside the
    documentation, and a dangling one is an error (decision 41).** A doc
    link is a CommonMark reference link that its comment does not define,
    whose label is a capitalised declaration name with an optional
    `.member`: `[CreateApiKey]`, `[City.name]`, `[Color.RED]`, the collapsed
    `[Name][]` and the full `[text][Name]`. A `[Name]: url` definition in the
    same comment wins, as CommonMark says: that `[Name]` is an ordinary link.
    Code spans and blocks, inline links, images and other bracketed prose
    (`[1]`, `[optional]`, `[lower]`) are not doc links. `Name` resolves as a
    type name does, to the package's declarations and its imports'; a
    prelude type is accepted and has no target. `Name.member` is a
    structure's, union's or error's member, an enum's value, or an
    operation's input member, else its output member, since operation
    documentation names what a caller sends most. A link that names nothing
    is an error at its label, every one collected; Leyline and Pigeon had
    none. Lowering records `Service::doc_links`, each label to the id it
    names (an operation, shape, member, enum value, resource or the
    service; a metadata or rule name is accepted with no entry), and for an
    `[Operation.member]` label the operation in `doc_link_operations`, since
    operations can share the structure that holds the member; and it leaves
    `docs` as written. Both fields are `serde(default)` and omitted when
    empty, so a snapshot without links is unchanged and every older one
    reads; `crucible-model.json` drops them with the documentation. Links
    are checked in a package with no service, or several, as well. A
    migration's and its steps' documentation describes what changed, so a
    label there may also name what the release the migration comes from
    had (a removed operation, a member's older name): it is accepted with
    no entry and renders as its text, while one the current model has links
    as any other, and one in neither is an error. Only the compiled
    package's migrations are checked so: an imported package's are its own
    compile's, against its own releases. Only the
    broken-link callback makes a doc link, never a destination or definition
    the model wrote, so model text cannot pose as one.
    `crucible_ir::doc_links::links` is the one finder (pulldown-cmark's
    broken-link callback): lowering, every renderer and the language server
    use it, so they agree on what a link is, and `Service::doc_link` says
    what a label names in this service, or nothing. The audience
    projection drops the links whose target it removed, so in external
    output a link to internal API is its text and no id. Renderers:
    `describe.json`'s `html` makes a link `<a data-crucible-ref="<id>">`
    with no address, for its reader to map the id to its own page, `text`
    keeps the link's text without brackets, and `markdown` is what the
    author wrote; the static reference and the Explorer give the anchor
    its hash route, and the mdBook pages a relative link to the page. Native
    SDK documentation renders each link in its language's form to the
    generated symbol, around each dialect's existing escaping
    (`crucible_codegen_common::doc_links::render`); a target the language
    has no symbol for, and a full `[text][Name]`, is plain text. OpenAPI and
    JSON Schema descriptions and MCP tool descriptions, which have nothing to
    link to, keep the Markdown with each doc link as its text; the CDK,
    Terraform and the service CLI still carry the author's Markdown.
    *Replaces:* the warning of decision 41.
    *Why:* a table, not Markdown rewritten to `[Name](crucible:<id>)` on
    lowering: the author's Markdown stays what `describe.json` promises, a
    renderer that does not read the table prints the label as it did
    instead of a URL that leaks an id, and frozen releases' `docs` keep
    their meaning. One table per service, keyed by label, since a name
    resolves the same everywhere in a package.

## Sender-signed operations

81. **`@sender("Name", signature: member)` declares an operation a third
    party calls and authenticates with a signature it computes: a webhook
    receiver.** The operation is `public` (it holds no credential of this
    service), and `signature` names the input member, bound with
    `@header`, that carries the signature; the member may be optional, so
    a request without one is refused like a wrong one. Lowering records
    `Operation::sender` (`Sender { name, signature }`, `serde(default)`
    and omitted when absent, so every older snapshot reads and a model
    without it freezes unchanged), and `Operation::signature_header` reads
    the header from the member's binding. `crucible_ir::validate` refuses
    it on an operation that is not `public` (`sender.public`), a
    `signature` that names no `@header` input member (`sender.signature`)
    and a blank name (`sender.name`); lowering refuses any other argument
    form and any position but an operation. `describe.json` gives the
    operation `sender: {name, member, header}`, and the references say
    "Called by Stripe, not by clients of this API", where the signature
    travels, and send no `Authorization` or `Crucible-Version` header,
    which a third party does not send. Clients never call it, so
    `crucible_ir::client_view` drops it, with what only it reached and
    the doc links to it, for every client generator, the service CLI, the
    in-build Rust client and `sdks.json`, and client capability checks
    read that view; MCP leaves it out too, since an agent has no signature
    to send, and the in-service Explorer's try-it panel sends no bearer
    token for it. OpenAPI marks it `x-crucible-sender`. The compatibility
    guard compares client views: a released operation that becomes
    sender-signed, and every type only it reached, is a
    `compatibility.sdk` finding saying so, since every SDK drops them. A
    resource lifecycle role is refused (`sender.lifecycle`): clients and
    Terraform call those to manage the resource. The server implements it
    as any `public` operation: the handler verifies the signature, over the
    raw body when the input is a `@payload`. A third party names no
    release, so a request to it without `Crucible-Version` is served at
    the current release whatever `Unversioned` says for clients;
    `OperationInfo::sender` (generated `.with_sender("Stripe")`) says which
    operations, and the service settles at build whether it has any, so
    only then does an unversioned request route against the current
    table first. A release the request names is honoured. *Replaces:* inferring "called by
    another host" from an empty `auth` list, which also describes a
    public health check, and the prose every webhook repeated. *Why:*
    Leyline's two webhook receivers (`RecordStripeEvent`,
    `CreateGitHubSecretScanningReport`) are `public` operations whose
    documentation says who calls them and how they are signed, and their
    methods sat in every SDK beside the calls clients make. An
    annotation, not a keyword, because it carries values (decision 27a);
    the signature is a member rather than a header name, so the header is
    declared once and documented with the other request parameters. The
    verification scheme (HMAC, ECDSA, a key id) stays the handler's and
    the documentation's: senders differ, and no generator acts on it.

## Settled in the syntax pass

- Annotations versus keywords: decision 27a. Optionality is `name?:`.
- `include` may target any structure, including one used as a type
  (research: TypeSpec spread allows it with no reported friction).
- `//` is an ordinary comment, `///` is documentation, `////` and longer
  are ordinary comments, as in Rust. No block comments.
