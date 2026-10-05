<?php
declare(strict_types=1);
// Published vectors for the CBOR codec (RFC 8949 Appendix A), HTTP Digest (RFC 7616 section 3.9),
// and SigV4a (the AWS CRT aws-signing-test-suite v4a cases), plus RPC v2 CBOR client behavior.
spl_autoload_register(static function(string $class): void {
    if (str_starts_with($class, 'VaultSdk\\')) require getenv('PHP_VAULT_PACKAGE') . '/src/' . str_replace('\\', '/', substr($class, 9)) . '.php';
});
use VaultSdk\{ChoiceBytes, ChoiceUnknownVariant, Denied, Record, SignInput, VaultClient};
use VaultSdk\Runtime\{Absent, Authentication, AwsCredentials, BasicCredentials, BigDecimal, BigInteger, CallScope, Cbor, CborBytes, CborTag, ClientConfig, DigestAuthentication, HttpRequest, HttpResponse, HttpTransport, JsonNumber, JsonObject, Timestamp, Unit, UnknownServiceError};
error_reporting(E_ALL);
set_error_handler(static function(int $severity, string $message, string $file, int $line): never { throw new ErrorException($message, 0, $severity, $file, $line); });
function check(bool $condition, string $message): void { if (!$condition) throw new RuntimeException($message); }
function rejects(string $class, callable $work, string $message): void
{
    try { $work(); } catch (Throwable $error) { if ($error instanceof $class) return; throw new RuntimeException("$message: " . $error::class . ' ' . $error->getMessage(), 0, $error); }
    throw new RuntimeException("$message: expected $class");
}
function same(mixed $left, mixed $right): bool {
    if (is_float($left) && is_float($right)) return is_nan($left) ? is_nan($right) : pack('E', $left) === pack('E', $right);
    if ($left instanceof JsonNumber && $right instanceof JsonNumber) return $left->text === $right->text;
    if ($left instanceof CborBytes && $right instanceof CborBytes) return $left->bytes === $right->bytes;
    if ($left instanceof CborTag && $right instanceof CborTag) return $left->tag === $right->tag && same($left->value, $right->value);
    if ($left instanceof JsonObject && $right instanceof JsonObject) {
        if (array_map('strval', array_keys($left->members)) !== array_map('strval', array_keys($right->members))) return false;
        foreach ($left->members as $key => $value) if (!same($value, $right->get((string)$key))) return false;
        return true;
    }
    if (is_array($left) && is_array($right)) {
        if (count($left) !== count($right)) return false;
        foreach ($left as $i => $value) if (!same($value, $right[$i])) return false;
        return true;
    }
    return $left === $right;
}

