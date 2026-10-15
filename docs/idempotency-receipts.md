# Idempotency record policies: Replay and Receipt

**Status: proposal. Nothing here is built.** It extends the server-side
`Idempotency` interceptor ([framework.md](framework.md#idempotency-tokens))
and the `.cru` language ([idl/reference.md](idl/reference.md)). The
test plan is §12, the rollout §13 and the open questions §15.

`Idempotency` today has one answer to a retry: it stores the first success's
output and replays it. This document adds a second, the answer AWS gives to a
retried create (EC2 `RunInstances`, IAM `CreateAccessKey`): remember only
*which resource* the token made, and answer a retry from that resource's
current state. The interceptor stays one interceptor. Each operation gets a
*record policy*, `Replay` or `Receipt`, derived from the model and overridable
in `.cru`.

Paths into the consumers are relative to their repositories: Leyline
(`encantado`, main `e8503660`) and Pigeon (main after #16).

## 1. The problem

A stored response is the wrong thing to keep for a create, for four reasons.

- **It stores secrets.** The record is the whole output as JSON, `@sensitive`
  members included, in plaintext for the retention (`idempotency/mod.rs`,
  last paragraph; framework.md "What is stored"). A secret shown once is then
  shown again to a retry, and sits in a table for 24 hours. Pigeon's
  `CreateWebhook` is `@token` plus a `sensitive string WebhookSecret` in its
  output (`model/api/operations/webhooks/CreateWebhook.cru:19,33,56`), and
  #16 puts it under `Idempotency` with no exception (only `CreateApiKey` is
  left out, because it has no token). As merged, the webhook secret appears to
  be stored in clear for a day. Nothing in the framework stops that.
- **It goes stale.** A retry five minutes later gets the resource as it was
  when created, not as it is. For a create whose resource changes (an import
  that starts, a graph that finishes provisioning) the retry lies about state.
  Leyline's standard says the opposite: "The same token and request return
  the resource as it is now" (`docs/architecture/api-design.md:271`, "A retry
  re-reads").
- **It breaks across a deploy.** A stored output is labelled with the API
  version it was written at and is refused as `Unreplayable` when the service's
  version has changed (`StoredResponse::read_output`, `stored_response.rs`).
  A retry that spans a deploy that bumps the version gets a 409 although the
  create succeeded.
- **It is large.** `max_stored_response` (256 KiB) exists because outputs are
  big, and an output over it becomes an `Unkept` note: the create happened,
  the retry is refused. A receipt is a few identifiers, so that failure mode
  disappears.

The AWS model avoids all four. The API standard Crucible follows asks for it
too: keep a token for "the lesser of 8 hours or the resource's life plus 1
hour" (`Replay` keeps 24 hours; §6.6).

Leyline already does this by hand, for every create, and must keep doing it:
its resource row records the caller's token inside the create's own
transaction, a token item holds only the resource id and a fingerprint, and a
retry re-reads (`service/src/api/shared/receipt/client_token.rs`). That
pattern belongs in Crucible if it is general. This document says how much of it
is, and what is not (§10).

## 2. Goals and non-goals

**Goals**

1. One `Idempotency` interceptor, two record policies per operation:
   `Replay` (as today) and `Receipt`.
2. `Receipt` stores the resource's identifiers, a fingerprint, the caller
   binding (in the key, as today) and an expiry. It never stores an output
   and so never a secret.
3. A retry is answered by a *generated* read: the resource lifecycle's `reads`
   operation, mapped into the create's output. A handler may override it.
4. The default comes from the model: a lifecycle `creates` operation with a
   token gets `Receipt`, every other tokened operation gets `Replay`. An
   operation says otherwise in `.cru`.
5. Leyline's shape (the create's transaction is the claim, so there is one
   write, not two) fits without a second code path in Crucible.
6. A secret cannot be stored by accident: the compiler says so.

**Non-goals**

- **No store-less "claim only" policy.** §10.
- **No change to `commit()`/`abandon()` semantics** for store-claimed
  operations.
- **No response storage in `Receipt`, ever**, not even as an option. A service
  that wants the first response replayed byte for byte chooses `Replay`.
- **No change to how the token is generated or sent.** SDKs fill `@token` as
  they do.
- **No in-flight queueing.** A bounded wait (§7.3) is not a queue.

## 3. What exists today

### 3.1 Crucible

`Idempotency` (`crates/crucible-server-runtime-rust/src/idempotency/`) is an
`Interceptor`:

- `before` validates the token, binds the caller, fingerprints the decoded
  input without the token (`Fingerprint::of`) and calls
  `IdempotencyStore::claim(key, fingerprint, attempt, now, expires)`.
  `Claim::Fresh` runs the operation. `Claim::Replay { fingerprint, response }`
  with an equal fingerprint reads the `StoredResponse`, asks `Permissions` to
  `reauthorize` the recorded handler checks, and ends the invocation with
  `DecodedJob::replay_output(output)`. `Claim::InFlight` is `Mismatch` or
  `InFlight` (409) at once.
- `after` stores `job.outcome()`'s output (`Outcome::Output(..).to_json()`) via
  `HeldClaim::finish` -> `complete`, or releases the claim on failure, or
  records a `Failed` note when a `commit()` stands (`attempt.rs`).
- `StoredResponse` is opaque to a store: a text. Its content is
  `{"apiVersion","output","authorization"?}` or `{"apiVersion","outcome":
  "failed"|"unkept"}`.
- `IdempotencyStore` has four calls (`claim`, `hold`, `complete`, `release`),
  each conditional on the attempt. `expires` doubles as the TTL.
- `prepare` refuses operations whose bindings predate replay
  (`replays_output`).

The model knows the token (`@token`, `OperationInfo::idempotency_token`) and the
resource lifecycle: roles `creates | puts | reads | updates | deletes | lists |
on`, "each lifecycle role is held once per resource", and `reads` requires every
identifier, inherited ones too (idl/reference.md, "Checks"; Leyline's
`GetGraph reads Graph` in `docs/idl/examples/leyline-graph/model/graph.cru`).
Nothing connects the two.

### 3.2 Leyline

Every create takes a token; "the service still refuses a request without one"
(`api-design.md` "Idempotency: client tokens"). Creates follow the AWS model:

- **One transaction.** The resource row records the principal-scoped token
  digest (`receipt::principal_token`), and a token item holds the resource id
  (and any key needed to read it back), a fingerprint "of the request as
  sent", the caller's scope and an expiry. No response, no secret
  (`client_token.rs` `ClientToken::put`).
- **Write first.** Nothing is read before the create. The token item's put
  is conditional (`attribute_not_exists(#partition) OR expiresAt <= :now`) and
  asks for the colliding item (`ReturnValuesOnConditionCheckFailure =
  ALL_OLD`), so the cancelled transaction says whether this is a retry
  (`ClientToken::classify` -> `Lost::Retry | Mismatch | Other`).
- **In flight does not exist**, except as a contended transaction: when the
  cancellation is `TransactionConflict`, `settle` looks again up to
  `CONTENDED_LOOKS = 5` times, 40 ms apart, for the item the concurrent attempt
  is committing (`client_token.rs:34-37`, `settle`). It waits; it does not 409.
- **A retry re-reads**: `CreateApiKey`'s retry reads the key back "without the
  secret" (`access/keys/creation.rs`, `Lost::Retry => created_key(..)`).
- **Retention**: `TOKEN_SECONDS = 8 h`, shortened by `for_resource_living` to
  "the lesser of eight hours and that life plus an hour".
- Its mismatch is a 400 `clientTokenMismatch`; a missing token a 400
  `clientTokenRequired`; the token is 8 to 128 safe characters
  (`TokenPolicy`), where Crucible's is 1 to 256 visible ASCII.
- Graph writes (`ExecuteQuery`, `RevertGraph`) are not covered: they key on the
  WAL write identity, which `RequireClientToken` scopes
  (`service/src/http/interceptors/client_token.rs`). §10.

### 3.3 Pigeon

#16 adopted `Idempotency` for `SendMessage` and four creates (`CreateInbox`,
`CreateDomain`, `CreateTemplate`, `CreateWebhook`), with a DynamoDB
`IdempotencyTable` (`service/src/data/idempotency.rs`) keyed by
`IdempotencyKey::digest()`. `SendMessage` splits `compose` from `send`, calls
`commit()` between them and `abandon()` only when SES proves nothing was sent.
`CreateApiKey` has no token. `CreateInbox`, `CreateDomain` and
`CreateTemplate` are lifecycle `creates` with a `reads` (`GetInbox`,
`GetDomain`, `GetTemplate`); `CreateWebhook` is `on Webhook collection` and the
resource has no `reads`; `SendMessage` is `on Message collection` (§9).

