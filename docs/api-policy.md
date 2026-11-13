# API policy and compatibility

Crucible checks project API conventions during Cargo builds and compares
contracts with `crucible diff`. Both use `crucible-policy` over the lowered IR.
Model validity and supported constructs remain `crucible-ir`'s
responsibility; policy cannot disable those diagnostics.

## Enabling build checks

Create `crucible.toml` beside the service's `Cargo.toml`:

```toml
schema_version = 1

[rules.naming]
severity = "error"
operations = "PascalCase"
shapes = "PascalCase"
members = "camelCase"
enum_values = "SCREAMING_SNAKE_CASE"
enum_wire_values = "camelCase"
operations_pattern = "(Get|List|Create|Update|Delete)[A-Z].*"

[rules.paths]
severity = "error"
prefix = "/v1"
segment_case = "kebab-case"
parameter_case = "camelCase"

[rules.documentation]
severity = "warning"
targets = ["operations", "errors"]

[compatibility]
severity = "error"
wire = true
sdk_source = true
```

Select the file in `build.rs`:

```rust,ignore
fn main() {
    crucible_build::model("model").server()
        .policy("crucible.toml")
        .generate();
}
```

The same `.policy(...)` method works with `model(...).server()`, and `model(...).client()`. Paths are relative to
the package's `Cargo.toml`; a workspace can select `../../crucible.toml`.
The selected config is required. Invalid config, missing config, or error
findings fail the build. Cargo tracks config edits and removal. Warnings and
excepted findings are printed without blocking generation.

New `crucible new` starters include the config and build hook. Existing
packages opt in with `.policy(...)`. Explicit selection avoids watching a
missing optional config, which would force Cargo to rerun the build script
on every build. Normal builds require no baseline, Git history, or CLI install.

### In a `.cru` package's manifest

A Crucible language package needs no second file: its own `crucible.toml`
holds the policy beside `[model]`, and every build of the package applies
it, as does `crucible check <package>`. `.policy(..)` and `--config` still
select another file.

```toml
[model]
namespace = "example.service"
imports = ["crucible.errors"]

[rules.naming]
operations = "PascalCase"
members = { case = "camelCase", severity = "warning" }
```

Every build that compiles the package applies that policy, so a workspace
with two such builds prints each warning twice: a model crate whose
`publish()` applies the policy, and a service that generates from the
published model. The second build can leave the report to the first:

```rust,ignore
// The model crate's build.rs, `crucible_build::model("api").publish()`,
// reports the model's warnings; this one only enforces them.
fn main() {
    crucible_build::model("example_model")
        .report_warnings(false)
        .server()
        .generate();
}
```

`report_warnings(false)` silences what the model build prints as
`cargo::warning`: compiler warnings and advice, compatibility findings the
policy makes warnings, and policy warnings and excepted findings. Errors
still fail the build. Use it only where another build of the same Cargo
invocation reports the model: Cargo shows a build script's warnings only
for a local package (a workspace member or a `path` dependency), so a model
crate fetched from a registry or Git reports nothing, and a model crate's
`publish()` compiles the model only when it has a policy to apply. Within
one build each warning prints once, however many outputs lower the model.

The language server applies the same policy as you type: a finding shows at
the name it concerns, as an error or a warning, and a naming finding offers
a rename to the configured case. It looks for the policy in the package's
manifest, then in the nearest `crucible.toml` above it that has one, which
covers a workspace that selects a shared file with `.policy(..)`. The
manifest itself completes every table, key and value, explains each on
hover, and reports a bad value where it is written.

## Config reference

`schema_version = 1` is optional; 1 is the only version. Unknown fields,
rules, enum values, malformed regular expressions, and unsupported schema
versions are errors. Each group accepts `severity = "error"`, `"warning"`, or
`"off"`; the default is `"error"`, except for `rules.examples` and `rules.intent`, whose default
is `"warning"`. Omitted static groups run no checks.

Every check also takes its own severity. Write it bare to use the group's,
or as a table naming its value and severity:

