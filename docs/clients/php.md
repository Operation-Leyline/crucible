# PHP clients

[All clients](../clients.md) · [Repository overview](../../README.md)

`crucible generate php` emits a Composer package with typed models,
synchronous operations, and lazy `Generator` pagination. It requires 64-bit
PHP 8.3 or later within PHP 8.x, with the cURL and JSON extensions. Generated
packages have no third-party PHP dependencies.

## Generating and installing

Use your service's `smithy/` source directory, a `.smithy` file, or a prepared
JSON AST as described in [preparing a model](../clients.md#preparing-a-model).

```sh
crucible generate php smithy \
  --out generated/client --package-name example/weather-client \
  --namespace 'Example\Weather' --package-version 0.1.0 \
  --endpoint https://weather.example
cd generated/client
composer install
```

The package uses PSR-4 autoloading, with one model or runtime class per file.
Generation writes package identity and provenance; it does not publish.
Supply your license and release metadata before distribution. Use a Composer
path repository during development or archive the package for distribution.

The public client name comes from the Smithy service: `WeatherClient` for
`Weather`, or `LeylineClient` for `Leyline`. Regenerate and replace earlier
`Client` imports and construction sites with the service-specific name.

## Calling operations

```php
<?php
require 'vendor/autoload.php';

use Example\Weather\WeatherClient;
use Example\Weather\CreateCityInput;
use Example\Weather\ListCitiesInput;
use Example\Weather\Runtime\ClientConfig;

$client = new WeatherClient(new ClientConfig(
    endpoint: 'https://weather.example',
    token: fn() => getenv('WEATHER_TOKEN'),
));
try {
    $client->createCity(new CreateCityInput(name: 'London'));
    foreach ($client->listCitiesPages(new ListCitiesInput()) as $page) {
        foreach ($page->cities as $city) echo $city->name, "\n";
    }
} finally {
    $client->close();
}
```

## Models and precision

Models live directly in the configured namespace, beside `WeatherClient`. Properties
are readonly and contain their native value. Pass ordinary named arguments:
`new CreateCityInput(name: 'London', location: null)`, then read `$input->name`.
Required members are constructor arguments. Optional members default to
`Runtime\Absent::Value`; explicit `null` remains null. Check
`$value === Absent::Value` when absence matters, and `$value === null` for an
explicit null. Import the sentinel with `use Example\Weather\Runtime\Absent;`.
No wrapper is needed around assigned values.

This changes earlier generated packages: remove `Models\` from model imports,
replace `Field::value($x)` with `$x`, `Field::nullValue()` with `null`, and
`new Field()` with `Absent::Value`. Read `$response->member` directly instead
of `$response->member->value`; replace `!$field->isSet` with
`$value === Absent::Value`. Regenerate the entire package and migrate its
consumers together; keeping the previous generated package restores the old API.

Missing response members receive modeled defaults; explicit null
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
or content type.

## API documentation and string representations

Every generated type, constructor argument, enum constant, operation and the
client carry PHPDoc built from the model's `@documentation` plus Crucible's own
text for the client, `ClientConfig`, transports, credentials and errors. `*/`
in model text cannot end a docblock, and `@tag` lines are escaped.

Models implement `__toString()` and print as
`Secret(name=db, password=*** Sensitive Data Redacted ***)`; modeled errors keep
PHP's exception text and expose the same format through `describe()`. Unset
members are omitted, nested models and collections recurse, a blob shows its
length, and `@sensitive` members (or members of a `@sensitive` shape or
collection of them) print the redaction marker. `__debugInfo()` returns the
same redacted members, so `var_dump()` and `print_r()` never reveal a
sensitive value either.

Structures and unions have no value equality besides PHP's `==` over public
properties; generated enums are plain classes compared by `value`.

## Transport and errors

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
[shared contract](../clients.md).

Generation diagnoses reserved names, case-insensitive naming collisions,
and recursive collection aliases. Nominal recursive structures are supported.

## Authentication

Operations that use HTTP bearer authentication send `Authorization: Bearer`
with the token from `ClientConfig::$token`, called on every attempt so a
provider can refresh it. A provider that returns null sends the request without
credentials. A caller's own `Authorization` header is kept, and credentials
require HTTPS except on loopback.

## Permissions

A model that declares typed permissions ([permissions](../permissions.md))
gets a `Permissions` namespace under the configured one, in
`src/Permissions/`; without permissions there is none. Its own namespace keeps
a model named `GraphPermission` from colliding. Each resource gets an open
class, `<Resource>Permission`, shaped like the model's string enums: a
readonly class wrapping `$value`, with one string constant per permission, in
declaration order, whose value is what grants store
(`GraphPermission::READ === 'graph:read'`), documented with the model's docs.
Constants are the permission's name in upper snake case, as PSR-12 writes
class constants (`mailSend` is `MAIL_SEND`). A deprecated alias is a constant
too, documented "An alias of `graph:read`." with `@deprecated`.

The classes are open: `new GraphPermission('graph:export')` keeps any string
verbatim, so a grant written by a newer server, or naming a permission
internal to the service, survives a round trip through `$value`.
`canonical()` maps an alias to its primary
(`(new GraphPermission(GraphPermission::VIEW))->canonical()->value === GraphPermission::READ`)
and returns any other value, unknown ones included, unchanged.

`Permissions\PERMISSIONS` is the registry, a namespace constant: a list of
`Runtime\ResourcePermissions` (`resourceType`, `permissions`), one per
resource type sorted by authorization name, each holding
`Runtime\PermissionDescription` entries (`value`, `description`, `tier`,
`grantable`, `deprecated`, `aliasOf`) sorted by value. `tier` is a plain string
(`'public'`, `'permissionOnly'` or `'notGrantable'`), since a newer server may
add tiers; `aliasOf` is null except on an alias. Both record types are
readonly runtime classes, shipped only with a registry. PSR-4 cannot autoload
a constant, so `composer.json` lists `src/Permissions/registry.php` under
`autoload.files`: Composer's autoloader defines it; any other loader must
`require` that file once. A grant picker offers the grantable, current
permissions:

```php
use const Example\Weather\Permissions\PERMISSIONS;

$offered = [];
foreach (PERMISSIONS as $resource) {
    foreach ($resource->permissions as $permission) {
        if ($permission->grantable && !$permission->deprecated) $offered[] = $permission;
    }
}
```

Internal permissions never appear: not as constants, not in `PERMISSIONS`, not
in any documentation. Two permissions of one resource that PHP names alike
(`read` and `READ`), or two resources whose class names differ only in case,
are diagnosed.

## Qualification

`python tools/test-php.py` checks exact codecs, shared retry/header fixtures,
native weather-service calls, body interruption and proxy behavior, Composer
archive installation, and empty and renamed packages. The typed permissions of
`tests/fixtures/permissions` are read through a Composer install
(`tests/permissions.php`).

Set `PHP` to
choose the executable and `COMPOSER_PHAR` to use a specific Composer archive. These checks
do not establish published-package or authenticated deployed-Leyline compatibility.
