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
    crucible_build::from_smithy("smithy")
        .policy("crucible.toml")
        .generate();
}
```

The same `.policy(...)` method works with `service(...)`, `client(...)`,
`Codegen::from_model(...)`, and `from_smithy_with(...)`. Paths are relative to
the package's `Cargo.toml`; a workspace can select `../../crucible.toml`.
The selected config is required. Invalid config, missing config, or error
findings fail the build. Cargo tracks config edits and removal. Warnings and
excepted findings are printed without blocking generation.

New `crucible new` starters include the config and build hook. Existing
packages opt in with `.policy(...)`. Explicit selection avoids watching a
missing optional config, which would force Cargo to rerun the build script
on every build. Normal builds require no baseline, Git history, or CLI install.

## Config reference

`schema_version = 1` is required. Unknown fields, rules, enum values, malformed
regular expressions, and unsupported schema versions are errors.
Each group accepts `severity = "error"`, `"warning"`, or `"off"`; the default
is `"error"`. Omitted static groups run no checks.

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
Additions can affect construction or exhaustive matching and are conservative
findings. This is not proof that every generated language's source/ABI remains
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