// RFC 8949 Appendix A: every example decodes; those whose preferred encoding is not a
// half-precision float also re-encode to the same bytes (Smithy avoids half floats).
$vectors = [
    ['00', 0, true], ['01', 1, true], ['0a', 10, true], ['17', 23, true], ['1818', 24, true], ['1819', 25, true],
    ['1864', 100, true], ['1903e8', 1000, true], ['1a000f4240', 1000000, true], ['1b000000e8d4a51000', 1000000000000, true],
    ['1bffffffffffffffff', new JsonNumber('18446744073709551615'), false],
    ['c249010000000000000000', new CborTag(2, new CborBytes(hex2bin('010000000000000000'))), true],
    ['3bffffffffffffffff', new JsonNumber('-18446744073709551616'), false],
    ['c349010000000000000000', new CborTag(3, new CborBytes(hex2bin('010000000000000000'))), true],
    ['20', -1, true], ['29', -10, true], ['3863', -100, true], ['3903e7', -1000, true],
    ['f90000', 0.0, false], ['f98000', -0.0, false], ['f93c00', 1.0, false], ['fb3ff199999999999a', 1.1, true],
    ['f93e00', 1.5, false], ['f97bff', 65504.0, false], ['fa47c35000', 100000.0, true], ['fa7f7fffff', 3.4028234663852886e+38, true],
    ['fb7e37e43c8800759c', 1.0e+300, true], ['f90001', 5.960464477539063e-8, false], ['f90400', 0.00006103515625, false],
    ['f9c400', -4.0, false], ['fbc010666666666666', -4.1, true], ['f97c00', INF, false], ['f97e00', NAN, false], ['f9fc00', -INF, false],
    ['fa7f800000', INF, true], ['fa7fc00000', NAN, true], ['faff800000', -INF, true],
    ['fb7ff0000000000000', INF, false], ['fb7ff8000000000000', NAN, false], ['fbfff0000000000000', -INF, false],
    ['f4', false, true], ['f5', true, true], ['f6', null, true], ['f7', null, false],
    ['c11a514b67b0', new CborTag(1, 1363896240), true], ['c1fb41d452d9ec200000', new CborTag(1, 1363896240.5), true],
    ['d74401020304', new CborTag(23, new CborBytes("\x01\x02\x03\x04")), true],
    ['40', new CborBytes(''), true], ['4401020304', new CborBytes("\x01\x02\x03\x04"), true],
    ['60', '', true], ['6161', 'a', true], ['6449455446', 'IETF', true], ['62225c', "\"\\", true], ['62c3bc', "\u{fc}", true],
    ['63e6b0b4', "\u{6c34}", true], ['64f0908591', "\u{10151}", true],
    ['80', [], true], ['83010203', [1, 2, 3], true], ['8301820203820405', [1, [2, 3], [4, 5]], true],
    ['98190102030405060708090a0b0c0d0e0f101112131415161718181819', range(1, 25), true],
    ['a0', new JsonObject(), true], ['a201020304', new JsonObject(['1' => 2, '3' => 4]), false],
    ['a26161016162820203', new JsonObject(['a' => 1, 'b' => [2, 3]]), true], ['826161a161626163', ['a', new JsonObject(['b' => 'c'])], true],
    ['a56161614161626142616361436164614461656145', new JsonObject(['a' => 'A', 'b' => 'B', 'c' => 'C', 'd' => 'D', 'e' => 'E']), true],
    ['5f42010243030405ff', new CborBytes("\x01\x02\x03\x04\x05"), false], ['7f657374726561646d696e67ff', 'streaming', false],
    ['9fff', [], false], ['9f018202039f0405ffff', [1, [2, 3], [4, 5]], false], ['9f01820203820405ff', [1, [2, 3], [4, 5]], false],
    ['83018202039f0405ff', [1, [2, 3], [4, 5]], false], ['83019f0203ff820405', [1, [2, 3], [4, 5]], false],
    ['9f0102030405060708090a0b0c0d0e0f101112131415161718181819ff', range(1, 25), false],
    ['bf61610161629f0203ffff', new JsonObject(['a' => 1, 'b' => [2, 3]]), false], ['826161bf61626163ff', ['a', new JsonObject(['b' => 'c'])], false],
    ['bf6346756ef563416d7421ff', new JsonObject(['Fun' => true, 'Amt' => -2]), false],
];
foreach ($vectors as [$hex, $value, $encodes]) {
    $decoded = Cbor::decode(hex2bin($hex));
    check(same($decoded, $value), "CBOR decode $hex");
    if ($encodes) check(bin2hex(Cbor::encode($value)) === $hex, "CBOR encode $hex: " . bin2hex(Cbor::encode($value)));
}
check(bin2hex(Cbor::encode(1.5)) === 'fa3fc00000' && bin2hex(Cbor::encode(-0.0)) === 'fa80000000', 'floats use single precision when exact, never half');
check(bin2hex(Cbor::encode(PHP_INT_MIN)) === '3b7fffffffffffffff' && Cbor::decode(hex2bin('3b7fffffffffffffff')) === PHP_INT_MIN, 'minimum 64-bit integer');
check(Cbor::toDecimal(Cbor::fromDecimal('340282366920938463463374607431768211456')) === '340282366920938463463374607431768211456', 'bignum digits');
foreach ([['', 'empty'], ['18', 'truncated argument'], ['1c', 'reserved information'], ['5f6161ff', 'text chunk in bytes'], ['7f4161ff', 'byte chunk in text'],
    ['62c328', 'invalid UTF-8'], ['a101', 'map value missing'], ['a1f601', 'null key'], ['a2616101616102', 'duplicate key'],
    ['0001', 'trailing bytes'], ['ff', 'lone break'], ['9f01', 'unterminated array'], ['81ff', 'break in definite array'], ['f818', 'simple value'],
    ['5b0000000100000000', 'oversized length'], ['1f', 'indefinite integer'], ['df', 'indefinite tag']] as [$hex, $why]) {
    rejects(UnexpectedValueException::class, static fn() => Cbor::decode(hex2bin($hex)), "malformed CBOR: $why");
}
rejects(UnexpectedValueException::class, static fn() => Cbor::decode(str_repeat("\x81", 300) . "\x00"), 'CBOR nesting limit');
rejects(UnexpectedValueException::class, static fn() => Cbor::encode("\xff"), 'CBOR text must be UTF-8');