## 4. The model

### 4.1 The clause

An operation with a `@token` member may carry one `idempotency` clause:

```
idempotency = "idempotency" ( "replay"
                            | "receipt" [ "reads" ident ] [ "by" "handler" ] ) ;
```

```
/// Claim an address on a domain.
operation CreateInbox creates Inbox {
    http POST "/v1/workspaces/{workspaceId}/inboxes" -> 201
    requires manage on Inbox(*)

    input {
        $workspaceId
        @token clientToken?: ClientToken
        localPart: LocalPart
        $domainId
    }

    output { inbox: InboxData }
}                                    // no clause: creates + @token = receipt
```

```
operation CreateWebhook creates Webhook {
    idempotency receipt              // explicit here only for the example
    ...
    output {
        webhook: WebhookData
        /// Shown by this call alone. Absent from a retry, which reads the
        /// webhook as it is now.
        once secret?: WebhookSecret
    }
}
```

```
/// A message handed to the mailer: nothing to read back.
operation SendMessage on Inbox {
    idempotency replay               // also what it gets with no clause
    ...
}
```

```
operation CreateGraph creates Graph {
    idempotency receipt by handler   // Leyline: the create's own transaction records it
    ...
}
```

Rules of the clause:

- **`replay`** is today's behavior and needs nothing else.
- **`receipt`** answers a retry from the operation's *read*: the resource's
  `reads` operation, found through the lifecycle (§4.2). `reads Op` names a
  different read operation where the lifecycle has none (an operation
  `on Webhook collection` is not a lifecycle `creates`) or the author wants
  another. The named operation must be `readonly`, unpaginated, and every *required* input member of it must be supplied
  (§4.2).
- **`by handler`** says who writes the receipt: omitted, the interceptor
  claims in the store before the handler runs and completes after (two
  writes, as `Replay` does). `by handler` means the handler records the receipt
  inside the transaction that creates the resource, and the interceptor keeps
  no store at all for that operation (§6.4). The word `handler` is the one
  `requires p by handler` already uses for "the handler does this part".
- **`once`** is a new output-member modifier (beside `sensitive`, `internal`):
  the member is delivered by the first response only. The member must be
  optional; clients document it as "absent from a retry". A `receipt` operation
  checks that each of its output members is either present in the read's
  output with the same name and type, or `once` (§5).

### 4.2 Defaults

Resolved in lowering, so the IR always states the policy (and whether it was
written or defaulted):

| Operation | Default |
|---|---|
| has a `@token` and plays `creates` | `receipt`, reading the resource's `reads` operation |
| has a `@token`, any other role or none (`puts`, `updates`, `deletes`, `on`, collection) | `replay` |
| no `@token` | none; `idempotency` on it is an error |

If the resource has no `reads` operation, the default for `creates` is still
`receipt`, and the compiler reports `idempotency.receipt-no-reads` (an error)
naming the two ways out: declare a `reads`, or write `idempotency replay`.
The default is not silently weakened; the author decides. This is also the
secret rule (§5, check 6).

`updates` and `puts` can choose `receipt` (Leyline records a revision applied;
a retry reads the setting "as it is now"). They are not the default: an update
re-sent after a *different* update has moved the resource on should say what
Replay says, which is that the second caller's view is not the first's
output, and that is a decision, not a default.

**What the read needs and where it comes from.** The read's required input
members are bound by name: first from the create's *output* (the `id` the
service minted), else from its *input* (the parent identifiers the path
carries). For a resource lifecycle these are exactly the resource's
identifiers (`reads` "requires every identifier, inherited ones too"). For an
explicit `reads Op` outside a lifecycle, each required input of `Op` must be a
string or string enum (so it is storable) and be found this way. The receipt
records these as a named map, not a position: `{"workspaceId": ".."}`. A model
change that adds a required read input makes old receipts unreadable (§11).

### 4.3 In the IR

A new operation field, `serde(default)` per the IR's rules
(AGENTS.md, `crucible-ir`):

```rust
pub struct IdempotencyPolicy {
    pub record: Record,                 // Replay | Receipt
    pub claim: Claim,                   // Store | Handler (Receipt only)
    pub reads: Option<ShapeId>,         // the read operation, for Receipt
    pub explicit: bool,                 // written, not defaulted
}
```

