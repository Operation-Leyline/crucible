# Ruby clients

[All clients](../clients.md) · [Repository overview](../../README.md)

`crucible generate ruby` emits native Ruby models, synchronous operations,
lazy `Enumerator` pagination, and a gemspec. Generated gems require Ruby 3.3
or later, below Ruby 5. They declare the Ruby standard-library gems they use,
including JSON and Net::HTTP.

## Generating and installing

Use your service's `smithy/` source directory, a `.smithy` file, or a prepared
JSON AST as described in [preparing a model](../clients.md#preparing-a-model).

```sh
crucible generate ruby smithy \
  --out generated/client --package-name weather_client --module-name Weather \
  --package-version 0.1.0 --endpoint https://weather.example
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
no body or content type.

## API documentation and string representations

Every generated type, member, enum value, operation and the client carry YARD
comments built from the model's `@documentation` plus Crucible's own text for
the client, `ClientConfig`, transports and errors. The gem ships a
`.yardopts` that selects Markdown, which is what Smithy documentation is
written in; `yard doc` renders them.

Models define `inspect` (and `to_s`, except on exceptions, whose `to_s` stays
the message) and print as `Secret(name=db, password=*** Sensitive Data Redacted ***)`.
Unset members are omitted, nested models and collections recurse, a blob shows
its length, and `@sensitive` members (or members of a `@sensitive` shape or
collection of them) print the redaction marker. A modeled error also shows its
transport metadata, without headers or message.

Generated enums define `==`, `eql?` and `hash`; structures and unions do not.

## Transport and errors

Token providers and middleware run on every attempt. Generated idempotency
tokens and input bytes remain stable across retries.
`ClientConfig.new(idempotency_token: -> { ... })` can override the
default random UUID supplier. It runs once for an omitted modeled token and
preserves explicit tokens. A single deadline covers
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

## Permissions

A model that declares typed permissions ([permissions](../permissions.md))
gets `Weather::Permissions` in `lib/<package>/permissions.rb`; without
permissions there is no such module. Its own module keeps a model named
`GraphPermission` or `PERMISSIONS` from colliding. Each resource gets an open
class, `<Resource>Permission`, shaped like the model's string enums: one
constant per permission, in declaration order, whose `value` is what grants
store (`Permissions::GraphPermission::READ.value == 'graph:read'`), with YARD
documentation from the model. Constants are the permission's name in upper
snake case (`mailSend` is `MAIL_SEND`). A deprecated alias is a constant too,
documented "An alias of `graph:read`." with `@deprecated`.

The classes are open: `GraphPermission.new('graph:export')` keeps any string
verbatim, so a grant written by a newer server, or naming a permission
internal to the service, survives a round trip. Values compare and hash by
`value`. `canonical` maps an alias to its primary
(`GraphPermission::VIEW.canonical == GraphPermission::READ`) and returns any
other value, unknown ones included, unchanged.

`Permissions::PERMISSIONS` is the registry: a frozen array of
`Runtime::ResourcePermissions` (`resource_type`, `permissions`), one per
resource type sorted by authorization name, each holding
`Runtime::PermissionDescription` entries (`value`, `description`, `tier`,
`grantable`, `deprecated`, `alias_of`) sorted by value. `tier` is a plain
string (`'public'`, `'permissionOnly'` or `'notGrantable'`), since a newer
server may add tiers; `alias_of` is nil except on an alias. Both are `Data`
classes from the bundled runtime, shipped only with a registry. A grant picker
offers the grantable, current permissions:

```ruby
offered = Weather::Permissions::PERMISSIONS.flat_map do |resource|
  resource.permissions.select { |p| p.grantable && !p.deprecated }
end
```

Internal permissions never appear: not as constants, not in `PERMISSIONS`, not
in any documentation. Two permissions of one resource that Ruby names alike
(`read` and `READ`), or a model named `Permissions`, are diagnosed.

## Qualification

Generation diagnoses invalid package/module names, core-class module
collisions, and model/member/operation name collisions. Recursive models and
collections are supported through runtime descriptions.
`python tools/test-ruby.py` checks shared fixtures, native weather-service
HTTP, interruptions and proxy handling, gem installation into a separate
consumer, and empty and renamed models. It also runs a native suite for the
typed permissions of `tests/fixtures/permissions` (`tests/permissions.rb`).
Set `RUBY` to select the executable.
These checks do not establish published-gem or authenticated deployed-Leyline
compatibility.
