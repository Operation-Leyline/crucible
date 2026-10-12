# API policy and compatibility

Crucible checks project API conventions during Cargo builds and compares
contracts with `crucible diff`. Both use `crucible-policy` over the lowered IR.
Model validity and supported Smithy constructs remain `crucible-smithy`'s
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
    crucible_build::model("smithy").server()
        .policy("crucible.toml")
        .generate();
}
```

The same `.policy(...)` method works with `model(...).server()`, `model(...).client()`,
`model("model.json").server()`, and `model("smithy").compile_with(...).server()`. Paths are relative to
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
`"off"`; the default is `"error"`. Omitted static groups run no checks.

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
| `rules.naming` | `operations`, `shapes`, `members`, `enum_values`: optional casing conventions; corresponding `*_pattern` fields add whole-name Rust regex checks |
| `rules.paths` | `prefix`: absolute literal path with segment-boundary matching; `segment_case` checks literals; `parameter_case` checks labels |
| `rules.documentation` | `targets`: any of `service`, `operations`, `shapes`, `members`, `errors`, `enums`; empty means no documentation checks |
| `compatibility` | `wire`, `sdk_source`: independently select wire and model-level public-symbol checks; both default to true |

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
    behavior ListGraphs.excludesRetiring
    sunset "2028-11-01"
}
```

The compiler checks the migration both ways. A breaking change no step
declares still fails, and each step must match what changed between the
baseline and the current model: a `rename` whose old member is still there,
a `move` whose old route was not the baseline's, an `add` of a value that
already existed, or a `remove operation` of an operation that still exists
is an error at that step. Paths name a shape (`GraphData`), a member
(`GraphData.storage`), or an operation's input or output
(`CreateIndex.input.kind`). One release has one migration; older ones stay
in the sources, for the server that serves them; steps that need code
generate hooks ([API versions](api-versions.md#hooks)).

When the release ships, freeze it and point `[model.baseline]` at the new
snapshot; the next release's migration starts from it.

## Comparing contracts

Compare the Smithy source directories from two revisions directly:

```sh
crucible diff before/smithy after/smithy --config crucible.toml --json
```

The CLI assembles both directories with its pinned official Smithy compiler,
imports the bundled Crucible traits and errors, and flattens mixins before
lowering. Compilation uses temporary directories outside the sources and does
not need a Cargo service, generated code, or lockfile. Each source directory
must contain its API and any additional model dependencies. Invalid Smithy or
missing directories fail with compiler diagnostics. The managed Java runtime
is downloaded and checksum-verified on first use, then cached.

Assembled Smithy JSON ASTs remain supported:

```sh
crucible check new.json --target rust --config crucible.toml
crucible diff old.json new.json --config crucible.toml
crucible diff old.json new.json --config crucible.toml --json
```

Use `--service namespace#Service` when the models contain several services.
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

Wire coverage currently supports Crucible JSON and AWS restJson1. Other
protocol families, streaming, checksums, endpoint rules/discovery, and other
unassessed advanced features produce `compatibility.unchecked` findings.
They require an explicit assessment or exception; they do not silently pass.

`sdk_source` checks model-level public types, operations, members, enums,
service-local renames, collection representations, and service/client identity.
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
and a caller template. Pin both the action and checker to reviewed commits.
Select the PR base revision for regression checks or a released tag to preserve
compatibility with the published contract. No baseline means a failed check.

Pass `before` and `after` as Smithy directories relative to the runner workspace,
plus `config` pointing to the proposed policy file. The action assembles and
compares those directories; it does not run service builds or require an export
script. JSON AST paths are also accepted. Older service build helpers do not
affect this source-directory comparison.

The action uploads JSON and Markdown reports even for breaking findings and
adds the report to the run summary. Ordinary PR builds enforce static policy;
the compatibility job compares the old and proposed contracts.

HTTP/error conventions, pagination requirements, constraint-presence policies,
and idempotency requirements can be added as static rule groups later. Unknown
groups fail config parsing until implemented.
