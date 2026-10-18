# Generating infrastructure packages

Crucible generates CDK construct libraries and Terraform/OpenTofu providers from
the same resource model. Deployment bindings select the supported surface
and supply policy that an HTTP API cannot establish.

## Architecture

```text
crucible.toml + .cru sources
  -> crucible-lang: operations, shapes, resource graph and lifecycle mappings
  -> crucible-codegen-infra: validated provisioning bindings
     -> crucible-codegen-cdk: TypeScript constructs -> jsii language packages
     -> crucible-codegen-terraform: Go provider using the generated Go client
```

The shared resource IR preserves identifiers, properties, nested resources,
lifecycle slots and collection operations. It resolves the member paths that
`$name` elisions and `for` views bind (decisions 33, 34 and 67 in
[idl/decisions.md](idl/decisions.md)) before a generator runs. Collection
element mappings remain separate from instance paths. Generators never read
the source; they read the IR. Resource references (`references`, decision 34b)
are not in the language yet.

The CDK and Terraform targets read the same lowered service as SDKs and
servers, which carries the resources but no shape that only a resource names;
their provisioning diagnostics are their own.

CDK packages bind to an existing custom-resource provider. They do not deploy a
service backend, create a provider Lambda, or implement CloudFormation lifecycle
handlers. Terraform packages implement their supported lifecycle subset using
Terraform's Plugin Framework and Crucible's Go client runtime.

## Deployment bindings

The JSON manifest selects resources by absolute shape ID. Unknown fields,
missing member paths, invalid refinements, and unbound resources are errors.
For example:

```json
{
  "resources": {
    "example#City": {
      "name": "City",
      "properties": {
        "Name": {
          "source": { "shape": "example#CreateCityInput", "path": ["name"] },
          "required": true,
          "immutable": true
        }
      },
      "attributes": {
        "Id": { "source": { "shape": "example#CreateCityOutput", "path": ["id"] } }
      },
      "cdk": {
        "resource_type": "Custom::WeatherCity",
        "context": {
          "SecretArn": {
            "target": "crucible.api#String",
            "required": true,
            "length": { "min": 1 }
          }
        }
      }
    }
  }
}
```

`source` resolves a structure member path against the IR, including its effective
constraints and enclosing sensitivity. Only provider context can declare a scalar
`target` without a source member. Sensitive attributes are rejected, including
aggregates containing sensitive descendants.

`required` describes the provisioning contract; API inputs and infrastructure
inputs are not always identical. `immutable` is explicit provider policy. A
selected member whose mutability is create-only is immutable too: one bound to
an `immutable` property (decision 34a), or marked `@mutability(create)` or
`@mutability(createAndRead)`.

Optional refinements are `enum_values` (a nonempty subset of a modeled enum),
`integer` (numeric types only), `length` (narrower bounds), and `timestamp_format`
(`date-time`, `http-date`, or `epoch-seconds`). Timestamp overrides describe the
provider's accepted representation; they do not change the service HTTP model.
An immutable annotation documents CDK replacement policy; its existing provider
must implement that policy. Terraform uses replacement plan modifiers.

## Generating CDK constructs

This complete example uses the checked-in test model and publishing metadata:

```sh
cargo run -p crucible-cli -- generate cdk \
  tests/models/cdk-model \
  --bindings crates/crucible-codegen-cdk/tests/fixtures/bindings.json \
  --config crates/crucible-codegen-cdk/tests/fixtures/options.json \
  --out generated/weather-cdk
cd generated/weather-cdk
npm install
npm run build
npm run package
```

The config names the npm package, version, author, repository, license, and optional
jsii Python, Java, .NET, and Go targets. The fixture demonstrates every target.
Generation does not publish packages or infer registry ownership.

