<?php
declare(strict_types=1);
// Run against a package generated from crates/crucible-smithy/tests/fixtures/client-docs.json
// with namespace VaultSdk.
spl_autoload_register(function (string $class): void {
    $path = getenv('PHP_VAULT_PACKAGE') . '/src/' . str_replace('\\', '/', substr($class, strlen('VaultSdk\\'))) . '.php';
    require $path;
});
use VaultSdk\{AccessDenied, Credential, CredentialPassword, CredentialPin, CredentialUnknownVariant, CredentialUsername, Kind, Level, Secret};
use VaultSdk\Runtime\{ErrorMetadata, JsonValue, Timestamp};

function check(bool $value, string $message): void { if (!$value) throw new RuntimeException($message); }
const MARKER = '*** Sensitive Data Redacted ***';
$secrets = ['hunter2', '4321', 'old-one', 'old-two', 'map-secret', 'union-secret'];

$secret = new Secret(
    name: 'db', password: 'hunter2', pin: '4321', history: ['old-one', 'old-two'],
    byName: ['k' => 'map-secret'], aliases: ['alias-a', 'alias-b'], labels: ['env' => 'prod'],
    kind: new Kind(Kind::LOGIN), level: new Level(Level::HIGH), credential: new CredentialPassword('union-secret'),
    createdAt: new Timestamp(0), payload: "\x01\x02\x03",
);

$text = (string)$secret;
check(str_starts_with($text, 'Secret(name=db, '), 'type and first member: ' . $text);
check(str_contains($text, 'aliases=[alias-a, alias-b]'), 'lists recurse: ' . $text);
check(str_contains($text, 'labels={env=prod}'), 'maps recurse: ' . $text);
check(str_contains($text, 'kind=Kind(login)'), 'enums: ' . $text);
check(str_contains($text, 'level=Level(2)'), 'int enums: ' . $text);
check(str_contains($text, 'createdAt=1970-01-01T00:00:00Z'), 'timestamps: ' . $text);
check(str_contains($text, 'payload=<3 bytes>'), 'blobs show their length: ' . $text);
check(!str_contains($text, 'revision'), 'absent members are omitted: ' . $text);

// Every inspection path (var_dump, print_r, string forms) hides sensitive values, not just __toString().
// var_export() ignores __debugInfo() and prints public properties, so it is not an inspection path a model can guard.
ob_start(); var_dump($secret); print_r($secret);
$dumped = ob_get_clean();
foreach ([$text, $secret->describe(), $dumped] as $output) {
    foreach ($secrets as $leaked) check(!str_contains($output, $leaked), "$leaked leaked in: $output");
    foreach (['password=', 'pin=', 'history=', 'byName='] as $member) {
        check(str_contains($output, $member . MARKER) || str_contains($output, MARKER), "$member redacted");
    }
}
check(str_contains($text, 'credential=Credential(password=' . MARKER . ')'), 'union member: ' . $text);

check((string)new CredentialPin('9999') === 'Credential(pin=' . MARKER . ')', 'sensitive union member');
check((string)new CredentialUsername('bob') === 'Credential(username=bob)', 'plain union member');
check((string)new CredentialUnknownVariant('future', JsonValue::parse('"token-123"')) === 'Credential(unknownVariant=future)', 'unknown variant omits its value');
check((string)new Secret(name: 'x') === 'Secret(name=x)', 'minimal model');

$error = new AccessDenied(message: 'denied', reason: 'secret-reason',
    metadata: new ErrorMetadata('AccessDenied', 'denied', 403, 'r-1', ['authorization' => 'x']));
$described = $error->describe();
check(str_contains($described, 'reason=' . MARKER) && !str_contains($described, 'secret-reason'), 'error members redact: ' . $described);
check(str_contains($described, 'metadata=ErrorMetadata(name=AccessDenied, status=403, requestId=r-1)'), 'error metadata: ' . $described);
check(!str_contains($described, 'authorization'), 'headers are omitted');
ob_start(); var_dump($error); $dumped = ob_get_clean();
check(!str_contains($dumped, 'secret-reason'), 'var_dump of an error redacts');
echo "PHP string representations passed\n";
