# Ruby clients

`crucible generate ruby` emits native Ruby models, synchronous operations,
lazy `Enumerator` pagination, and a gemspec. Generated gems require Ruby 3.3
or later, below Ruby 5. They declare the Ruby standard-library gems they use,
including JSON and Net::HTTP.

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

```ruby
require 'weather_client'

client = Weather::Client.new(Weather::Runtime::ClientConfig.new(
  endpoint: 'https://weather.example', token: -> { ENV.fetch('WEATHER_TOKEN') }
))
begin
  client.create_city(Weather::Models::CreateCityInput.new(name: 'London'))
  client.list_cities_pages(Weather::Models::ListCitiesInput.new).each do |page|
    page.cities.each { |city| puts city.name }
  end
ensure
  client.close
end
```

Each model has keyword constructor arguments and readers in `Models`.
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
[shared contract](clients.md). The transport refuses redirects, uses no response
cache or cookie jar, and requests identity encoding. Truncated bodies are
transport failures. Successful responses without modeled bodies are discarded
without draining them. Credentials require HTTPS except on loopback.

`close` closes the owned transport and prevents new calls. An injected
transport remains caller-owned and implements
`send_request(request, read_body:, scope:)` and `close`. It must honor the
scope's deadline/cancellation and the body-read decision. Modeled errors
inherit `ServiceError`; metadata retains wire status, error name, request ID,
headers, and retry traits even when modeled members cannot be decoded.

Generation diagnoses invalid package/module names, core-class module
collisions, and model/member/operation name collisions. Recursive models and
collections are supported through runtime descriptions.
`python tools/test-ruby.py` checks shared fixtures, native weather-service
HTTP, interruptions and proxy handling, gem installation into a separate
consumer, and empty and renamed models. Set `RUBY` to select the executable.
These checks do not establish published-gem or authenticated deployed-Leyline
compatibility.