and `once: bool` on an output member. A service that declares no clause gets
the default.

## 5. Compiler checks

Each has a diagnostic code, a fixture of its refusal and of its acceptance
(AGENTS.md: "every new construct ... a fixture case").

1. `idempotency.no-token`: a clause on an operation without `@token`.
2. `idempotency.receipt-no-reads`: `receipt` (written or defaulted) with no
   read operation: the resource has no `reads` and no `reads Op` is given.
3. `idempotency.read-shape`: the read operation is not `readonly`, is
   paginated, or has a required input member that the create's output
   and input do not supply by name and type. The message names the member.
4. `idempotency.receipt-shape`: an output member of a `receipt` operation that
   is neither in the read's output (same name and type) nor `once`. The
   fix is `once`, or a different read; a service-side `receipt_read` override
   (§6.3) does not lift the check (open question 5).
5. `idempotency.once-required`: a `once` member that is not optional. A client
   must be able to read a retry's response.
6. `idempotency.replay-sensitive`: a `replay` operation (defaulted or written)
   whose output has a `sensitive` member. Defaulted, it is an error: `Replay`
   stores the member in plaintext for the retention. Written (`idempotency
   replay`) it is accepted with a warning saying so, because the author has
   made the choice. This is the check Pigeon's `CreateWebhook` would have hit.
7. `idempotency.resource-unreachable`: a `receipt` whose read has a required
   input that neither the create's output nor its input supplies (the
   identifiers of the resource, or the explicit read's inputs, §4.2).

Warnings never withhold the IR (the same rule as permissions checks).

## 6. Runtime

### 6.1 The API

New items in `crucible_server_runtime_rust::idempotency` (behind the existing
`idempotency` feature), as a sketch:

```rust
/// How an operation's retries are answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RecordPolicy {
    /// Store the first success's output and replay it (today's behavior).
    Replay,
    /// Store which resource the token made; answer a retry from its current state.
    Receipt { claim: ReceiptClaim },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReceiptClaim {
    /// The interceptor claims in the store before the handler and completes after.
    Store,
    /// The handler records the receipt inside the transaction that creates the resource.
    Handler,
}

/// What a receipt says about a resource: the inputs of its read, by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceRef(BTreeMap<&'static str, String>);

/// A retry's handle on the resource its token made.
pub struct Receipt<O: ReceiptOperation> { /* resource, expiry, version */ }

impl<O: ReceiptOperation> Receipt<O> {
    pub fn resource(&self) -> &ResourceRef;

    /// The create's output for the resource as it is now: the read operation's
    /// result mapped into `O::Output`. The generated default, unless the
    /// service overrides it (§6.3).
    pub async fn read(&self, cx: &Context) -> Result<O::Output>;
}

/// Generated for each operation whose policy is Receipt.
pub trait ReceiptOperation: Operation {
    type Read: Operation;
    /// The read's input from a receipt (names -> typed members, parsed).
    fn read_input(resource: &ResourceRef) -> Result<<Self::Read as Operation>::Input>;
    /// The resource a success made: from the output and the input.
    fn resource_of(input: &Self::Input, output: &Self::Output) -> Result<ResourceRef>;
    /// Maps the read's output into the create's; `once` members are None.
    fn from_read(output: <Self::Read as Operation>::Output) -> Self::Output;
}
```

`OperationMetadata` (already `#[non_exhaustive]`) gains `idempotency_record:
RecordPolicy` next to `idempotency_token` and `replays_output`; `replays_output`
stays and means "a `Replay` operation can be replayed"; a `Receipt` operation
sets `receipts: bool` instead. Generated code states `Operation::RECORD`,
`Operation::RECEIPT` as `'static` data (AGENTS.md: thin generated code).

`Idempotency` gains:

```rust
impl Idempotency {
    /// How long a receipt answers its token: the lesser of this and the
    /// resource's life plus an hour. 8 hours by default.
    pub fn receipt_retention(self, retention: Duration) -> Self;

    /// How long a retry waits for a request still in flight before it is
    /// refused (§7.3). Zero by default (today's behavior).
    pub fn in_flight_wait(self, wait: Duration) -> Self;

    /// A service with no store: every covered operation is `Receipt` with
    /// `ReceiptClaim::Handler` (§6.4). `build` refuses any other policy.
    pub fn handler_claimed(
        caller: impl Fn(&Context) -> Option<String> + Send + Sync + 'static,
    ) -> Self;

    /// Replaces the generated read for one operation (§6.3).
    pub fn receipt_read<O: ReceiptOperation, F>(self, read: F) -> Self
    where
        F: for<'a> Fn(&'a Context, &'a Receipt<O>) -> BoxFuture<'a, Result<O::Output>>
            + Send + Sync + 'static;
}
```

and the handler-side functions, beside `commit` and `abandon`:

```rust
/// Shortens the receipt's retention to the resource's life plus an hour,
/// when that is under the retention. Never lengthens it. A no-op unclaimed.
pub fn resource_lives(life: Duration) -> Result<()>;
```

and for `ReceiptClaim::Handler` only, §6.4.

### 6.2 What `before` and `after` do

Store-claimed `Receipt`:

- `before`: as today up to the claim. On `Claim::Replay { fingerprint, response }`
  with an equal fingerprint, `response.read(version)` returns
  `Recorded::Receipt { resource, authorization }` (new variant). The
  interceptor then:
  1. **reauthorizes** as today (`permissions::reauthorize`, §7.4);
  2. runs the read (§6.3, §7.4) and maps it to `O::Output`;
  3. `job.replay_output(..)` with that output (typed output; no JSON round
     trip is required but `replay_output` takes JSON and `to_json` already
     exists; an `Output` taking variant can avoid the pass: cheap, noted);
  4. on the read's modelled not-found, `IdempotencyRefusal::Gone` (§7.1).
- `after`: `HeldClaim::finish(output, authorization)` is given
  `StoredResponse::receipt(version, O::resource_of(input, output)?,
  authorization)` instead of `wrap(version, output.to_json(), ..)`. It is
  always small, so `max_stored` does not apply. If `resource_of` fails (a
  generated-code bug: the output lacks a member the check required), the
  claim is recorded `Unkept`: the create happened, a retry is refused as
  `Unreplayable`, and the log says why. It is never a 500 for the first
  request.