// RFC 7616 section 3.9.1 (MD5 and SHA-256) and 3.9.2 (SHA-512-256 with userhash).
$challenge = ['realm' => 'http-auth@example.org', 'qop' => 'auth, auth-int', 'nonce' => '7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v', 'opaque' => 'FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS'];
$cnonce = 'f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ';
check(DigestAuthentication::header([...$challenge, 'algorithm' => 'MD5'], 'Mufasa', 'Circle of Life', 'GET', '/dir/index.html', '', '00000001', $cnonce)
    === 'Digest username="Mufasa", realm="http-auth@example.org", uri="/dir/index.html", algorithm=MD5, nonce="7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v", nc=00000001, cnonce="f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ", qop=auth, response="8ca523f5e9506fed4657c9700eebdbec", opaque="FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS"', 'RFC 7616 MD5 example');
check(str_contains(DigestAuthentication::header([...$challenge, 'algorithm' => 'SHA-256'], 'Mufasa', 'Circle of Life', 'GET', '/dir/index.html', '', '00000001', $cnonce),
    'response="753927fa0e85d155564e2e272a28d1802ca10daf4496794697cf8db5856cb6c1"'), 'RFC 7616 SHA-256 example');
$header = DigestAuthentication::header(['realm' => 'api@example.org', 'qop' => 'auth', 'algorithm' => 'SHA-512-256', 'nonce' => '5TsQWLVdgBdmrQ0XsxbDODV+57QdFR34I9HAbC/RVvkK',
    'opaque' => 'HRPCssKJSGjCrkzDg8OhwpzCiGPChXYjwrI2QmXDnsOS', 'charset' => 'UTF-8', 'userhash' => 'true'], "J\u{e4}s\u{f8}n Doe", 'Secret, or not?', 'GET', '/doe.json', '', '00000001', 'NTg6RKcb9boFIAS3KrFK9BGeh+iDa/sm6jUMp2wds69v');
// The printed 3.9.2 values truncate SHA-512 rather than use FIPS 180-4 SHA-512/256; RFC erratum
// 4897 (reported) gives these SHA-512/256 values, which PHP's sha512/256 reproduces.
check(str_contains($header, 'username="793263caabb707a56211940d90411ea4a575adeccb7e360aeb624ed06ece9b0b"') && str_contains($header, 'userhash=true')
    && str_contains($header, 'response="3798d4131c277846293534c3edc11bd8a5e4cdcbff78b05db9d95eeb1cec68a5"'), 'RFC 7616 SHA-512-256 userhash example (erratum 4897)');
$plain = DigestAuthentication::header(['realm' => 'r', 'nonce' => 'n', 'qop' => 'auth'], "J\u{e4}son", 'p', 'GET', '/', '', '00000001', 'c');
check(str_contains($plain, "username*=UTF-8''J%C3%A4son"), 'non-ASCII usernames use the extended parameter');
$selected = DigestAuthentication::select('Basic realm="x", Digest realm="r", qop="auth", algorithm=MD5, nonce="a", Digest realm="r", qop="auth-int", algorithm=SHA-256, nonce="b", Digest realm="r", algorithm=UNKNOWN, nonce="c"');
check($selected['nonce'] === 'b' && $selected['algorithm'] === 'SHA-256', 'strongest supported challenge');
check(DigestAuthentication::select('Digest realm="r", nonce="n", qop="auth-conf"') === null && DigestAuthentication::select('Bearer realm="x"') === null, 'unsupported challenges');
check(str_contains(DigestAuthentication::header(['realm' => 'r', 'nonce' => 'n', 'qop' => 'auth-int', 'algorithm' => 'MD5-sess'], 'u', 'p', 'POST', '/x', 'body', '00000002', 'c'),
    'response="' . md5(md5(md5('u:r:p') . ':n:c') . ':n:00000002:c:auth-int:' . md5('POST:/x:' . md5('body'))) . '"'), 'MD5-sess with auth-int');
