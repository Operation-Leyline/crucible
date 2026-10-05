# Ruby clients

[All clients](../clients.md) · [Repository overview](../../README.md)

`crucible generate ruby` emits native Ruby models, synchronous operations,
lazy `Enumerator` pagination, and a gemspec. Generated gems require Ruby 3.3
or later, below Ruby 5. They declare the Ruby standard-library gems they use,
including JSON and Net::HTTP.

## Generating and installing

```sh
cargo run -p crucible-cli -- generate ruby model.json \
  --out generated/client --package-name weather_client --module-name Weather \
  --package-version 0.1.0 --endpoint https://weather.example --content-sha256
cd generated/client
gem build weather_client.gemspec
gem install ./weather_client-0.1.0.gem
```

Generation does not publish the gem. The package owner supplies author,
license, and repository metadata before publishing. `crucible.json` records
the generator version, service version, package version, and schema digest.

The public client name comes from the Smithy service: `WeatherClient` for
`Weather`, or `LeylineClient` for `Leyline`. Regenerate and replace earlier
`Client` imports and construction sites with the service-specific name.

## Calling operations

```ruby
require 'weather_client'

client = Weather::WeatherClient.new(Weather::Runtime::ClientConfig.new(
  endpoint: 'https://weather.example', token: -> { ENV.fetch('WEATHER_TOKEN') }
))
begin
  client.create_city(Weather::CreateCityInput.new(name: 'London'))
  client.list_cities_pages(Weather::ListCitiesInput.new).each do |page|
    page.cities.each { |city| puts city.name }
  end
ensure
  client.close
end
```

## Models and precision

Each model has keyword constructor arguments and readers directly under the
package module, for example `Weather::CreateCityInput.new(name: "London")`.
Read native values with `input.name` or `page.cities.each`. To migrate an
earlier package, replace `Weather::Models::Type` with `Weather::Type`;
constructor arguments and member values keep their existing semantics.
Regenerate the package and its consumers together; the previous generated gem
retains the old namespace if a rollback is needed.
Ordinary model instances are frozen; their arrays and strings remain native
Ruby values. A call snapshots its input before invoking hooks. Optional
members default to `Runtime::UNSET`; `nil` means explicit null. Members that
the IR makes non-optional on clients are required constructor arguments.
Smithy input structures and `@clientOptional` members allow omission even
when marked `@required`: missing HTTP labels are rejected before transport,
while missing body/query/header members are left for service validation.
Missing response members receive modeled
defaults, while explicit null does not. Names use snake_case, with a
`_value` suffix where a member would hide a keyword or runtime method.

Smithy integers use Ruby `Integer`, with wire bounds enforced for byte,
short, integer, and signed 64-bit long. Floating-point values must be finite.
The JSON codec preserves exact decimal values before numeric conversion.
`Timestamp` stores whole seconds and nanoseconds and converts to native `Time`
without losing nanoseconds. Date-time and HTTP-date text use the Gregorian
calendar and years 1–9999. Epoch values support signed 64-bit seconds.
`JsonValue` owns an exact JSON snapshot, preserving unknown numeric values
and object/list identity. JSON and modeled nesting are bounded to 255 levels.

String enums wrap their wire value and preserve unknown values. Unions have
nested variant classes, such as `Location::CoordinatesVariant`, plus
`Location::Unknown`, which takes a wire name and `JsonValue`. Lists and maps
use `Array` and `Hash`; enum map keys use enum instances. Blobs are binary
strings, encoded as padded base64 in JSON or sent directly for raw payloads.
An empty response payload remains present; an empty request payload sends
no body or content type. Optional SHA-256 covers bytes after middleware.

## Transport and errors

Token providers and middleware run on every attempt. Generated idempotency
tokens and input bytes remain stable across retries. A single deadline covers
the call and retry waits. `CancellationToken` supports both `cancel` and an
optional polling block. A retry delay extending past the call deadline ends
with `DeadlineError`; cancellation can interrupt the wait.
Cancellation is permanent once observed. Network I/O
runs in an owned worker so a stalled connection can be interrupted; synchronous
caller hooks must cooperate, with deadline checks before and after each hook.
Pagination snapshots input when created, fetches on iteration, detects token
cycles, and gives each page a fresh deadline.