- `commit()`/`abandon()`: unchanged (§7.5).

`StoredResponse` text for a receipt:

```json
{"apiVersion":"v3","receipt":{"resource":{"workspaceId":"w_1","inboxId":"i_9"},
 "shape":"vip.mailpigeon#Inbox"},"authorization":[...]}
```

A secret cannot be here because no output is. `shape` guards against a retry
that reads a record written for another resource shape (§11).

### 6.3 The generated read, and overriding it

The generated default calls the read operation's *handler* with the input
`read_input(&resource)` builds, in the retry's own `Context` (so its request
id, deadline and caller are the retry's), and maps the result. It is not
`Crucible::invoke`: that runs the whole pipeline (authentication, `Permissions`,
`Idempotency`, page tokens) as a fresh request and needs an `InvokeProtocol`
with no caller. The read is a nested call of the handler after exactly one
extra step: the read operation's declared checks (§7.4).

A service overrides it when the read operation's output does not map by name
(open question 5), when the resource lives somewhere the generated read does
not reach, or to answer more cheaply (Leyline reads the row the token item
already names; it has the `Item` in hand). `receipt_read::<CreateGraph, _>(..)`
replaces it. The override receives the same `Receipt<O>` and returns the
create's output type, so the compiler still checks the shape.

An override that cannot find the resource returns its modelled not-found error
(§7.1), and the same mapping applies.

### 6.4 `ReceiptClaim::Handler`: the create's transaction is the claim

Leyline's constraint is real: the receipt must commit atomically with the
resource, or a crash between two writes leaves a resource without a receipt
(the very window `Unkept` and `Failed` notes exist to shrink). So the
framework must be able to *not* claim, and let the handler write the receipt
in its own transaction.

For `by handler` operations the interceptor:

- validates the token, binds the caller, fingerprints the input and computes
  the expiry exactly as before (`well_formed`, `CallerBinding`,
  `Fingerprint::of`, `IdempotencyKey`), **and writes nothing**;
- leaves a `PendingReceipt` on the `Context` for the handler:

```rust
pub struct PendingReceipt<O: ReceiptOperation> { /* key, fingerprint, expires */ }

pub fn pending<O: ReceiptOperation>() -> Result<PendingReceipt<O>>;

impl<O: ReceiptOperation> PendingReceipt<O> {
    pub fn key(&self) -> &IdempotencyKey;          // .digest() is a fixed-length partition key
    pub fn fingerprint(&self) -> &Fingerprint;     // .to_hex() / from_hex()
    pub fn expires(&self) -> SystemTime;           // retention, shortened by resource_lives
    pub fn now(&self) -> SystemTime;               // the clock the claim used

    /// The handler checks this request was allowed (`permissions::stored_checks`),
    /// as JSON text; `None` outside `Permissions`. Call it after the handler's
    /// checks ran, and store it in the receipt item beside the resource.
    pub fn authorization(&self) -> Option<&str>;

    /// The handler's transaction wrote the receipt for `resource`: the
    /// obligation is met. Call it after the transaction committed.
    pub fn recorded(self, resource: ResourceRef) -> Result<()>;

    /// The handler's write found a receipt of this key already there (its put's
    /// condition failed and returned the old item). Answers as a retry
    /// would: a different fingerprint is `Mismatch`; the same one is
    /// authorized and read (§6.3). Return the result as the handler's own.
    pub async fn found(self, earlier: FoundReceipt) -> Result<O::Output>;
}

pub struct FoundReceipt {
    pub fingerprint: Fingerprint,
    pub resource: ResourceRef,
    /// What `PendingReceipt::authorization` returned when the receipt was
    /// written; `None` for a receipt written without it.
    pub authorization: Option<String>,
}
```

- an `after` hook checks that a *success* either called `recorded` or
  returned from `found`; otherwise the request fails as the service's internal
  error (a wiring bug: the create has no receipt), in every build, the same
  rule as `permissions`' unsatisfied obligations (`before_commit`; "a success
  with one fails the request in every build unless `assert_obligations(false)`
  opts out"). A failure releases nothing: there is nothing claimed.

Leyline's adapter:

```rust
let receipt = idempotency::pending::<CreateGraph>()?;
let token_put = ClientToken::from(&receipt).put(table, resource_ids)?; // its own item format
match tx.send().await {
    Ok(_)  => { receipt.recorded(graph_ref)?; Ok(output) }
    Err(e) => match classify(&e) {
        Lost::Retry(item)    => receipt.found(FoundReceipt { fingerprint: item.fingerprint(), resource: item.resource(), authorization: item.authorization() }).await,
        Lost::Mismatch(_)    => receipt.found(FoundReceipt { fingerprint: other, .. }).await,  // Mismatch
        Lost::Other          => Err(existing_meaning(e)),
    }
}
```

The service writes whatever item shape and keys it already has. Crucible
needs no knowledge of DynamoDB; it needs the key digest, the fingerprint, the
expiry and a typed way to turn a found receipt into an answer. This is why
the same interceptor serves Leyline: *claim* is what the store does in
`Store` mode and what the handler's transaction does in `Handler` mode, and
everything either side of it (token rules, binding, fingerprint, retention,
authorization, the read, the 409s) is shared.

**No store.** A service whose every covered operation is `by handler` has no
`IdempotencyStore`. `Idempotency::handler_claimed(caller)` is the constructor;
`build` refuses an operation whose policy needs a store. A service with both
passes a store to `Idempotency::new` and uses `by handler` where it wants.
`IdempotencyStore` itself does not change (§6.5).

**In `Handler` mode `commit()` and `abandon()` do nothing** (there is no claim
to hold or release) but `commit()` still refuses an open permission obligation
(`before_commit`), as it does for an unclaimed request today. The create's
transaction is the irreversible step; an external effect after it (Leyline's
Athena query for `StartAuditSearch`) is the service's own "work outside the
database has one owner" rule, and Crucible does not try to subsume it.

**The "refused before it could be attempted" case** (Leyline reads the token
item once when a state its own earlier attempt caused refuses the create:
api-design.md, paragraph after "Kept for the lesser of"): the handler reads its
item and calls `found` with it. Same entry point.

### 6.5 The `IdempotencyStore` delta

None to the four methods. The reasons, so the review can check them:

- `StoredResponse` is opaque text; a receipt is a new content shape of it
  (`StoredResponse::receipt`, `Recorded::Receipt`). A store that already
  passes the text through is unchanged. Pigeon's table, which stores the text
  as an attribute, needs no migration.
