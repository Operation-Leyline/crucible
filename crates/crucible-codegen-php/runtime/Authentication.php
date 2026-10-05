<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

final class Authentication
{
    public static function apply(HttpRequest $request, JsonObject $op, JsonObject $schema, ClientConfig $config, CallScope $scope): HttpRequest
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
            if (in_array($kind,['bearer','basic'],true) && isset($headers['authorization']) && str_starts_with(strtolower($headers['authorization']),$kind . ' ')) return self::secure($request);
            if ($kind === 'sigV4' && isset($headers['authorization']) && str_starts_with($headers['authorization'],'AWS4-HMAC-SHA256 ')) return self::secure($request);
            if ($kind === 'apiKey' && $scheme->get('location') === 'header' && isset($headers[strtolower($scheme->get('name'))])) return self::secure($request);
            if ($kind === 'apiKey' && $scheme->get('location') === 'query') {
                foreach (explode('&',parse_url($request->url,PHP_URL_QUERY) ?? '') as $pair) if (rawurldecode(explode('=',$pair,2)[0]) === $scheme->get('name')) return self::secure($request);
            }
            $provider = match ($kind) { 'bearer'=>$config->token, 'basic'=>$config->basicAuth, 'apiKey'=>$config->apiKey, 'sigV4'=>$config->credentials, default=>null };
            if ($provider === null) continue;
            $identity = $provider(); $scope->check(); if ($identity === null) continue;
            if ($kind === 'sigV4') {
                if (!$identity instanceof AwsCredentials) throw new \UnexpectedValueException('Expected AwsCredentials');
                return self::secure(self::sign($request,$identity,$scheme->get('signingName'),$scheme->get('signingRegion') ?? $config->region ?? '', null,$op->get('unsignedPayload') === true));
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
        // A stream is never read to hash it: it signs its declared SHA-256 or UNSIGNED-PAYLOAD.
        $headers['x-amz-content-sha256'] = $unsignedPayload ? 'UNSIGNED-PAYLOAD' : ($request->stream !== null ? ($request->stream->sha256 ?? 'UNSIGNED-PAYLOAD') : hash('sha256',$request->body ?? ''));
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
        $canonicalRequest = implode("\n",[$request->method,$path,$query,$lines,$signed,$headers['x-amz-content-sha256']]);
        $date = $now->format('Ymd'); $scope = "$date/$region/$service/aws4_request";
        $text = "AWS4-HMAC-SHA256\n" . $headers['x-amz-date'] . "\n$scope\n" . hash('sha256',$canonicalRequest);
        $key = 'AWS4' . $credentials->secretAccessKey; foreach ([$date,$region,$service,'aws4_request'] as $part) $key = hash_hmac('sha256',$part,$key,true);
        $headers['authorization'] = 'AWS4-HMAC-SHA256 Credential=' . $credentials->accessKeyId . "/$scope, SignedHeaders=$signed, Signature=" . hash_hmac('sha256',$text,$key);
        return $request->withHeaders($headers);
    }
}