rejects(UnexpectedValueException::class, static fn() => DigestAuthentication::header(['realm' => 'r', 'nonce' => 'n', 'qop' => 'auth-int'], 'u', 'p', 'POST', '/', null, '00000001', 'c'), 'auth-int needs a buffered body');

// SigV4a: the AWS CRT aws-signing-test-suite/v4a cases (sign_body=false). ECDSA signatures are
// randomized, so the canonical request and string to sign must match exactly, the derived key
// must equal the published public key, and both the published and our signatures must verify.
$credentials = new AwsCredentials('AKIDEXAMPLE', 'wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY');
$key = openssl_pkey_get_details(Authentication::sigV4aKey($credentials));
check(bin2hex($key['ec']['x']) === 'b6618f6a65740a99e650b33b6b4b5bd0d43b176d721a3edfea7e7d2d56d936b1'
    && bin2hex($key['ec']['y']) === '865ed22a7eadc9c5cb9d2cbaca1b3699139fedc5043dc6661864218330c8e518', 'SigV4a derived public key');
$now = new DateTimeImmutable('2015-08-30T12:36:00Z');
foreach ([
    ['GET', null, "host;x-amz-date;x-amz-region-set", 'cf59db423e841c8b7e3444158185aa261b724a5c27cbe762676f3eed19f4dc02',
        '3045022018b4e277d0281864beb51d3600e23f88510ea5031d68ddfbb68614b82a5eb7d2022100effb9c5f22ed9ef3ae0ab243d21f06bce82365bbb79529a07b6888c343ae5f8c'],
    ['POST', null, "host;x-amz-date;x-amz-region-set", '806a9b01b76472cc6b66fff02630726d55f8b4ada6d2fd9b36eb0d710e215861',
        '3044022051fe398025aafbfc21d054bc78e5edfb96c9acb7fd272795565181d757815e4702202e7b8d2b92324290b1d95f8b0fc5e333bb8b5e333f6160bcab39d7258156d224'],
    ['GET', '6e86291e8372ff2a2260956d9b8aae1d763fbf315fa00fa31553b73ebf194267', "host;x-amz-date;x-amz-region-set;x-amz-security-token", '79893373104239a0547df489af395ec3c1b8873a8601f07f11ffd3f1ac557e7d',
        '3044022035c6bc9a8cab8a434bd8bd1cc3b6f1401842f530f1537722ad10d72a1577f8c80220101352fe0720715efbab80f82edb133175f0b61cad85daf943ad615852f9ae10'],
] as [$method, $token, $signed, $hash, $published]) {
    $identity = new AwsCredentials('AKIDEXAMPLE', 'wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY', $token);
    [$request, $canonical, $text] = Authentication::signV4aParts(new HttpRequest('https://example.amazonaws.com/', $method), $identity, 'service', 'us-east-1', $now, false, false);
    $lines = "host:example.amazonaws.com\nx-amz-date:20150830T123600Z\nx-amz-region-set:us-east-1\n" . ($token === null ? '' : "x-amz-security-token:$token\n");
    check($canonical === "$method\n/\n\n$lines\n$signed\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855", "SigV4a $method canonical request");
    check($text === "AWS4-ECDSA-P256-SHA256\n20150830T123600Z\n20150830/service/aws4_request\n$hash", "SigV4a $method string to sign");
    check(openssl_verify($text, hex2bin($published), $key['key'], OPENSSL_ALGO_SHA256) === 1, "published SigV4a $method signature verifies with the derived key");
    check(preg_match('/^AWS4-ECDSA-P256-SHA256 Credential=AKIDEXAMPLE\/20150830\/service\/aws4_request, SignedHeaders=' . preg_quote($signed, '/') . ', Signature=([0-9a-f]+)$/D', $request->headers['authorization'], $m) === 1, "SigV4a $method authorization");
    check(openssl_verify($text, hex2bin($m[1]), $key['key'], OPENSSL_ALGO_SHA256) === 1 && openssl_verify($text . 'x', hex2bin($m[1]), $key['key'], OPENSSL_ALGO_SHA256) === 0, "SigV4a $method signature");
    check($request->headers['x-amz-region-set'] === 'us-east-1' && !isset($request->headers['x-amz-content-sha256']), "SigV4a $method headers");
}
rejects(InvalidArgumentException::class, static fn() => Authentication::signV4a(new HttpRequest('https://h/', 'GET'), $credentials, 'service', ''), 'SigV4a needs a region set');
rejects(InvalidArgumentException::class, static fn() => Authentication::signV4a(new HttpRequest('https://h/', 'GET'), new AwsCredentials('a', 'b', expiresAt: new DateTimeImmutable('-1 minute')), 's', 'r'), 'expired SigV4a credentials');
rejects(InvalidArgumentException::class, static fn() => new ClientConfig(sigv4aSigningRegionSet: ['us-east-1,us-west-2']), 'region set entries are single regions');

