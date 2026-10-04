# PHP clients

`crucible generate php` emits a Composer package with typed models,
synchronous operations, and lazy `Generator` pagination. It requires 64-bit
PHP 8.3 or later within PHP 8.x, with the cURL and JSON extensions. Generated
packages have no third-party PHP dependencies.

```sh
cargo run -p crucible-cli -- generate php model.json \
  --out generated/client --package-name example/weather-client \
  --namespace 'Example\Weather' --package-version 0.1.0 \
  --endpoint https://weather.example --content-sha256
cd generated/client
composer install
```

The package uses PSR-4 autoloading, with one model or runtime class per file.
Generation writes package identity and provenance; it does not publish.
Supply your license and release metadata before distribution. Use a Composer
path repository during development or archive the package for distribution.

```php
<?php
require 'vendor/autoload.php';

use Example\Weather\Client;
use Example\Weather\Models\CreateCityInput;
use Example\Weather\Models\ListCitiesInput;
use Example\Weather\Runtime\ClientConfig;
use Example\Weather\Runtime\Field;

$client = new Client(new ClientConfig(
    endpoint: 'https://weather.example',
    token: fn() => getenv('WEATHER_TOKEN'),
));
try {
    $client->createCity(new CreateCityInput(name: Field::value('London')));
    foreach ($client->listCitiesPages(new ListCitiesInput()) as $page) {
        foreach ($page->cities as $city) echo $city->name, "\n";
    }
} finally {
    $client->close();
}
```

Model properties are readonly. Required members are constructor arguments;
optional members use `Field`, whose default constructor means absent.
`Field::nullValue()` means explicit null and `Field::value($value)` means
present. Missing response members receive modeled defaults; explicit null
does not receive a default. Lists and maps use PHP arrays, with element types
recorded in PHPDoc. PHP canonicalizes integer-looking map keys to integers;
serialization preserves their string wire spelling.

Smithy `long` uses native 64-bit `int`. JSON integer decoding retains numeric
lexemes and checks bounds before conversion. `Timestamp` stores whole seconds
and nanoseconds, including negative fractional epochs. Date-time and HTTP-date
text support years 1–9999; conversion to `DateTimeImmutable` truncates to
microseconds. `JsonValue` retains exact JSON numbers and object/list identity,
including empty objects and object keys containing null bytes. JSON nesting
is bounded to 255 levels.

Open enums wrap wire values in a class. Unions have a class per variant and an
unknown variant carrying its name and `JsonValue`. Blobs are binary strings,
encoded as padded base64 in JSON. Raw blob payloads use bytes directly. An
empty response payload remains present; an empty request payload sends no body
or content type. Optional SHA-256 covers the final bytes after middleware.

Each call snapshots its input before hooks. Retries reuse that snapshot and
its generated idempotency token; token providers and middleware run on every
attempt. One deadline covers the call and retry delays. A `CancellationToken`
can take a polling closure to observe external cancellation during transport
and retry waits. Synchronous hooks cannot be interrupted: they must cooperate,
and the runtime checks the deadline and cancellation before and after each
hook. Pagination snapshots input at creation, starts requests on iteration,
detects repeated tokens, and uses a fresh deadline per page.

The default cURL transport reuses a connection pool, refuses redirects, and
has no response cache or cookie jar. It checks transfer completion so truncated
responses are transport failures. Successful responses without a modeled body
are discarded without waiting for that body. Proxy CONNECT responses never
count as API responses. Credentials require HTTPS except on loopback.

`close()` closes the owned transport and prevents new calls. An injected
`HttpTransport` remains caller-owned and must honor `CallScope` and `readBody`.
Calls on one transport are synchronous and cannot be reentrant. Modeled errors
extend `ServiceError`, which retains status, name, request ID, headers, and
retry traits even if modeled members are malformed. Retries follow the
[shared contract](clients.md).

Generation diagnoses reserved names, case-insensitive naming collisions,
and recursive collection aliases. Nominal recursive structures are supported.
`python tools/test-php.py` checks exact codecs, shared retry/header fixtures,
native weather-service calls, body interruption and proxy behavior, Composer
archive installation, and empty and renamed packages. Set `PHP` to choose the
executable and `COMPOSER_PHAR` to use a specific Composer archive. These checks
do not establish published-package or authenticated deployed-Leyline compatibility.
