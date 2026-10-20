# JSON Schema export

Crucible exports a standalone JSON Schema Draft 2020-12 document for a named
shape in the selected service. The exporter consumes the same lowered,
audience-projected IR and schema engine as OpenAPI and API references.

```sh
crucible generate json-schema model --shape example.render#RenderPlan --out render-plan.schema.json
```

`--shape` accepts a fully qualified `namespace#Name` or an unambiguous original
shape name. Service renames do not change these identities. A missing, invalid,
ambiguous, unreachable, or audience-excluded root is a diagnostic; ambiguous
names list their fully qualified candidates. Only shapes reachable in the
selected service's lowered IR are available. Select the service with `--service`
and a release with `--api-version` as for other generators.

The default audience is external. `--audience internal` includes internal shapes
and members; audience projection happens before root selection. No server-only
shape is restored to satisfy a reference. The CLI's `--out` names a file in an
existing directory; generation diagnostics leave it unchanged.

## Cargo build scripts

Enable the `json-schema` feature of `crucible-build`:

```toml
[build-dependencies]
crucible-build = { version = "0.8", features = ["json-schema"] }
```

```rust
let model = crucible_build::model("model").compile();
model.json_schema("render-plan")
    .shape("RenderPlan")
    .generate_into("exports/render-plan");
```

Both `Model` and `ModelPackage` expose `json_schema`. The named output writes
`schema.json`, publishes an artifact with target `json-schema`, and uses the
same ownership and export rules as other document targets. It supports
`audience`, `options(JsonSchemaOptions { shape })`, `generate`, `try_generate`,
`generate_into` and `try_generate_into`. Shared codegen callers can enable the
`json-schema` feature and use `Generator::JsonSchema(JsonSchemaOptions { shape })`.
`all-outputs` includes the target. Adding the target changes no default outputs.

## Schema contract and limits

The document declares `$schema: https://json-schema.org/draft/2020-12/schema`.
It inlines the selected root and carries its reachable dependency closure under
`$defs`, using deterministic `namespace.Name` keys and local references. There
is no invented deployment identity (`$id`). Recursive shapes remain finite.
Whole operation inputs and outputs describe document values, including members
bound to HTTP labels, headers, or payloads; they are not HTTP body projections.

The shared engine preserves effective constraints, required and nullable
members, defaults, unions with exactly one arm, maps, collections, enums,
numeric widths, and document timestamp formats. Defaults annotate a schema;
validators do not insert them. Additional structure properties follow the IR's
strictness; union arms reject extra properties.

The export uses the shared `x-crucible-*` annotations documented for
[OpenAPI](openapi.md#extension-metadata). Generic validators do not enforce those
annotations: decoded blob byte length, base64 encoding, and nonstandard timestamp
formats require application validation. Arbitrary precision number handling and
regular expression support depend on the validator. HTTP routing and header
encoding capabilities do not restrict a document schema. Unsupported IR
capabilities remain diagnostics rather than silently omitted constructs.