// RPC v2 CBOR through the generated client: wire shape, big numbers, errors, and malformed responses.
final class Fake implements HttpTransport {
    /** @var list<HttpRequest> */
    public array $requests = [];
    public function __construct(private Closure $respond) {}
    public function send(HttpRequest $request, bool $readBody, CallScope $scope): HttpResponse { $this->requests[] = $request; return ($this->respond)($request); }
    public function close(): void {}
}
$cbor = ['smithy-protocol' => 'rpc-v2-cbor', 'content-type' => 'application/cbor'];
$digestAuth = static fn() => new BasicCredentials('u', 'p');
$echo = new Fake(static fn(HttpRequest $request) => new HttpResponse(200, $cbor, $request->body));
$client = new VaultClient(new ClientConfig(endpoint: 'https://vault.example/prefix', transport: $echo, digestAuth: $digestAuth));
$record = new Record(key: 'k', data: "\x00\xff", when: new Timestamp(-2, 250000000), count: new BigInteger('-18446744073709551617'),
    ratio: new BigDecimal('-1.0000000000000000000000001'), small: -9, score: NAN, level: 1.5, tags: ['a' => 'b'], items: ["\x01", ''], choice: new ChoiceBytes("\x02"));
$output = $client->put($record);
$sent = $echo->requests[0];
check($sent->url === 'https://vault.example/prefix/service/Vault/operation/Put' && $sent->method === 'POST', 'RPC v2 URL keeps the endpoint prefix');
check($sent->headers['smithy-protocol'] === 'rpc-v2-cbor' && $sent->headers['content-type'] === 'application/cbor' && $sent->headers['accept'] === 'application/cbor' && !isset($sent->headers['x-amz-target']), 'RPC v2 request headers');
$wire = Cbor::decode($sent->body);
check(same($wire->get('when'), new CborTag(1, -1.75)) && same($wire->get('data'), new CborBytes("\x00\xff")), 'timestamps are tag 1 epochs and blobs byte strings');
check(same($wire->get('count'), new CborTag(3, new CborBytes(hex2bin('010000000000000000')))), 'bigInteger beyond 64 bits is a tag 3 bignum');
check(same($wire->get('ratio'), new CborTag(4, [-25, new CborTag(3, new CborBytes(Cbor::fromDecimal('10000000000000000000000000')))])), 'bigDecimal is a tag 4 decimal fraction');
check(is_nan($wire->get('score')) && same($wire->get('level'), 1.5) && same($wire->get('choice'), new JsonObject(['bytes' => new CborBytes("\x02")])), 'floats and unions');
check($output->when == $record->when && (string)$output->count === '-18446744073709551617' && $output->ratio->equals($record->ratio) && is_nan($output->score)
    && $output->data === "\x00\xff" && $output->items === ["\x01", ''] && $output->choice instanceof ChoiceBytes && $output->choice->value === "\x02" && $output->small === -9, 'CBOR round trip');
// An indefinite map with half floats, a null member, an unknown variant beside __type, a bignum, and a decimal fraction.
$handmade = "\xbf" . Cbor::encode('key') . Cbor::encode('k') . Cbor::encode('choice') . "\xa2" . Cbor::encode('__type') . Cbor::encode('x') . Cbor::encode('otherVariant') . "\x41\x02"
    . Cbor::encode('tags') . "\xf6" . Cbor::encode('score') . "\xf9\x7e\x00" . Cbor::encode('level') . "\xf9\x3e\x00"
    . Cbor::encode('count') . "\xc2\x49\x01" . str_repeat("\x00", 8) . Cbor::encode('ratio') . "\xc4\x82\x21\x19\x03\xe8" . "\xff";
