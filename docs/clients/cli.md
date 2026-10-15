# Generating a service CLI

Crucible generates a standalone Rust command-line package from the same Smithy
model as its SDKs. Commands call the generated Rust client, which owns HTTP
bindings, bearer authentication, retries, modeled errors, and pagination.

## Generate and build

Run from a Crucible checkout with Rust installed:

```sh
cargo run -p crucible-cli -- generate cli \
  crates/crucible-smithy/tests/fixtures/weather.json \
  --bin-name weather --out e2e/generated/weather-cli --runtime-path .
cargo build --manifest-path e2e/generated/weather-cli/Cargo.toml
```

`--runtime-path` identifies the Crucible checkout supplying the unpublished
`crucible-client-runtime-rust` and `crucible-cli-runtime` dependencies. Generation resolves
it to an absolute path. Keep that checkout available while building; the built
executable does not need it. `--package-version` defaults to `0.0.0`.
`--endpoint` sets an optional default endpoint in the generated descriptor.

The output directory belongs to generation. Put handwritten extensions in a
separate package. The generated package exports its SDK and a dispatch function;
a wrapper can parse an invocation, replace its client configuration, and dispatch
it with custom token providers or interceptors.

Build a binary for each target operating system and architecture. Distributing
signed installers and publishing packages are separate release tasks.

## Commands and input

Operation and member names become kebab-case commands and flags:

```sh
weather --help
weather get-city --help
weather get-city --city-id c-00000001 --endpoint-url http://127.0.0.1:8080
weather create-city --generate-input
weather create-city --input-json file://request.json
weather create-city --input-json -
```

The last command reads JSON from standard input. Help, version, completion,
and input templates work without an endpoint or credentials.

`--input-json` also accepts a literal JSON object. Explicit member flags override
the corresponding top-level JSON members. Nested objects, lists, maps, unions,
and document values use JSON syntax. String flags are literal strings;
booleans accept explicit `true` or `false`. Use JSON input to send an explicit
null. Omission, null, false, zero, and empty values retain their distinctions.
Use files or stdin for complex input to avoid shell-specific quoting.

Unknown input object members are rejected locally. Scalar decoding uses the
generated SDK. Client omission follows the shared IR's nullability rules;
service validation remains authoritative for required inputs and constraints.
Input templates are editable examples, not guaranteed valid requests: required
values, constraints, recursive shapes, and service-specific rules may need edits.

Blob flags accept base64 or `fileb://PATH` to read raw bytes. Blob values in
JSON responses are base64, including responses whose HTTP body is a raw payload.

## Configuration and authentication

For a binary named `weather`, use `WEATHER_ENDPOINT_URL` and
`WEATHER_BEARER_TOKEN`. The prefix comes from the binary name, uppercased with
hyphens replaced by underscores. Credentials are supplied through configuration
or environment, rather than a command-line token flag.
When a bearer token is configured, the endpoint must use HTTPS, except for
local development on `localhost`, loopback IPv4 addresses, or `::1`.

Pass `--config profiles.json --profile development` with a configuration file:

```json
{
  "profiles": {
    "development": {
      "endpoint_url": "http://127.0.0.1:8080"
    }
  }
}
```

A profile may also contain `bearer_token`. `WEATHER_CONFIG` and `WEATHER_PROFILE`
select the file and profile when flags are absent. An explicit configuration
file defaults to the profile named `default`; profiles are not discovered from
a home directory. A named profile requires a configuration file.

Endpoint precedence is command flag, environment, profile, then generated
default. The bearer-token environment variable overrides the profile token.
The SDK sends credentials only for operations whose effective authentication
requires them. Browser login and credential refresh flows belong to the service's
wrapper and can use the SDK's token-provider interface.

## Output, pagination, and failures

`--output json` is the default. An ordinary call returns one JSON response.
A paginated operation retrieves all pages and writes one array of complete page
objects. Page metadata stays with its page; the CLI does not infer how to merge
arbitrary output members. `--no-paginate` returns only the first response object.

`--output jsonl` writes each complete page as one JSON line. This streams pages,
while JSON output buffers them. If a later page fails, JSON mode emits no partial
result; JSON Lines may already have emitted earlier pages. Both modes exit with
a failure code. For large result sets, use JSON Lines or explicit service token
and page-size members. There is no CLI-level `--max-items` continuation format.

Results go to stdout and failures to stderr. Exit codes are:

| Code | Meaning |
|---|---|
| `0` | Success, help, version, completion, or input template |
| `1` | Request, response, or output failure |
| `2` | Invalid arguments, input, or configuration |

Generate shell completion without contacting a service:

```sh
weather --generate-completion bash
weather --generate-completion powershell
```

This version has JSON and JSON Lines output. It does not implement table output,
JMESPath queries, shorthand object syntax, or interactive prompting.

## Qualification

```sh
python tools/test-cli.py
```

Qualification requires the Rust Clippy component. The harness builds and lints
a generated package as an independent consumer, launches its executable,
checks requests against a deterministic HTTP fixture, and calls the
real Weather service. It covers input sources, presence, configuration precedence,
authentication overrides, retry tokens, pagination, errors, and binary payloads.
CI runs it on Linux and Windows. These checks do not prove authenticated access
to deployed Leyline or Pigeon services.

## Local MCP server

Every generated executable includes an `mcp` subcommand for local agents:

```sh
weather --endpoint-url http://127.0.0.1:8080 mcp
claude mcp add weather -- weather --config profiles.json --profile agent mcp
```

It uses the same endpoint, environment variables, named profiles and bearer
credentials as operation commands. Wrappers can customize a parsed MCP
invocation's `config` and pass it to `dispatch`, including token providers.
Calls use the generated SDK and its operation authentication and retry rules.

Stdio carries one JSON-RPC message per line and stdout contains only protocol
responses. Initialize the session, send `notifications/initialized`, then use
`tools/list` and `tools/call`; `ping` is available throughout. Closing stdin
ends the server. Frames are limited to 1 MiB. Batch requests and discovery
pagination are unsupported; operation pagination is explicit through cursors.
Tools use operation names such as `GetCity`, while ordinary CLI commands
use kebab-case. Tool errors return `isError: true` with a generic message;
validation and SDK errors are never echoed because they may contain sensitive
values. Successful tool results carry the modeled output in `structuredContent`
and JSON text content. Protocol results intentionally retain requested sensitive
output members; the stdio server does not log request or response values.

The CLI reuses the HTTP MCP descriptor emitter, JSON schemas and validating
registry. It respects the selected client audience (`External` by default,
`--audience internal` when generating for trusted internal agents). No server
`.mcp()` option is required: the CLI calls the ordinary service HTTP API.
The command name `mcp` is reserved and collisions are collected generation diagnostics.

`python tools/test-cli.py` also verifies stdio initialization, discovery,
notifications, malformed input, oversized frames, schema rejection, profiles,
authentication overrides, modeled errors and a real Weather tool call in CI.