| Check | Bare | With its own severity |
|---|---|---|
| a naming or path case | `members = "camelCase"` | `members = { case = "camelCase", severity = "warning" }` |
| a naming pattern | `operations_pattern = "Get.*"` | `operations_pattern = { pattern = "Get.*", severity = "warning" }` |
| the path prefix | `prefix = "/v1"` | `prefix = { path = "/v1", severity = "warning" }` |
| a documentation target | `targets = ["operations"]` | `targets = [{ target = "errors", severity = "warning" }]` |
| a compatibility family | `wire = true` | `sdk_source = { enabled = true, severity = "warning" }` |

| Group | Options |
|---|---|
| `rules` | `namespaces`: exact namespaces to check; empty defaults to the selected service's namespace |
| `rules.naming` | `operations`, `shapes`, `members`, `enum_values`, `enum_wire_values`: optional casing conventions (`enum_values` checks the value's name; `enum_wire_values` checks what goes on the wire for string enums: the explicit `ACTIVE = "active"` value, or the name when bare; integer enums are skipped); corresponding `*_pattern` fields add whole-name Rust regex checks |
| `rules.paths` | `prefix`: absolute literal path with segment-boundary matching; `segment_case` checks literals; `parameter_case` checks labels |
| `rules.documentation` | `targets`: any of `service`, `operations`, `shapes`, `members`, `errors`, `enums`; empty means no documentation checks |
| `rules.intent` | Nonempty `Use when` and `Don't use when` operation documentation sections; severity defaults to `"warning"` |
| `rules.examples` | `operations`, `errors`: an `example` on each operation, and one showing each error the operation declares; both default to true, and the group's severity defaults to `"warning"` |
| `compatibility` | `wire`, `sdk_source`: independently select wire and model-level public-symbol checks; both default to true |

### Operation intent

Enable guidance coverage with an opt-in table:

```toml
[rules.intent]
severity = "warning"
```

`intent.operations` reports each selected operation without both nonempty
[`## Use when` and `## Don't use when` sections](idl/reference.md#operation-intent).
It reads the compiler's extracted metadata. A heading inside a code example does
not satisfy the check. Missing or incomplete guidance is a warning by default;
`severity = "error"` requires it, and `"off"` disables the check. The existing
namespace selection and `[[exceptions]]` mechanism apply. This group leaves
`documentation.required` and its configured severity unchanged.

### Example coverage

A reference is easiest to use when each operation shows a real request and
response, and what each of its errors looks like. `[rules.examples]` checks
both, as warnings unless the group or a check says otherwise, so a model
can adopt it before every operation has an example:

```toml
[rules.examples]
operations = true                                # examples.operations
errors = { enabled = true, severity = "error" }  # examples.errors
```

`examples.operations` reports each selected operation that declares no
`example`; its reference page shows a [generated sample](docs.md#generated-samples)
instead. `examples.errors` reports, at the operation, each error it
declares that none of its examples shows with `error Name { .. }`. The
service's own errors (`errors [..]` on the service, such as throttling or
validation) are shared by every operation and not asked of each; whether
an error is documented is `[rules.documentation]`'s `errors` target.
Either can be excepted like any other finding, such as a health check that
needs no example.

Naming patterns are wrapped in `\A(?:...)\z` and validated in that form.
Verbose-mode (`(?x)`) comments need a terminating newline before the pattern
ends; otherwise they consume the wrapper and produce a configuration error.

Naming cases are `PascalCase`, `camelCase`, `snake_case`,
`SCREAMING_SNAKE_CASE`, and `kebab-case`. Casing and regex requirements both
apply when configured together. Shapes and members are the public types
reachable from the selected service's operations, including modelled errors.
Trait definitions, unreachable shapes, and resource-only provisioning metadata
are outside this API policy surface. Imported namespaces are excluded from
static checks unless explicitly selected; compatibility checks include all
reachable types regardless of namespace.

Static checks evaluate the current model in full. Existing violations can be
introduced as warnings or explicitly excepted during adoption. They do not
depend on the CI diff to select changed shapes.

## Reading a `.cru` package's compiled model

Scripts, gates and build steps that read the contract want the lowered model,
not the source: resolved names, defaults, HTTP bindings, and the package's own
`metadata` and `///` documentation. `crucible compile --json <package>`
prints it (`--out <file>` writes it instead; `--service` picks one of several):

```sh
crucible compile --json model/api --out target/model.json
```

The JSON is the release snapshot format (`crucible freeze` writes the same
document, and `crucible_ir::snapshot::read` reads it back), in full: every
operation's `traits` map holds the package's metadata under its namespaced key
(`example.service#operationClass`), and operations, shapes, members and enum
values carry `docs`. `crucible generate ts` writes `crucible-model.json` for
the TypeScript package, which is the compact contract without either.

A build script gets the same file from `crucible_build`: a `.cru` package
passed to `model(..).server().model_output(path)` (or the
`CRUCIBLE_MODEL_OUT` environment variable) exports its compiled model there,
rewritten only when it changes so a watched export does not rerun the build.

## Guarding a `.cru` package against its last release

A Crucible language package can name its last release as a baseline. Every
build, `crucible check`, `crucible generate` and the editor then compare the
package against it, so a breaking change fails where it is made instead of
when someone remembers to run `crucible diff`.

Freeze the release when you ship it, and point the manifest at it:

```sh
crucible freeze model/api          # writes model/api/releases/<service version>.json
```

```toml
[model]
namespace = "example.service"

[model.baseline]
path = "releases/2026-09-04.json"
```

### Releases

A frozen release is a snapshot of the package's **compiled** model, not a
copy of its sources: the lowered model as deterministic, pretty-printed
JSON, so a release's file diffs as the API changed. Builds read
it straight back; an older release is never recompiled, so a later
compiler or language change cannot change what a shipped contract means.
The file records its `format` and the compiler that wrote it; a compiler
reads every format up to its own, and refuses a newer format or a field it
does not know (one at its zero value, such as `false` or `[]`, reads as
absent) rather than misread the release. The snapshot's field names are the
IR's: renaming an IR field, or adding one without a `serde` default, is a
format change.

`freeze` refuses to overwrite a release, and records each one's SHA-256 in
`releases/crucible.lock`. A build refuses a release that is not in the lock
or no longer matches it, which catches an accidental edit (it is not
tamper protection: rewriting the lock too passes). A release is never
edited, only superseded by a new one. Commit the snapshots and the lock
with the release. A release's version names its file, so it uses letters,
digits, `.`, `-`, `_` and `+`, and does not start with `.`.

A release that was never frozen is backfilled from history:

```sh
crucible freeze model/api --from v2026-09-04   # any git commit or tag
```

`--from` reads the package's files as they were at that commit, compiles
them with the current compiler (ignoring that commit's own
`[model.baseline]`), and freezes the result like any other release. It needs the package to be in a git repository; nothing else
does, so builds work offline and from a published crate.

The comparison is the one `crucible diff` runs,
with the package's `[compatibility]` settings and `[[exceptions]]`: a
finding at `error` fails the build, one at `warning` is printed. In the
editor, each breaking change shows at the declaration it concerns, or, for
something removed, at what held it; a baseline that does not load shows on
`[model.baseline]`.

### Declaring a breaking change

A deliberate break compiles when the migration from the baseline's version
declares it. A release is named by its service's `version`: `crucible freeze` writes
it to `releases/<version>.json`, and the compiler checks that a release's
file name and its service's `version` agree. The migration from
it names both versions, and its new version must be the current service's
`version`, so a baseline at `releases/2026-09-04.json` is served by
`migration "<the service's version>" from "2026-09-04"`:

```
/// Region becomes a required choice; storage is reported as one structure.
migration "2026-11-01" from "2026-09-04" {
    rename GraphData.manifestGeneration -> generation
    require CreateIndex.input.kind default BTREE
    add GraphStatus.RETIRING downgrade DELETED
    add CreateGraph.errors GraphNameReservedException downgrade ConflictException {
        message <- message
    }
    move RevertGraph from POST "/v1/graphs/{graphId}/revert"
    reshape GraphData { sizeBytes, indexBytes } -> storage by hook
    retype Usage.cost -> Money by hook
    remove operation GetWrite
    narrow role graphWriter
    behavior ListGraphs.excludesRetiring
    sunset "2028-11-01"
}
```

The compiler checks the migration both ways. A breaking change no step
declares still fails, and each step must match what changed between the
baseline and the current model: a `rename` whose old member is still there,
a `move` whose old route was not the baseline's, an `add` of a value that
already existed, or a `remove operation` of an operation that still exists
is an error at that step. `remove role`, `remove scope` and `narrow role`
declare the role findings ([below](#comparing-contracts)): a role or scope
whose value names nothing now, and a role whose holders lose something (a
`narrow role` whose role lost nothing is an error too). A role that
includes a narrowed one narrows with it and needs its own step. A widening
is a warning and needs none. A change of `internal`, or a scope moved to
another resource, is declared by an exception. A `rename` declares the new
name alone: any
other change to the member, such as an optional input that became
required, needs its own step, which names the member as it is now
(`require CreateIndex.input.kind default ..` after a rename to `kind`).
Paths name a shape (`GraphData`), a member
(`GraphData.storage`), or an operation's input or output
(`CreateIndex.input.kind`). One release has one migration; older ones stay
in the sources, for the server that serves them; steps that need code
generate hooks ([API versions](api-versions.md#hooks)).

When the release ships, freeze it and point `[model.baseline]` at the new
snapshot; the next release's migration starts from it.

## Comparing contracts

Compare the `.cru` packages of two revisions directly:

```sh
crucible diff before/model after/model --config crucible.toml --json
```

The CLI compiles both package directories itself, with no Cargo service,
generated code or lockfile. Each directory is a package: its `crucible.toml`
and sources, with any `[model.dependencies]` it imports by path. Invalid
sources or missing directories fail with compiler diagnostics.

Use `--service namespace#Service` when the packages contain several services.
Without `--config`, `diff` uses the default compatibility settings.
An error finding returns a nonzero exit code. With `--json`, stdout remains
a valid JSON report even when findings fail the check; setup/model errors go
to stderr and fail without a report. Finding order is deterministic.

The wire checker assesses old clients calling a new service. It checks removed
operations, methods/paths/statuses, effective authentication, input/output
bindings, member removal, required inputs, response nullability, defaults,
scalar/collection types, enums/unions, constraints, timestamp formats, modelled
errors, idempotency declarations, and existing pagination contracts. Request
and response usage are tracked independently, including shared recursive types.

An optional new input or input enum value is wire-compatible. A new required
input without a default, a removed accepted enum value, or a new output enum
value is reported. Input bounds must continue accepting old requests; output
bounds must preserve the old response guarantees. Changed regex constraints
are conservative findings when inclusion cannot be proven. Member removal is
conservative because some readers reject unknown fields. New modelled errors
are reported for callers relying on the previous exhaustive error contract.

Wire coverage currently supports Crucible JSON. Unassessed advanced features produce `compatibility.unchecked` findings.
They require an explicit assessment or exception; they do not silently pass.

Permissions ([permissions.md](permissions.md) §5) are stored data: grants and
audit records carry a resource's authorization name and a permission's
value. Removing a permission or an alias, changing a value, and changing an
authorization name (a resource shape renamed without `@authorizationName`
keeping the old one) are `compatibility.permissions` findings. So is
changing which permission a stored value satisfies: two values swapped, a
value moved to another released permission, an alias pointed at another
primary, or two released permissions folded into one new primary. Renaming
a permission by adding the new primary and keeping the old value as a
deprecated `@aliasOf` alias is additive. Requirements are compared by the
stored pair (authorization name, value), not by symbol. Adding or
tightening a requirement on a released operation, including replacing
`requires none` or `requires pending`, is a `compatibility.authorization`
warning: reported, never refused, because it is how holes get closed. The
`crucible.errors#NotFoundException` such a requirement attaches is part of
that change, not a `compatibility.error` or `compatibility.sdk` finding.

Roles and scopes ([roles.md](roles.md) §6) are stored data too: a binding
stores a role's value and a scope's value. They are compared by value,
and grants by the stored pair, so a symbol renamed with its value kept is
the same role, and a grant whose value is now a deprecated alias of a
primary the role grants is kept. Findings are reported at the role or
scope, as `namespace#symbol` (`example.api#graphWriter`), which is what an
exception names.

- `compatibility.scope` (error): a scope removed, its value changed, or
  moved to another resource, or between one instance and `R(*)`, since a
  stored binding names an instance of what it was on; and a scope whose
  instance is named another way: its resource, or one above it, moved
  under another parent, or an identifier the instance carries renamed.
  The instance's path (each resource from the root, with the identifiers
  it adds) is compared whether or not an operation's route shows the
  change. A resource is the same one when its authorization name is, if
  both releases give it one, and otherwise when its shape is: one that
  declares its first permission has not moved.
- `compatibility.role` (error): a role removed or its value changed; a role
  that no longer grants a permission it granted (its own grants, an
  included role's, or a `*` that covers less, `by service` grants
  included), that can no longer be bound at a scope it could, or that no
  longer admits a kind of principal it did (`holders`), or one whose
  binding at a scope it keeps no longer covers a resource the model still
  grants on (a `by service` grant applies whatever the scope covers),
  because the resource moved out from under the scope's. Its holders are
  refused what they had. So is a role whose `assignedBy` no longer names a
  role it named, or names roles where it named none (whoever the binding
  operation admits): an assigner can no longer grant or revoke it. A
  permission or scope that is gone altogether is
  reported once, by `compatibility.permissions` or `compatibility.scope`,
  not again for each role. Admitting more kinds grants no holder anything
  and is not reported.
- `compatibility.role` (warning): a grant the model decided now left to
  the service (`by service`). Holders keep it where the service's hook
  allows, which only a review of the hook can tell.
- `compatibility.role-widened` (warning): a role that grants more, by an
  edit, through a role it includes, because its `*` covers a permission
  added since, because a grant the service decided is now the model's, or
  because a binding at one of its scopes now covers a resource it grants
  on that moved in beneath the scope's.
  Widening is a security review, not a break: reported, never refused.
  So is a role more roles may assign (`assignedBy` naming another, or
  dropped so the binding operation decides), and one bound at a scope it
  could not be: no holder gains anything, but whoever may assign it can
  now grant it there.

A baseline frozen before roles existed has no roles to compare, so adding a
catalogue reports nothing.

`sdk_source` checks model-level public types, operations, members, enums,
collection representations, service/client identity,
and permission, role and scope constants: one removed, or moved to
`internal`, is a `compatibility.sdk` finding. Making one public again is
additive.
A new optional member (not required, no default) added after a structure's
existing members passes, unless the structure is an error: generated clients
accept it without a source change (Rust models are `#[non_exhaustive]` and
built with builders; other languages use builders, named or keyword
arguments, or trailing optional parameters). Code that constructs models
positionally where the language discourages it (Go unkeyed composite
literals) and Rust code that includes the bindings with `include_client!()`
and uses struct literals in its own crate can still break. An error
structure's members are always reported: C++ and PHP error constructors take
their metadata after the members. Other additions are conservative findings: a required or
defaulted member is a new constructor argument in most languages, one
inserted between existing members shifts positional arguments (C, C++, PHP,
R), and a union arm breaks exhaustive matches. This is not proof that every generated language's source/ABI remains
compatible. Generator version changes, service behavior, dependencies, and
target-specific consumer code need their own tests.

## Excepting a finding

Exceptions match an exact rule ID and shape/member ID, and remain visible in
reports. They apply to all findings with that rule and location, so review the
scope of the exception. A nonempty reason is required:

```toml
[[exceptions]]
rule = "compatibility.http"
shape = "example.api#GetItem"
reason = "Intentional /v2 migration, reviewed with the service major release"
```

Stable rule IDs are published by `crucible_policy::RULE_IDS`.
An exception does not suppress model validity errors.

## Using the shared GitHub workflow

The [crucible-actions repository](https://github.com/Operation-Leyline/crucible-actions)
contains `setup-crucible`, the `api-diff` composite action, a reusable workflow,
and a caller template. Setup reads the service's `Cargo.lock`, installs the
matching CLI, and adds `crucible` to PATH. Pin the action to a reviewed commit;
the lockfile selects the compiler. Public tooling needs no App credentials.

Packages with a frozen baseline already check compatibility during builds and
`crucible check`; the shared comparison workflow is optional reporting.
Select the PR base revision for regression checks or a released tag to preserve
compatibility with the published contract. No baseline means a failed check.

Pass `before` and `after` as `.cru` package directories relative to the runner
workspace. The policy defaults to the proposed package's `crucible.toml`;
set `config` only to select another workspace-relative policy file. The action
compiles and compares those directories; it does not run service builds or
require an export script.

The action uploads JSON and Markdown reports even for breaking findings and
adds the report to the run summary. Ordinary PR builds enforce static policy;
the compatibility job compares the old and proposed contracts.

HTTP/error conventions, pagination requirements, constraint-presence policies,
and idempotency requirements can be added as static rule groups later. Unknown
groups fail config parsing until implemented.