The default Net::HTTP transport reuses a connection to the current origin and
serializes access to it. Waiting calls honor cancellation and deadlines.
Net::HTTP's internal retry is disabled; retries follow the
[shared contract](../clients.md). The transport refuses redirects, uses no response
cache or cookie jar, and requests identity encoding. Truncated bodies are
transport failures. Successful responses without modeled bodies are discarded
without draining them. Credentials require HTTPS except on loopback.

`close` closes the owned transport and prevents new calls. An injected
transport remains caller-owned and implements
`send_request(request, read_body:, scope:)` and `close`. It must honor the
scope's deadline/cancellation and the body-read decision. Modeled errors
inherit `ServiceError`; metadata retains wire status, error name, request ID,
headers, and retry traits even when modeled members cannot be decoded.

## Protocols

Besides Crucible JSON, generated clients speak AWS `restJson1`, `awsJson1_0`
and `awsJson1_1`, chosen at generation time with
`--protocol aws.protocols#...`. The RPC variants POST an object (`{}` when the
input is empty) to `/` with `X-Amz-Target: Service.Operation` and
`application/x-amz-json-1.x`. `restJson1` applies the binding timestamp
formats: HTTP dates in headers (lists of them are unquoted) and date-time in
query strings and labels. Under the AWS protocols a structure member's `null`
is treated as absent (documents keep `null`), non-finite floats travel as
`"NaN"`/`"Infinity"`/`"-Infinity"`, a union's `__type` is ignored, and errors
are named by `X-Amzn-ErrorType`, `code`, or `__type` with any namespace or `:`
suffix removed. `X-Amzn-RequestId` becomes `metadata.request_id`.

`aws.protocols#restXml`, `aws.protocols#awsQuery`, and `aws.protocols#ec2Query`
are generated the same way. XML is read with REXML (the gem declares `rexml`);
the runtime refuses any document type declaration and any undeclared entity
reference before using a value, so no entity is ever expanded.
Element and query names, flattening, attributes, and namespaces come from the
lowered IR (`@xmlName`, `@xmlFlattened`, `@xmlAttribute`, `@xmlNamespace`,
`aws.protocols#ec2QueryName`). restXml applies the HTTP bindings and writes the
remaining members under the input's root element, in the input's namespace or
else the service's; a structure `@httpPayload` is named by the member's
`@xmlName`, else the target's name. Lists wrap `<member>` (or the list member's
`@xmlName`) elements, maps wrap `<entry><key>...</key><value>...</value></entry>`,
and flattened collections repeat the member's own element. awsQuery and ec2Query
POST `application/x-www-form-urlencoded` forms that start with `Action` and the
service `Version`: awsQuery lists are `Name.member.N` (`Name.N` when flattened, a
bare `Name=` when empty) and maps `Name.entry.N.key`/`.value`; ec2Query always
flattens, leaves empty lists out, and names members by `ec2QueryName` or the
capitalized XML name. Timestamps default to date-time; non-finite floats are
`NaN`, `Infinity`, and `-Infinity`; blobs are base64.

Responses are read by local element name, so namespace prefixes do not matter.
awsQuery reads the `<OperationResult>` element and ignores `<ResponseMetadata>`;
ec2Query reads the root element. Errors are found in `<ErrorResponse><Error>`,
restXml's unwrapped `<Error>` (`noErrorWrapping`, as S3 sends), or ec2Query's
`<Response><Errors><Error>`: `Code` selects the modeled error (an
`aws.protocols#awsQueryError` code under awsQuery), `Message` becomes the
error message, and `RequestId`/`RequestID` is the request ID unless an
`x-amzn-requestid` or `x-amz-request-id` header supplies one. A success
response's request ID is not exposed. Nesting is limited to 256 elements, and
malformed XML fails the call.
A restXml body with text XML 1.0 cannot carry (most control characters) is
rejected before sending. Sparse collections are refused for these protocols at
generation time, because XML has no null element; document shapes and raw restXml error payloads are refused as
well, and `awsQueryCompatible` is not supported.
Errors carry the envelope's code as `metadata.name` and its message as
`metadata.message`; a modeled `Message` member is `message_value`.

## Waiters

Each modeled waiter becomes `wait_until_<name>(input, timeout:, min_delay:
nil, max_delay: nil, cancellation: nil)`, returning the output that satisfied
a success acceptor (`nil` when an error-type acceptor succeeds):