- The conditional writes, the attempt, `Ownership` and the TTL are what a
  store-claimed receipt needs, because it *is* a claim followed by a
  completion.
- `Handler` mode uses no store.

Contract additions (documentation, not signatures):

- `complete`'s `response` may be small and its `expires` may be earlier than
  the claim's (shortened by `resource_lives`). A store must honor both.
- A store may see `complete` called once per attempt with either kind of
  text, and must not parse it.

One method was considered and rejected: a `rewrite(key, response)` to overwrite
an old `Replay` record with a receipt when answering it (§11), so that its
plaintext secret is scrubbed early. It needs a new conditional write on a
*completed* record, which no store has today and which would reopen the
"never change a completed record" invariant. The plaintext lives out its
original TTL instead; §11 says what a service can do.

### 6.6 Retention

`Replay`: unchanged, 24 hours (`DEFAULT_RETENTION`; the doc comment there
already notes the standard's lesser-of-8-hours floor). `Receipt`: 8 hours by
default, and "the lesser of 8 hours or the resource's life plus one hour" when
the handler calls `resource_lives(life)` (an API key with an expiry, a job with
a TTL). A shorter window is safe for a receipt because it names a resource that
a retry can still read, and it is what lets a token make a new resource once
the old one is long gone, which Leyline's `for_resource_living` relies on.

## 7. Behavior at the edges

### 7.1 Retry after the resource was deleted

A `Receipt` retry reads the resource; the resource is gone. The read
operation's own modelled not-found (a client error with status 404) comes out
of the generated or overridden read.

Options:

- **A. Pass the read's 404 through.** Truthful, simple, no new refusal. But a
  POST to a collection answering 404 reads as "the collection does not exist"
  and, with `Permissions`, a 404 means *Hidden*; a client cannot tell "you may
  not see it" from "you made it and it was deleted".
- **B. A new `IdempotencyRefusal::Gone`, a 409.** Says what happened: the
  token made a resource that no longer exists, so this retry is neither
  repeated nor served; send a new token for a new resource. 409 is already the
  family of `Mismatch`, `Failed` and `Unreplayable`, so clients and
  `refusal_factory` handle one class.
- **C. Run the create again** (treat as expired). Violates the point of the
  token: the first request did happen.

**Recommendation: B**, mapping only the *handler-origin* modelled 404 (the
read ran and found nothing) to `Gone`. A refusal from the read's *declared
checks* (§7.4: Denied 403, Hidden 404) is passed through unchanged, so
`Permissions` keeps answering a caller who may no longer see it exactly as it
answers the read. AWS's practice is to keep a deleted resource readable in a
terminal state for about an hour, which is what "life plus 1 hour" is for: the
service may implement a soft state and the retry then reads it normally. Where
it does not, `Gone` is the honest answer inside the window.

The `Gone` message: "a request with this idempotency token created a resource
that no longer exists; send a new token to create another". Added to the
`#[non_exhaustive]` `IdempotencyRefusal`, so a factory's `_` arm covers it, and
documented in the `Migration` entry (§12).

### 7.2 The resource changed, or the read fails

The retry gets the resource as it is now, in the create's output shape and
the create's status (201): the status is the *operation's* (`http POST ... ->
201`), the output the create's. `once` members are absent. Any other error from
the read (a 5xx, a timeout) is the retry's failure and the claim is untouched:
a read changes nothing, so the client may retry again. It is not `Unreplayable`.

### 7.3 A retry while the first is in flight

Today (`Claim::InFlight`) a retry is a 409 `InFlight` at once. Leyline's
behavior is to wait: `settle` looks again 5 times at 40 ms for the concurrent
attempt's token item. They answer different situations, and both are right for
theirs:

- In `Handler` mode there is no in-flight state to observe. A concurrent
  attempt is a contended *transaction*; the handler's bounded look (Leyline's
  `settle`, kept in the handler) is what waits, and `found` is its result.
  Crucible does nothing here.
- In `Store` mode there is: from `claim` until `complete`, as long as the
  create runs (tens of milliseconds to a Lambda's whole timeout, bounded by the
  lease). A 409 for something the client's own SDK retried after a timeout
  defeats the purpose of the token: the first succeeded, the SDK reports an
  error. Weighing:
  - *Wait*: the retry succeeds in the common case (a create that takes a
    second). Cost: a Lambda billed while it polls, one consistent read per
    poll, and a thread held. Bounded by `in_flight_wait` and the request's
    deadline, polling the claim with a capped backoff (50 ms doubling to
    400 ms). The poll is `claim` again, which changes nothing on `InFlight`.
  - *409 now*: no cost, exact, honest; the client backs off itself if its SDK
    treats the 409 as retryable (to be confirmed for the generated
    clients: a plain `ConflictException` is not marked `retryable`).
  - *409 with `retryable`*: the model marks the in-flight conflict retryable,
    so SDKs back off and retry. It needs a distinct error from the other
    conflicts (a `reason`), which `refusal_factory` can build but the default
    `FrameworkError::Conflict` cannot.

**Recommendation:** ship `in_flight_wait(Duration)`, **default zero** (no
behavior change), and say in the docs that a create-heavy service should set
it to about two seconds (Leyline's contended wait is under a quarter of a
second, a store-claimed create is a database write plus the handler). If the
wait expires, `InFlight` as today. Whether `InFlight` should be a retryable
error by default is open question 3.

### 7.4 Authorization on a retry

**Handler mode.** The framework cannot see the handler's receipt item, so the
handler persists the recorded handler checks itself: it writes
`PendingReceipt::authorization()` into the item and hands it back as
`FoundReceipt::authorization`. `found` then runs `reauthorize` on it exactly as
store-claimed `Receipt` does, before the read. A receipt without it (`None`)
is authorized by the declared checks and the read's checks alone, as an old
`Replay` record is today. Whether to *require* it for operations whose handler
makes checks is open question 11.