$decoded = (new VaultClient(new ClientConfig(endpoint: 'https://vault.example', transport: new Fake(static fn() => new HttpResponse(200, $cbor, $handmade)), digestAuth: $digestAuth)))->put(new Record(key: 'k'));
check($decoded->choice instanceof ChoiceUnknownVariant && $decoded->choice->name === 'otherVariant' && $decoded->choice->value->value() === 'Ag==', 'unknown union variant keeps its value');
check($decoded->tags === Absent::Value && is_nan($decoded->score) && $decoded->level === 1.5 && (string)$decoded->count === '18446744073709551616' && $decoded->ratio->equals(new BigDecimal('10.00')), 'half floats, null members, bignums and decimal fractions decode');
$error = static fn(int $status, array $headers, string $body) => new VaultClient(new ClientConfig(endpoint: 'https://vault.example', transport: new Fake(static fn() => new HttpResponse($status, $headers, $body)), digestAuth: $digestAuth));
try { $error(403, $cbor, Cbor::encode(new JsonObject(['__type' => 'example.vault#Denied', 'message' => 'no', 'reason' => 'policy'])))->put(new Record(key: 'k')); throw new RuntimeException('expected Denied'); }
catch (Denied $denied) { check($denied->metadata->name === 'Denied' && $denied->getMessage() === 'no' && $denied->reason === 'policy' && $denied->metadata->status === 403, 'modeled CBOR error'); }
try { $error(403, [...$cbor, 'x-amzn-errortype' => 'Denied'], Cbor::encode(new JsonObject(['code' => 'Denied'])))->put(new Record(key: 'k')); throw new RuntimeException('expected unknown error'); }
catch (UnknownServiceError $unknown) { check($unknown->metadata->status === 403, 'X-Amzn-ErrorType and code never identify RPC v2 errors'); }
try { $error(403, ['content-type' => 'application/cbor'], Cbor::encode(new JsonObject(['__type' => 'example.vault#Denied'])))->put(new Record(key: 'k')); throw new RuntimeException('expected unknown error'); }
catch (UnknownServiceError $unknown) { check($unknown->metadata->status === 403 && $unknown->metadata->name === 'UnknownError', 'a response without Smithy-Protocol is malformed'); }
try { $error(500, $cbor, "\xff\x00")->put(new Record(key: 'k')); throw new RuntimeException('expected unknown error'); }
catch (UnknownServiceError $unknown) { check($unknown->metadata->status === 500, 'a malformed CBOR error body keeps the status'); }
rejects(UnexpectedValueException::class, static fn() => $error(200, ['content-type' => 'application/cbor'], "\xa0")->put(new Record(key: 'k')), 'a success without Smithy-Protocol is malformed');
rejects(UnexpectedValueException::class, static fn() => $error(200, $cbor, Cbor::encode(new JsonObject(['key' => 'k', 'when' => 5])))->put(new Record(key: 'k')), 'timestamps require tag 1');
rejects(UnexpectedValueException::class, static fn() => $error(200, $cbor, Cbor::encode(new JsonObject(['key' => 'k', 'data' => 'text'])))->put(new Record(key: 'k')), 'blobs require byte strings');
$ping = new Fake(static fn() => new HttpResponse(200, ['smithy-protocol' => 'rpc-v2-cbor'], ''));
check((new VaultClient(new ClientConfig(endpoint: 'https://vault.example', transport: $ping)))->ping() instanceof Unit, 'Unit output');
check($ping->requests[0]->body === null && !isset($ping->requests[0]->headers['content-type']) && $ping->requests[0]->headers['accept'] === 'application/cbor', 'Unit input sends no body or content type');