```ruby
job = client.wait_until_ready(Jobs::Input.new(names: ['a']), timeout: 300)
```

Acceptors run the compiled JMESPath from the IR over member names, with
exact rational numbers and epoch-second timestamps. Delays follow Smithy's
jittered exponential backoff between the modeled minimum and maximum; the last
delay is clipped to the deadline, and each attempt gets the smaller of the
client timeout and the time left. A failure acceptor or an unmatched error
raises `Runtime::WaiterError` (`attempts`, `response`, `error`, and the
acceptor message as `reason`); the deadline raises `WaiterTimeoutError`.
`CancellationToken` interrupts both requests and delays.

## Endpoints and checksums

When the model has endpoint rules, the client resolves every call from them,
so `endpoint` becomes optional. Parameters come from the IR's sources in
order: input members and paths, `ClientConfig.new(endpoint_parameters: {})`,
built-ins (`AWS::Region` from `region`, `SDK::Endpoint` from `endpoint`,
overridable with `endpoint_builtins:`), and defaults. Resolved headers are
sent, and an `authSchemes` property narrows the modeled schemes and supplies
the SigV4 signing name and region. Error rules raise `Runtime::EndpointError`.
`@endpoint` host prefixes validate each label before dispatch;
`disable_host_prefix: true` turns them off.

`@httpChecksumRequired` sends `Content-MD5`; `aws.protocols#httpChecksum`
supports CRC32, CRC32C, CRC64NVME (pure Ruby), MD5, SHA1, SHA256 and SHA512.
With the default `request_checksum_calculation: 'when_supported'` an unset
algorithm member becomes CRC32 and an unset validation mode `ENABLED`, in a
copy of the input. Checksums cover the bytes after middleware, and a
caller-supplied checksum header is never replaced. Successful responses with
a modeled checksum header are verified (`Runtime::ChecksumError` on
mismatch) and reported to `checksum_validated: ->(operation, algorithm)`;
`'when_required'` opts out of either default.

## Streaming blobs

A `@streaming` blob input accepts a binary `String`, any IO responding to
`read`, or `Runtime::ByteStream.new(length: n) { fresh_io }`. Strings,
seekable IOs (replayed from their starting position) and factories are
retried like any other body; any other IO is one-shot and gets exactly one
attempt. A known length is sent as `Content-Length`, otherwise the body is
chunked; `@requiresLength` and `@length` bounds are checked before dispatch
and while the bytes flow. Checksums and `--content-sha256` read a replayable
stream once in chunks before sending it; for a one-shot stream, supply the
header yourself or opt out. SigV4 signs streams as `UNSIGNED-PAYLOAD`.

A streamed response is never buffered when the call has a block or a target:

```ruby
client.download { |chunk| file.write(chunk) }          # body is an empty IO
client.download(response_target: File.open('out', 'wb')) # body is the target
```

Without either, the body is buffered into a `StringIO`. Chunks reach the
caller on the calling thread through a bounded queue, so a slow consumer
applies backpressure; the deadline and cancellation are checked between
chunks. Response checksums are computed incrementally and checked at the end.
A failure after bytes reached the caller is not retried. An injected
transport that serves streamed responses accepts `sink:` and calls it with
each chunk of a successful body, and reads a `Runtime::StreamBody` request
body with `read`.

## Qualification

Generation diagnoses invalid package/module names, core-class module
collisions, and model/member/operation name collisions. Recursive models and
collections are supported through runtime descriptions.
`python tools/test-ruby.py` checks shared fixtures, native weather-service
HTTP, interruptions and proxy handling, gem installation into a separate
consumer, and empty and renamed models. It also runs native suites for
waiters (every acceptor kind, the JMESPath functions, jitter, deadlines and
cancellation), endpoint rules and host prefixes, the published checksum
vectors and policies, the three AWS JSON protocols, the restXml, awsQuery and
ec2Query vectors (through `tests/protocol_adapter.rb` and
`tools/protocol_conformance.py`) and their consumer suite (fake and loopback
HTTP transports, error envelopes, hostile XML), and streaming blobs
(replay rules, length rules, cancellation, and 256 MiB each way over loopback
HTTP with bounded peak memory). Set `RUBY` to select the executable.
These checks do not establish published-gem or authenticated deployed-Leyline
compatibility.
