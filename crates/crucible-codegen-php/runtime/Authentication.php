<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

final class Authentication
{
    public static function apply(HttpRequest $request, JsonObject $op, JsonObject $schema, ClientConfig $config, CallScope $scope, ?DigestAuthentication $digest = null): HttpRequest
    {
        $schemes = $schema->get('authSchemes'); $ids = $op->get('auth') ?? ($op->get('bearer') ? ['smithy.api#httpBearerAuth'] : []);
        $endpointSchemes = $op->get('resolvedEndpoint')['properties']['authSchemes'] ?? null;
        foreach ($ids as $id) {
            $scheme = $schemes?->get($id) ?? new JsonObject(['kind'=>'bearer']); $kind = $scheme->get('kind');
            if (is_array($endpointSchemes)) {
                // A resolved endpoint narrows the schemes and may override signing properties.
                $match = null;
                foreach ($endpointSchemes as $candidate) {
                    if ($candidate instanceof JsonObject && is_string($candidate->get('name')) && strcasecmp($candidate->get('name'), $kind) === 0) { $match = $candidate; break; }
                }
                if ($match === null) continue;
                $members = $match->members; unset($members['name']);
                $scheme = new JsonObject([...$scheme->members, ...$members]);
            }
            $headers = array_change_key_case($request->headers, CASE_LOWER);
            if (in_array($kind,['bearer','basic','digest'],true) && isset($headers['authorization']) && str_starts_with(strtolower($headers['authorization']),$kind . ' ')) return self::secure($request);
            if ($kind === 'sigV4' && isset($headers['authorization']) && str_starts_with($headers['authorization'],'AWS4-HMAC-SHA256 ')) return self::secure($request);
            if ($kind === 'sigV4a' && isset($headers['authorization']) && str_starts_with($headers['authorization'],'AWS4-ECDSA-P256-SHA256 ')) return self::secure($request);
            if ($kind === 'apiKey' && $scheme->get('location') === 'header' && isset($headers[strtolower($scheme->get('name'))])) return self::secure($request);
            if ($kind === 'apiKey' && $scheme->get('location') === 'query') {
                foreach (explode('&',parse_url($request->url,PHP_URL_QUERY) ?? '') as $pair) if (rawurldecode(explode('=',$pair,2)[0]) === $scheme->get('name')) return self::secure($request);
            }
            $provider = match ($kind) { 'bearer'=>$config->token, 'basic'=>$config->basicAuth, 'apiKey'=>$config->apiKey, 'sigV4', 'sigV4a'=>$config->credentials, 'digest'=>$config->digestAuth, default=>null };
            if ($provider === null) continue;
            $identity = $provider(); $scope->check(); if ($identity === null) continue;
            if ($kind === 'sigV4' || $kind === 'sigV4a') {
                if (!$identity instanceof AwsCredentials) throw new \UnexpectedValueException('Expected AwsCredentials');
                if ($kind === 'sigV4') return self::secure(self::sign($request,$identity,$scheme->get('signingName'),$scheme->get('signingRegion') ?? $config->region ?? '', null,$op->get('unsignedPayload') === true));
                $regions = $scheme->get('signingRegionSet') ?? $config->sigv4aSigningRegionSet ?? ($config->region === null ? null : [$config->region]);
                if (!is_array($regions) || $regions === []) throw new \UnexpectedValueException('SigV4a requires a signing region set or region');
                return self::secure(self::signV4a($request,$identity,$scheme->get('signingName'),implode(',',array_map([WireCodec::class,'string'],$regions)),null,$op->get('unsignedPayload') === true));
            }
            if ($kind === 'digest') {
                if (!$identity instanceof BasicCredentials) throw new \UnexpectedValueException('Expected BasicCredentials');
                // Until a server challenge arrives the request carries no credentials; the runtime answers the 401.
                if ($digest === null || !$digest->ready()) return self::secure($request);
                $headers['authorization'] = $digest->authorization($identity,$request->method,$request->url,$request->stream === null ? ($request->body ?? '') : null);
                return self::secure($request->withHeaders($headers));
            }
            if ($kind === 'bearer') $headers['authorization'] = 'Bearer ' . WireCodec::string($identity);
            elseif ($kind === 'basic') {
                if (!$identity instanceof BasicCredentials) throw new \UnexpectedValueException('Expected BasicCredentials');
                $headers['authorization'] = 'Basic ' . base64_encode($identity->username . ':' . $identity->password);
            } else {
                $identity = WireCodec::string($identity); if ($identity === '') throw new \UnexpectedValueException('API key must be nonempty');
                if ($scheme->get('location') === 'query') {
                    $parts = explode('?', $request->url,2); $pairs = [];
                    foreach (isset($parts[1]) ? explode('&',$parts[1]) : [] as $pair) if (rawurldecode(explode('=',$pair,2)[0]) !== $scheme->get('name')) $pairs[] = $pair;
                    $pairs[] = rawurlencode($scheme->get('name')) . '=' . rawurlencode($identity);
                    return self::secure(new HttpRequest($parts[0] . '?' . implode('&',$pairs),$request->method,$headers,$request->body,$request->stream));
                }
                $headers[strtolower($scheme->get('name'))] = ($scheme->get('scheme') === null ? '' : $scheme->get('scheme') . ' ') . $identity;
            }
            return self::secure($request->withHeaders($headers));
        }
        if ($ids !== [] && array_filter($ids,static fn($id)=>$id !== 'smithy.api#httpBearerAuth') !== [] && $op->get('optionalAuth') !== true) throw new \UnexpectedValueException('No credentials for an enabled authentication scheme');
        return $request;
    }
    private static function secure(HttpRequest $request): HttpRequest
    {
        $url = parse_url($request->url);
        if (strtolower($url['scheme'] ?? '') !== 'https' && !in_array(strtolower(trim($url['host'] ?? '', '[]')),['localhost','127.0.0.1','::1'],true)) throw new \InvalidArgumentException('Credentials require HTTPS or loopback HTTP');
        return $request;
    }
    public static function sign(HttpRequest $request, AwsCredentials $credentials, string $service, string $region,
        ?\DateTimeImmutable $now = null, bool $unsignedPayload = false): HttpRequest
    {
        $now = ($now ?? new \DateTimeImmutable())->setTimezone(new \DateTimeZone('UTC'));
        if ($credentials->expiresAt !== null && $credentials->expiresAt <= $now) throw new \InvalidArgumentException('AWS credentials have expired');
        if ($service === '' || $region === '' || strpbrk($service . $region,"/\r\n") !== false) throw new \InvalidArgumentException('Invalid SigV4 service or region');
        $headers = array_change_key_case($request->headers, CASE_LOWER); unset($headers['authorization'],$headers['x-amz-security-token']);
        $headers['x-amz-date'] = $now->format('Ymd\THis\Z');
        if ($credentials->sessionToken !== null) $headers['x-amz-security-token'] = $credentials->sessionToken;
        $headers['x-amz-content-sha256'] = self::payloadHash($request, $unsignedPayload);
        [$canonicalRequest, $signed, $headers] = self::canonical($request, $headers, $service, $headers['x-amz-content-sha256']);
        $date = $now->format('Ymd'); $scope = "$date/$region/$service/aws4_request";
        $text = "AWS4-HMAC-SHA256\n" . $headers['x-amz-date'] . "\n$scope\n" . hash('sha256',$canonicalRequest);
        $key = 'AWS4' . $credentials->secretAccessKey; foreach ([$date,$region,$service,'aws4_request'] as $part) $key = hash_hmac('sha256',$part,$key,true);
        $headers['authorization'] = 'AWS4-HMAC-SHA256 Credential=' . $credentials->accessKeyId . "/$scope, SignedHeaders=$signed, Signature=" . hash_hmac('sha256',$text,$key);
        return $request->withHeaders($headers);
    }
    /**
     * AWS Signature Version 4a: an ECDSA P-256 signature over a region set, with a key derived
     * from the secret access key. $contentSha256Header false leaves x-amz-content-sha256 unsigned
     * and unsent (the published vectors' sign_body=false); the payload hash still ends the canonical request.
     */
    public static function signV4a(HttpRequest $request, AwsCredentials $credentials, string $service, string $regionSet,
        ?\DateTimeImmutable $now = null, bool $unsignedPayload = false, bool $contentSha256Header = true): HttpRequest
    {
        return self::signV4aParts($request, $credentials, $service, $regionSet, $now, $unsignedPayload, $contentSha256Header)[0];
    }
    /**
     * signV4a() with its intermediate canonical request and string to sign, for conformance tests.
     * @return array{HttpRequest, string, string}
     */
    public static function signV4aParts(HttpRequest $request, AwsCredentials $credentials, string $service, string $regionSet,
        ?\DateTimeImmutable $now = null, bool $unsignedPayload = false, bool $contentSha256Header = true): array
    {
        $now = ($now ?? new \DateTimeImmutable())->setTimezone(new \DateTimeZone('UTC'));
        if ($credentials->expiresAt !== null && $credentials->expiresAt <= $now) throw new \InvalidArgumentException('AWS credentials have expired');
        if ($service === '' || $regionSet === '' || strpbrk($service . $regionSet,"/\r\n") !== false) throw new \InvalidArgumentException('Invalid SigV4a service or region set');
        $headers = array_change_key_case($request->headers, CASE_LOWER);
        unset($headers['authorization'],$headers['x-amz-security-token'],$headers['x-amz-content-sha256']);
        $headers['x-amz-date'] = $now->format('Ymd\THis\Z');
        $headers['x-amz-region-set'] = $regionSet;
        if ($credentials->sessionToken !== null) $headers['x-amz-security-token'] = $credentials->sessionToken;
        $payload = self::payloadHash($request, $unsignedPayload);
        if ($contentSha256Header) $headers['x-amz-content-sha256'] = $payload;
        [$canonicalRequest, $signed, $headers] = self::canonical($request, $headers, $service, $payload);
        $scope = $now->format('Ymd') . "/$service/aws4_request";
        $text = "AWS4-ECDSA-P256-SHA256\n" . $headers['x-amz-date'] . "\n$scope\n" . hash('sha256',$canonicalRequest);
        if (!openssl_sign($text, $signature, self::sigV4aKey($credentials), OPENSSL_ALGO_SHA256)) throw new \RuntimeException('SigV4a signing failed');
        $headers['authorization'] = 'AWS4-ECDSA-P256-SHA256 Credential=' . $credentials->accessKeyId . "/$scope, SignedHeaders=$signed, Signature=" . bin2hex($signature);
        return [$request->withHeaders($headers), $canonicalRequest, $text];
    }
    /** The SigV4a ECDSA P-256 private key derived from AWS credentials (NIST SP 800-108 HMAC-SHA256 counter KDF). */
    public static function sigV4aKey(AwsCredentials $credentials): \OpenSSLAsymmetricKey
    {
        static $cache = null;
        if (!extension_loaded('openssl')) throw new \LogicException('SigV4a requires the openssl extension');
        $fingerprint = hash('sha256', $credentials->accessKeyId . "\0" . $credentials->secretAccessKey, true);
        if ($cache !== null && hash_equals($cache[0], $fingerprint)) return $cache[1];
        $limit = "\xff\xff\xff\xff\x00\x00\x00\x00\xff\xff\xff\xff\xff\xff\xff\xff\xbc\xe6\xfa\xad\xa7\x17\x9e\x84\xf3\xb9\xca\xc2\xfc\x63\x25\x4f"; // n - 2
        for ($counter = 1; $counter <= 254; $counter++) {
            $candidate = hash_hmac('sha256', "\x00\x00\x00\x01AWS4-ECDSA-P256-SHA256\x00" . $credentials->accessKeyId . chr($counter) . "\x00\x00\x01\x00", 'AWS4A' . $credentials->secretAccessKey, true);
            if (strcmp($candidate, $limit) > 0) continue;
            // d = candidate + 1 in big-endian bytes; candidate <= n - 2 cannot overflow.
            for ($i = 31; $candidate[$i] === "\xff"; $i--) $candidate[$i] = "\x00";
            $candidate[$i] = chr(ord($candidate[$i]) + 1);
            $key = openssl_pkey_new(['ec' => ['curve_name' => 'prime256v1', 'd' => $candidate]]);
            if ($key === false) throw new \RuntimeException('SigV4a key derivation failed');
            $cache = [$fingerprint, $key];
            return $key;
        }
        throw new \RuntimeException('SigV4a key derivation exhausted its counter');
    }
    private static function payloadHash(HttpRequest $request, bool $unsignedPayload): string
    {
        // A stream is never read to hash it: it signs its declared SHA-256 or UNSIGNED-PAYLOAD.
        return $unsignedPayload ? 'UNSIGNED-PAYLOAD' : ($request->stream !== null ? ($request->stream->sha256 ?? 'UNSIGNED-PAYLOAD') : hash('sha256',$request->body ?? ''));
    }
    /** @return array{string, string, array<string, string>} the canonical request, signed header names, and headers with host */
    private static function canonical(HttpRequest $request, array $headers, string $service, string $payload): array
    {
        $url = parse_url($request->url); $host = strtolower($url['host']); $port = $url['port'] ?? null;
        if ($port !== null && !(($url['scheme'] === 'https' && $port === 443) || ($url['scheme'] === 'http' && $port === 80))) $host .= ':' . $port;
        $headers['host'] = $host; $path = $url['path'] ?? '/';
        if ($service !== 's3') {
            $parts = []; foreach (explode('/',$path) as $part) { if ($part === '..') array_pop($parts); elseif ($part !== '' && $part !== '.') $parts[] = $part; }
            $path = '/' . implode('/',$parts) . (str_ends_with($path,'/') && $parts !== [] ? '/' : '');
            $path = str_replace('%2F','/',rawurlencode($path));
        }
        $query = []; foreach (explode('&',$url['query'] ?? '') as $pair) { if ($pair === '') continue; $parts = explode('=',$pair,2); $query[] = [rawurlencode(rawurldecode($parts[0])),rawurlencode(rawurldecode($parts[1] ?? ''))]; }
        usort($query, static fn($a,$b)=>strcmp($a[0],$b[0]) ?: strcmp($a[1],$b[1]));
        $query = implode('&',array_map(static fn($pair)=>$pair[0] . '=' . $pair[1],$query));
        $canonical = []; foreach ($headers as $name=>$value) if (!in_array($name,['authorization','connection','expect','transfer-encoding','user-agent','x-amzn-trace-id','accept-encoding','content-length'],true)) $canonical[$name] = preg_replace('/\s+/',' ',trim($value));
        ksort($canonical,SORT_STRING); $signed = implode(';',array_keys($canonical)); $lines = ''; foreach ($canonical as $name=>$value) $lines .= $name . ':' . $value . "\n";
        return [implode("\n",[$request->method,$path,$query,$lines,$signed,$payload]), $signed, $headers];
    }
}