`Permissions` runs first (registered before `Idempotency`; building the other
order fails), so the *create's own* declared checks (`requires manage on
Inbox(*)`) run on every retry before any claim or read: a caller who lost them
is refused as the request itself, with the token unspent.

`Replay` then calls `reauthorize` for the handler checks the first success was
allowed (`stored_checks`), kept in the record as `authorization`. `Receipt`
keeps that field unchanged: a few checks and ids, never a grant or a secret.

`Receipt` adds one decision, the **read operation's declared checks on the
specific resource**. A retry is a *read of current state* by a caller; it
should be refused if that caller may not read the resource now, as a
`GetInbox` would refuse them. Mechanically: before the read handler runs,
`Permissions` evaluates the read operation's `requires` (or `filters`) against
the `ResourceRef` (the read's bindings are by name, the same names), and an
Allowed result proceeds; Denied/Hidden are the read's refusals unchanged (§7.1).
The create's own checks are not repeated (they already ran).

Why not skip it? Without it, a caller with only `manage` on `Inbox(*)` who
created an inbox could retry and read its details without `read` on it. The
first response did show those details to them (the output of the create), so
this is not a leak of anything the create did not already return; the
argument for skipping is "a retry is as good as the first" and the argument
for the check is "a read is a read". Pigeon's table says `inbox:manage` does
not imply `inbox:read` (permissions.md §3.1), so the difference is real: with
the check, a manage-only creator's retry is a 403 where its first call
succeeded. **Recommendation:** run the check by default (the read is the
operation, so its authorization applies); open
question 1 asks whether an escape from it should exist at all.

`Replay` is unchanged here.

### 7.5 `commit()` and `abandon()`

For `Store`-claimed `Receipt`, both are as today and mean the same thing:

- Before `commit`, a failure releases the claim: a retry runs the create
  again, correctly, since it created nothing.
- After `commit`, a failure is recorded `Failed` for the retention
  (`IdempotencyRefusal::Failed`), and `abandon()` takes the latest commit back.
- A success is recorded as a *receipt*. If the success then cannot be recorded
  (store failure on `complete`), the `Unkept` note and the background retries
  apply as today: the resource exists and the retry is refused rather than run
  again. A receipt is more recoverable than a response: the resource's id is
  in hand, so an `Unkept` could be upgraded to a receipt by the background
  retry (`Retry::Unkept` writes the receipt instead of the note when it holds
  one). That is a small addition to `attempt.rs`, listed in the plan (§12,
  phase 1).

For `Handler` mode both are no-ops except for the obligation check (§6.4).

An operation that creates, then does something not repeatable, and is
`Store`-claimed commits before that step exactly as `SendMessage` does. If the
step fails after the resource was created, the first request fails (the client
sees an error), the record is `Failed`, and a retry is refused: the resource
exists and the client checks. A `Receipt` could do better (answer the retry
with the resource and its failed step) but the output shape for a failed step
is the operation's business; not proposed.

### 7.6 PageTokens

No interaction. `Receipt`'s read must be unpaginated (check 3), and a create's
output carries no page token. `PageTokens` and `Idempotency` stay independent
interceptors that share the caller binding; a `Receipt` read never runs the
`PageTokens` pipeline because it is a nested handler call (§6.3), not a request.

### 7.7 API versions

`Replay` stores the version the output was written at and refuses a different
one (`Recorded::Unreadable`). `Receipt` does not: its content is identifiers,
and the retry builds the *current* version's create output from the current
read, which the version conversion pipeline then converts for the retry's own
version, the same way it converts any output. So a receipt written at v3 answers
a retry at v1 and across a deploy that bumps the version.

What can still make a receipt unreadable is a change to the receipt's own
contract: the `shape` of the resource (a different resource or a rename of its
identifiers) or a read operation that now requires an input the record lacks.
Those are model changes that `crucible check`'s compatibility guard can flag
(a migration `rename` of an identifier is the author saying so). An unreadable
receipt is `Unreplayable`, as today's stale responses are.

An older version whose model lacks the token still "runs unclaimed", as today.

## 8. How Leyline's pattern fits

Mapping Leyline's pieces to this design (nothing here changes Leyline's
storage; the adoption plan is a separate lane):

| Leyline today | In this design |
|---|---|
| `TokenPolicy` (8-128 safe chars), `RequireClientToken` | `Idempotency` validates the token; the length rule needs `Idempotency::token_length(8..=128)` (open question 6) |
| `principal_token(caller, token)` digest | `IdempotencyKey::digest()` of (service, operation, caller binding, token); keys change at adoption, receipts in the 8-hour window do not carry over |
| token item: resource id, fingerprint, scope, expiry | `PendingReceipt::{key, fingerprint, expires}` the handler writes in its item; `ResourceRef` for the resource |
| create attempted first, conditional put with `ALL_OLD` | unchanged; `ReceiptClaim::Handler` |
| `Lost::Retry / Mismatch / Other` | `PendingReceipt::found` for the first two; `Other` is the handler's own error |
| `CONTENDED_LOOKS`/`settle` | stays in the handler (§7.3) |
| `for_resource_living` | `resource_lives` (shortens `PendingReceipt::expires`) |
| retry re-reads (`created_key`, `GetGraph`-like) | the generated read, or a `receipt_read` override that uses the `Item` already in hand |
| mismatch 400 `clientTokenMismatch`, missing 400 `clientTokenRequired` | `refusal_factory` |
| the resource row records the token digest | unchanged; Crucible never sees it |

What Crucible does *not* absorb from Leyline: the item formats, the table
layout, the contended-transaction wait, the "refused before attempted" look.
They are DynamoDB; the seam is `PendingReceipt` and `FoundReceipt`.

`ExecuteQuery`/`RevertGraph`: §10.

## 9. What Pigeon would choose

| Operation | Policy | Why |
|---|---|---|
| `SendMessage` | `replay` (`on Inbox`, not a create) | An accepted send has nothing to read back; the first response is the answer. Keeps `commit()` before `send` and `abandon()` on a proven refusal. |
| `CreateInbox` | `receipt` (default) | `creates Inbox`; `GetInbox reads Inbox` exists. Retry shows the inbox as it is now. |
| `CreateDomain` | `receipt` (default) | Same; the retry shows the current verification state, which is what the caller wants. |
| `CreateTemplate` | `receipt` (default) | Same. |
| `CreateWebhook` | `receipt` with `once secret` | Needs a read: no `GetWebhook` exists today (only the list and update/delete), so Pigeon adds `reads Webhook` first or writes `idempotency receipt` with a `receipt_read` override. Until then the check in §5 (6) would flag the merged state: `Replay` of a `sensitive` secret. |
| `CreateApiKey` | no `@token` (unchanged) | The secret and the key are one response; a retry reading the key back "without the secret" is the AWS `CreateAccessKey` behavior and is available the moment Pigeon adds a `@token` and `reads ApiKey`, which it has not done. Not a Crucible question. |

Two Pigeon facts the survey turned up: `CreateWebhook` is `on Webhook
collection`, not `creates Webhook`, so the *default* would be `replay`, and
check 6 makes the compiler say what to do. And its table already stores
`StoredResponse` text opaquely (`data/idempotency.rs`), so the new content
shape needs no schema change.

## 10. Why a store-less "claim only" policy does not belong in Crucible

Leyline's graph writes (`ExecuteQuery` of a mutating statement,
`RevertGraph`) have a token, require it, and do *not* want a claim, a record or
a replay. They scope the token into a write identity (`WriteIdentity`) that the
WAL keys on, so the WAL dedupes. `RequireClientToken` is that: validate the
token, put a value on the context. One could name that a third policy
(`Require`: validate the token, store nothing) and have Crucible own it. It
should not, for four reasons.

1. **The interceptor's promise would be false.** `Idempotency` means: "a retry
   does not run the operation twice." `Require` would not keep it. The handler
   runs on every retry; the guarantee is whatever the downstream does with a
   value Crucible does not understand. A policy under that name that does not
   deduplicate invites a reader to believe an operation is protected when it is
   not. The default derivation ("`creates` -> Receipt, otherwise Replay") could
   never produce it, and `.cru` authors who write it are asserting something
   only the downstream can honor.
2. **What is general is already there.** The only shared work is "the token is
   present and well formed" and "give me the caller-bound token". The first
   is the validation the interceptor already does before the policy matters; the
   second is `CallerBinding` plus `OperationInfo::idempotency_token`, both
   public. Leyline's interceptor is about 50 lines over those. The part that is
   not a line of code is Leyline's: `ExecuteQuery` demands a token only if the
   statement parses and writes (`needs_token`), a rule of the query language,
   and the WAL write identity (`audit::write_identity`), which has no meaning
   elsewhere.
3. **Two sources of truth.** If the policy lived in the model, the model would
   claim the operation is idempotent while the guarantee lives in a WAL the
   model does not mention. Leyline's own architecture doc says the opposite
   ("Graph writes ... key on the WAL write identity instead and are not covered
   here"). Keeping it out of the clause keeps the clause honest: every value
   of `idempotency` is a mechanism Crucible runs and tests.
4. **The `.cru` clause is the wrong shape.** A graph write is not a claim, a
   response or a resource; there is no lifecycle read, no fingerprint worth
   storing. The policy would carry no parameters Crucible can use.

What *is* worth giving Leyline, and is proposed as a small separate item (open
question 6): a public helper that returns the validated token and the
caller-binding digest for a job (`idempotency::token(job) -> Result<ValidToken>`),
sharing `well_formed` and a configurable length rule, so Leyline's
`RequireClientToken` does not re-implement them and cannot drift from the
framework's rules. Leyline keeps `RequireClientToken` for graph writes either
way.

## 11. Store schema and migration

**Records.** Nothing in the table changes: a record is key, fingerprint,
attempt, expiry and an opaque text. Three content shapes now exist:
`{apiVersion, output, authorization?}` (a replay), `{apiVersion, outcome}` (a
note), and `{apiVersion, receipt, authorization?}` (new).

**An operation moving from `Replay` to `Receipt`** (the default flip in phase 4,
or an author writing `receipt`):

- A retry that finds an old `Replay` record (`Recorded::Output`) is answered by
  extracting the resource from the stored output with the same generated
  `resource_of`, then reading it (§6.2). The old output carries the
  identifiers because the check (§5) required it. This needs no data
  migration.
- The old record's plaintext (secrets included) stays in the table until its
  own TTL, at most the 24 hours of the old retention; a service for which
  that matters shortens the table's TTL with the deploy or deletes by key
  prefix.
- If the old output has no extractable identifiers (the output changed), the
  retry is `Unreplayable`, as a stale response is today.

**An operation moving from `Receipt` to `Replay`:** a receipt record under a
`Replay` operation is `Unreplayable` (a `Replay` has no hook to read it), for
at most 8 hours.

**A stale shape:** a receipt whose `shape` or read inputs no longer match the
generated code is `Unreplayable` (§7.7).

**The `Idempotency::new` signature and the four store methods** are unchanged;
no `Migration` entry is needed for stores. The default flip is a behavior
change (a `creates` with a token answers retries differently, and a delete
now yields `Gone`), so it gets a CHANGELOG `Migration` entry, and a `crucible.toml`
switch for the staging period:

```toml
[idempotency]
default = "replay"   # until the service is ready; "receipt" is the new default
```

(open question 7 asks whether a switch is worth it while the only consumers
are Leyline and Pigeon, AGENTS.md: "Breaking changes are fine".)

## 12. Test plan

Unit (runtime):

- `StoredResponse::receipt` round trip; a receipt never contains an output
  (assert no `output` key); unknown `shape` and missing named inputs read as
  `Unreadable`; an old `output` record under a `Receipt` operation yields the
  resource; a receipt under a `Replay` operation is `Unreplayable`.
- `resource_lives` caps `expires` at life plus an hour, never lengthens it,
  with the injected clock (`Idempotency::clock`).
- `Gone` mapping: a handler-origin 404 maps; a read-check Denied/Hidden does not;
  a 5xx from the read is the retry's failure and leaves the record.
- `in_flight_wait`: a retry that arrives mid-create is served once the first
  completes; a retry that times out is `InFlight`; the wait stops at the
  request's deadline.

Interceptor (on the real builder with `MemoryIdempotencyStore`, as Pigeon's
tests do):

- identical retry runs the create once and returns the resource as updated
  in between (the "re-read" proof), with the create's status and no `once`
  member; different caller or workspace runs its own; changed input is
  `Mismatch`; concurrent retries run one.
- failure before `commit` releases; after, `Failed`; `abandon` releases;
  success then a failing `complete` records `Unkept`, and the background retry
  writes the receipt (new).
- retry after delete is `Gone`; after the resource's soft-deleted state it
  reads normally.
- permissions: the create's check fails -> token unspent; the read's check
  Denied -> 403 and the retry succeeds later once granted; handler checks
  recorded in `authorization` are asked again.
- API versions: write at one version, retry at an older version and across a
  version bump: answered, not `Unreplayable`.
- `Handler` mode: `recorded` meets the obligation; a success that never calls
  it fails the request; `found` with an equal fingerprint reads, with another
  is `Mismatch`; `handler_claimed` constructor refuses a store-needing
  operation at `build`; `commit()` is a no-op but still refuses an open
  permission obligation.
- `prepare` refusals: two interceptors on one operation (as today).

Language (`crucible-lang`, `crucible-ir`): a fixture pair for each check
in §5 (refusal and acceptance), the default derivation table of §4.2, and
`once` parsing; the IR snapshot with the new field defaulting
(`serde(default)`), so committed releases still load.

Codegen: goldens for `ReceiptOperation`, `RECORD`/`RECEIPT` statics and the
`once` member (optional in every client; the doc says "absent from a retry").
`tools/test-consumers.py` stays green.

End to end (`e2e/weather`): create a forecast station with `receipt`, retry
after a change, retry after delete, with HTTP parity for status and headers
(the "what a replay is not" caveats: response headers are the retry's own).

Live (Leyline, later): the integ suite proves a retry of `CreateApiKey` has no
secret and a retry after `DeleteGraph` is `Gone` (or whatever Leyline's
`refusal_factory` maps it to) before any release (Leyline's standing
rule: prove fixes locally before pushing).

## 13. Phased rollout

Each phase is a pull request that changes no default until phase 4.

1. **Runtime, store-claimed `Receipt`.** `RecordPolicy`, `Recorded::Receipt`,
   `StoredResponse::receipt`, `Receipt<O>`, `ReceiptOperation` (hand-written for
   the weather e2e), `Gone`, `resource_lives`, `receipt_retention`,
   `in_flight_wait`, the `Unkept` upgrade. Opt-in per operation by the builder
   (`Idempotency::receipt::<O>()`). CHANGELOG entry; framework.md section.
2. **Language, IR, checks and codegen.** The clause, `once`, defaults
   *computed* but with `[idempotency] default = "replay"` in effect, the §5
   diagnostics (check 6 included, as an error only on a defaulted `replay`),
   generated `ReceiptOperation`/statics, clients' `once` docs, OpenAPI.
3. **`Handler` mode.** `PendingReceipt`, `found`, `recorded`, `handler_claimed`,
   obligation check. Behind the same feature.
4. **Flip the default** (`creates` + token -> `receipt`), with the CHANGELOG
   `Migration` entry; Pigeon adopts (`CreateWebhook` first, since it has the
   secret).
5. **Leyline** (after its `.cru` migration): `by handler` on its creates, the
   token helper (open question 6), and `RequireClientToken` shrunk to the graph
   writes.

## 14. Alternatives considered

- **Two interceptors**, `Idempotency` and `Receipts`. Rejected: one request
  has one idempotency state (`Attempt` on the context; the interceptor refuses
  a second), and they would need the same token rules, binding, fingerprint,
  refusals and `commit()`. Two would drift.
- **Receipt as `Replay` plus a "scrub sensitive members" filter.** Rejected:
  it still stores the rest of the output, still goes stale and still breaks
  across versions; and "sensitive" does not mean "once".
- **Make `Replay` re-read everywhere** (no policies). Rejected: `SendMessage`
  and `puts` have nothing to read; a delete's retry has nothing to read.
- **Always claim in the store; let Leyline pay two writes.** Rejected: it
  opens the window the AWS model closes (resource without receipt), and
  Leyline would give up a property it keeps on purpose.
- **A `Handler` mode with `IdempotencyStore` methods that the handler
  calls** (`claim_in(transaction)`). Rejected: the trait would grow a
  transaction type every store defines differently. Plain data
  (`PendingReceipt`) keeps Crucible out of DynamoDB.
- **Receipt keyed positionally by the resource's identifier chain.** Rejected
  for a named map: a reorder or an added optional read input must not
  invalidate receipts silently.

## 15. Open questions

1. **Should a retry run the read operation's checks (§7.4)?** Recommended yes.
   The cost is a manage-only creator's retry being a 403 where the first
   call succeeded (Pigeon's `manage` does not imply `read`). Is there an escape
   (`idempotency receipt unchecked`)? Or is a create whose caller cannot read
   the result a modelling smell the compiler should flag instead?
2. **`Gone` as a 409 (§7.1)**, not a pass-through 404. Does Leyline want a
   modelled error with a `reason` (via `refusal_factory`), and should the
   framework offer the read's 404 as an alternative setting?
3. **Should `InFlight` be retryable by default (§7.3)** (a `retryable` conflict
   with a `reason`), so SDKs back off without server-side waiting? That needs
   the framework's default `FrameworkError::Conflict` to grow a retryable
   variant; whether generated clients retry a 409 is to be confirmed.
4. **`in_flight_wait` default.** Zero preserves behavior. Is two seconds for
   `Receipt` operations right, and is a per-operation setting worth having
   over a service-wide one?
5. **The read-shape rule (§5, check 4).** Name-and-type equality of the
   create's output with the read's is strict. Leyline's `CreateGraph` returns
   `graph: GraphData`, as `GetGraph` does, so it passes; an operation whose
   read returns a differently named member needs an override. Is a
   `receipt_read` override enough, or should the clause map members
   (`receipt reads GetGraph { graph <- item }`)?
6. **The token helper and length rule.** Leyline's token is 8 to 128 safe
   characters, Crucible's 1 to 256 visible ASCII. Do we add
   `Idempotency::token_length(min..=max)` and `idempotency::token(job)`
   (§10), or does Leyline keep its own rule and accept the drift?
7. **The default-flip switch (§11).** A `crucible.toml` `[idempotency] default`
   staging switch costs a configuration surface for two consumers. Skip it and
   flip in one release?
8. **How the generated read calls a handler (§6.3).** The proposal is a nested
   call of the read handler in the retry's context after the read's checks.
   `Crucible::invoke` is not it (it runs the whole pipeline). The exact
   mechanism, and whether the read should be allowed to see the retry's request
   headers, needs a prototype in phase 1.
9. **`updates`/`puts` receipts.** Leyline records a revision applied and
   re-reads "as it is now" for settings and policies. They are an explicit
   `idempotency receipt`, never a default (§4.2). Is a revision (not just a
   resource ref) part of the receipt, and so part of `ResourceRef`?
10. **Retention of `Replay`.** Leave at 24 hours, or bring it to the standard
    (8 hours) with a deprecation, since `Replay` is now the rarer policy?
11. **Handler-mode authorization.** Persisting the handler checks is the
    handler's job (§7.4). Should `recorded` fail when `Permissions` recorded
    checks and the handler passed none along, so a forgotten write is loud?