// Digest through an injected transport: challenge, answer, preemptive reuse, stale nonce, and failure.
$attempts = [];
$server = new Fake(static function(HttpRequest $request) use (&$attempts, $cbor): HttpResponse {
    $authorization = $request->headers['authorization'] ?? null;
    $attempts[] = $authorization === null ? 'none' : (preg_match('/nc=(\w+)/', $authorization, $m) ? $m[1] : 'other');
    $challenge = ['www-authenticate' => 'Digest realm="vault", qop="auth", algorithm=SHA-256, nonce="n' . count($attempts) . '", opaque="o"' . (count($attempts) === 4 ? ', stale=true' : '')];
    if ($authorization === null || count($attempts) === 4) return new HttpResponse(401, [...$cbor, ...$challenge], '');
    return new HttpResponse(200, $cbor, $request->body);
});
$digestClient = new VaultClient(new ClientConfig(endpoint: 'http://127.0.0.1:1', transport: $server, digestAuth: static fn() => new BasicCredentials('Mufasa', 'Circle of Life')));
$digestClient->put(new Record(key: 'a')); check($attempts === ['none', '00000001'], 'challenge then answer');
$digestClient->put(new Record(key: 'b')); check($attempts === ['none', '00000001', '00000002'], 'preemptive answer with the next nonce count');
$digestClient->put(new Record(key: 'c')); check($attempts === ['none', '00000001', '00000002', '00000003', '00000001'], 'stale nonce is answered once with the new challenge');
$denied = new Fake(static fn() => new HttpResponse(401, [...$cbor, 'www-authenticate' => 'Digest realm="vault", qop="auth", nonce="x"'], ''));
rejects(UnknownServiceError::class, static fn() => (new VaultClient(new ClientConfig(endpoint: 'http://127.0.0.1:1', transport: $denied, digestAuth: static fn() => new BasicCredentials('u', 'wrong'))))->put(new Record(key: 'k')), 'rejected digest credentials');
check(count($denied->requests) === 2, 'a rejected answer is not retried');
$preset = new Fake(static fn() => new HttpResponse(401, [...$cbor, 'www-authenticate' => 'Digest realm="vault", nonce="x"'], ''));
rejects(UnknownServiceError::class, static fn() => (new VaultClient(new ClientConfig(endpoint: 'http://127.0.0.1:1', transport: $preset, headers: ['authorization' => 'Digest preset'], digestAuth: static fn() => throw new RuntimeException('provider must not run'))))->put(new Record(key: 'k')), 'caller-supplied Authorization bypasses digest');
check(count($preset->requests) === 1 && $preset->requests[0]->headers['authorization'] === 'Digest preset', 'caller-supplied Authorization is sent unchanged');
rejects(InvalidArgumentException::class, static fn() => (new VaultClient(new ClientConfig(endpoint: 'http://vault.example', transport: $server, digestAuth: $digestAuth)))->put(new Record(key: 'k')), 'digest credentials require HTTPS off loopback');

// SigV4a through the client: region set from configuration, then from the region.
$signed = new Fake(static fn() => new HttpResponse(200, $cbor, "\xa0"));
(new VaultClient(new ClientConfig(endpoint: 'https://vault.example', transport: $signed, credentials: static fn() => $credentials, region: 'us-west-2', sigv4aSigningRegionSet: ['us-west-2', 'eu-*'])))->sign(new SignInput(value: 'v'));
(new VaultClient(new ClientConfig(endpoint: 'https://vault.example', transport: $signed, credentials: static fn() => $credentials, region: 'us-west-2')))->sign(new SignInput(value: 'v'));
check($signed->requests[0]->headers['x-amz-region-set'] === 'us-west-2,eu-*' && $signed->requests[1]->headers['x-amz-region-set'] === 'us-west-2', 'SigV4a region sets');
check(str_starts_with($signed->requests[0]->headers['authorization'], 'AWS4-ECDSA-P256-SHA256 Credential=AKIDEXAMPLE/') && str_contains($signed->requests[0]->headers['authorization'], '/vault/aws4_request, SignedHeaders=accept;content-type;host;smithy-protocol;x-amz-content-sha256;x-amz-date;x-amz-region-set,'), 'SigV4a signs the RPC v2 request');
rejects(UnexpectedValueException::class, static fn() => (new VaultClient(new ClientConfig(endpoint: 'https://vault.example', transport: $signed, credentials: static fn() => $credentials)))->sign(new SignInput()), 'SigV4a needs a region set');
echo "PHP CBOR (RFC 8949), Digest (RFC 7616), SigV4a (AWS CRT vectors), and RPC v2 CBOR client checks passed\n";