From a Cargo build script, enable `crucible-build`'s `cdk` feature and use
`model.cdk("constructs")` with `.bindings(...)` and publishing settings.
`.python(distribution, module)`, `.java(package, group_id, artifact_id)`,
`.dotnet(namespace, package_id)`, and `.go(module, package)` select optional
jsii language packages within the same output. See [building model outputs](build-outputs.md#cdk-language-packages)
for a complete example and the separate native packaging step.

The package contains `Cfn<Resource>` classes, props interfaces, named nested
property structs, string attributes, and internal validation. Public types follow
jsii restrictions. Enums are strings with validation so unresolved CDK string
tokens remain usable. CloudFormation property spelling comes from the binding;
the TypeScript property starts with a lowercase letter. `serviceToken` is always
required and serializes as `ServiceToken`; other provider context is explicit.

```typescript
import { App, Stack, RemovalPolicy } from 'aws-cdk-lib';
import { CfnCity } from '@example/weather-cdk';

const stack = new Stack(new App(), 'Example');
const city = new CfnCity(stack, 'City', {
  serviceToken: 'arn:aws:lambda:us-east-1:123456789012:function:provider',
  secretArn: 'arn:aws:secretsmanager:us-east-1:123456789012:secret:deployment',
  name: 'Portland',
});
city.applyRemovalPolicy(RemovalPolicy.RETAIN);
// city.attrId is a deployment-time reference.
```

Concrete inputs receive type, enum, nested member, length, range, pattern, and
uniqueness checks. Unresolved tokens defer validation to the deployment provider.
Unsupported properties such as unions, blobs, and documents produce diagnostics.
Low-level constructs choose no removal policy. Keep handwritten convenience
constructs outside the generated directory: generation rewrites that directory.

jsii supplies Python, Java, .NET, and Go bindings from the TypeScript library.
All use its JavaScript implementation and need a supported Node.js runtime.
Rust, Swift, Dart, and other SDK languages are not native AWS CDK targets.

## Generating a Terraform/OpenTofu provider

```sh
cargo run -p crucible-cli -- generate terraform \
  tests/models/terraform-inventory \
  --bindings crates/crucible-codegen-terraform/tests/fixtures/bindings.json \
  --module-path example.com/terraform-provider-inventory \
  --provider-name inventory \
  --provider-address registry.terraform.io/example/inventory \
  --out generated/terraform-provider-inventory
cd generated/terraform-provider-inventory
go mod tidy
go build -o terraform-provider-inventory .
```

The provider uses protocol version 6. Configure the service `endpoint` and either
`bearer_token` or the `CRUCIBLE_TOKEN` environment variable. Credentials require
HTTPS, with loopback HTTP allowed for local tests. Provider diagnostics omit raw
server messages, response bodies, and credentials.

A resource opts in with an explicit `terraform` binding:

```json
{
  "name": "widget",
  "synchronous": true,
  "read_after_write_consistent": true,
  "not_found_errors": ["NotFound"]
}
```

The initial subset requires one server-generated string identifier; create,
read, and delete lifecycle operations; string, enum, boolean, integer, or double
properties; and a read operation that observes every managed property.
Optional mutable properties are refused
until their null/reset behavior can be expressed. Resources without update
must mark every property immutable. Composite identities,
eventual consistency, and non-scalar property mappings need additional support.
Generation diagnoses unsupported contracts before producing a provider.

Property sources must point to their create input mappings. Their types,
constraints, and sensitivity must agree across lifecycle operations. Pattern
constraints, float32 properties, and integer refinements of floating-point
types are currently refused. Resource properties use lowercase Terraform
identifiers; reserved names such as `count` cannot be generated.

An omitted optional immutable property stays null in state even if the service
supplies a default. That omitted value is unmanaged; an explicitly configured
value is refreshed normally. Import refreshes required properties and identity,
while optional properties remain unmanaged until configured.

An explicit `id` attribute can identify a nested output path when the create and
read operations share that output shape. Other computed attributes are outside
the initial subset. Not-found errors must be modeled by the read and delete
operations; an arbitrary HTTP 404 does not authorize dropping state.

Requests use the generated Go client with one attempt. A missing create response
can leave an object without a known ID: reconcile the service before retrying.
Once creation returns an ID, the provider saves it before refreshing, preserving
recovery even if the subsequent read fails. Import takes that resource ID.

Install the compiled provider using the engine's provider distribution or local
development-override mechanism. `tools/test-terraform.py` demonstrates an isolated
development override and full lifecycle against a fake HTTP service. No registry
publication or service credentials are needed for that test.

## Migrating custom CDK providers

[`examples/infra/graph.bindings.json`](../examples/infra/graph.bindings.json)
maps resource surfaces (`CfnGraph`, `CfnIndex`, and `CfnGraphBackupSchedule`) to
a custom CloudFormation provider.

Compatibility qualification proves construct synthesis compatibility, not that an older backend speaks the current Crucible wire protocol. Neither backend source nor credentials are packaged.

Keep the existing provider implementation, resource type names, physical-ID
decoder, construct paths, attribute names, dependencies, and retention policies.
Compare synthesized templates before replacing an existing generator. Roll back by
restoring the previous construct package and provider together. Do not migrate
CloudFormation-managed objects into Terraform implicitly; the engines own
different state and such a move needs a separate adoption plan.

## Validation and boundaries

```sh
cargo test
cargo clippy --all-targets -- -D warnings
python tools/test-cdk.py
python tools/test-cdk.py --language all
python tools/test-terraform.py
```

CDK qualification installs the packed npm package in a separate consumer,
generates all four jsii language bindings, and compares synthesized templates
from selected language consumers. The default consumers are TypeScript,
JavaScript, and Python. `--language all` also requires Maven, .NET, and Go.
Missing requested toolchains fail instead of silently reducing coverage.

Set `TERRAFORM` to a Terraform or OpenTofu binary to add real plugin loading,
schema validation, create, update, replacement, no-op plans, and destroy tests.
Without it, the harness still compiles the provider and tests its framework
handlers over HTTP. Neither path contacts a production service.

Native CloudFormation registry handlers, a published Pulumi bridge, arbitrary
language template builders, service-specific CDK provider generation, and live
CloudFormation deployment qualification are separate follow-up work. Terraform
provider compatibility does not by itself prove Pulumi bridge compatibility.

## References

- [jsii public API restrictions](https://aws.github.io/jsii/user-guides/lib-author/typescript-restrictions/)
- [CloudFormation custom-resource lifecycle](https://docs.aws.amazon.com/cdk/api/v2/python/aws_cdk.custom_resources/README.html)
- [Terraform resource plan modification](https://developer.hashicorp.com/terraform/plugin/framework/resources/plan-modification)
- [Pulumi provider bridge](https://www.pulumi.com/docs/iac/concepts/providers/)
