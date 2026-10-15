<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

final class Authentication
{
    /** Adds the bearer token to a request whose operation uses bearer authentication. */
    public static function apply(HttpRequest $request, JsonObject $op, ClientConfig $config, CallScope $scope): HttpRequest
    {
        if ($op->get('bearer') !== true) return $request;
        $headers = array_change_key_case($request->headers, CASE_LOWER);
        if (isset($headers['authorization']) && str_starts_with(strtolower($headers['authorization']), 'bearer ')) return self::secure($request);
        if ($config->token === null) return $request;
        $token = ($config->token)(); $scope->check();
        if ($token === null) return $request;
        $headers['authorization'] = 'Bearer ' . WireCodec::string($token);
        return self::secure($request->withHeaders($headers));
    }
    private static function secure(HttpRequest $request): HttpRequest
    {
        $url = parse_url($request->url);
        if (strtolower($url['scheme'] ?? '') !== 'https' && !in_array(strtolower(trim($url['host'] ?? '', '[]')),['localhost','127.0.0.1','::1'],true)) throw new \InvalidArgumentException('Credentials require HTTPS or loopback HTTP');
        return $request;
    }
}
