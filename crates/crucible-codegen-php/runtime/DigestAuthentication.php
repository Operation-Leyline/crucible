<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/**
 * RFC 7616 HTTP Digest authentication. A client keeps the last accepted challenge and
 * answers later requests preemptively with an increasing nonce count; a 401 carrying a
 * new challenge (or stale=true) replaces it. MD5, SHA-256, and SHA-512-256 are supported,
 * with their -sess variants, qop "auth" (preferred) or "auth-int", and userhash.
 */
final class DigestAuthentication
{
    private const ALGORITHMS = ['SHA-512-256' => 'sha512/256', 'SHA-256' => 'sha256', 'MD5' => 'md5'];
    /** @var ?array<string, string> */
    private ?array $challenge = null;
    private int $count = 0;

    /**
     * Every challenge in a WWW-Authenticate value, with lowercase parameter names.
     * @return list<array{scheme: string, params: array<string, string>}>
     */
    public static function challenges(string $header): array
    {
        $result = [];
        $pattern = '/\G[\s,]*([!#$%&\'*+.^_`|~0-9A-Za-z-]+)(?:[ \t]*=[ \t]*("(?:[^"\\\\]|\\\\.)*"|[^,\s]*))?/';
        $offset = 0;
        while ($offset < strlen($header) && preg_match($pattern, $header, $match, 0, $offset)) {
            if ($match[0] === '') break;
            $offset += strlen($match[0]);
            if (!isset($match[2])) {
                $result[] = ['scheme' => strtolower($match[1]), 'params' => []];
                continue;
            }
            if ($result === []) continue;
            $value = $match[2];
            if (str_starts_with($value, '"')) $value = preg_replace('/\\\\(.)/s', '$1', substr($value, 1, -1));
            $result[count($result) - 1]['params'][strtolower($match[1])] = $value;
        }
        return $result;
    }

    /**
     * The strongest supported Digest challenge in a WWW-Authenticate value, or null.
     * @return ?array<string, string>
     */
    public static function select(string $header): ?array
    {
        $best = null; $rank = PHP_INT_MAX;
        foreach (self::challenges($header) as $challenge) {
            $params = $challenge['params'];
            if ($challenge['scheme'] !== 'digest' || !isset($params['realm'], $params['nonce'])) continue;
            $algorithm = strtoupper($params['algorithm'] ?? 'MD5');
            $base = str_ends_with($algorithm, '-SESS') ? substr($algorithm, 0, -5) : $algorithm;
            $position = array_search($base, array_keys(self::ALGORITHMS), true);
            if ($position === false) continue;
            if (isset($params['qop']) && array_intersect(self::qops($params), ['auth', 'auth-int']) === []) continue;
            if ($position < $rank) { $best = $params; $rank = $position; }
        }
        return $best;
    }

    /** @return list<string> */
    private static function qops(array $params): array
    {
        return array_values(array_filter(array_map(static fn($qop) => strtolower(trim($qop)), explode(',', $params['qop'] ?? ''))));
    }

    /** @param array<string, string> $challenge a challenge from select() */
    public function accept(array $challenge): void
    {
        $this->challenge = $challenge;
        $this->count = 0;
    }

    public function ready(): bool { return $this->challenge !== null; }

    /** Answers the accepted challenge for a request; a body of null (a stream) cannot use auth-int. */
    public function authorization(BasicCredentials $credentials, string $method, string $url, ?string $body, ?string $cnonce = null): string
    {
        if ($this->challenge === null) throw new \LogicException('No digest challenge');
        $parts = parse_url($url);
        $uri = ($parts['path'] ?? '/') . (isset($parts['query']) ? '?' . $parts['query'] : '');
        $this->count++;
        return self::header($this->challenge, $credentials->username, $credentials->password, $method, $uri, $body,
            sprintf('%08x', $this->count), $cnonce ?? bin2hex(random_bytes(16)));
    }

    /** The Authorization value for one challenge, nonce count, and client nonce. */
    public static function header(array $challenge, string $username, string $password, string $method, string $uri,
        ?string $body, string $nc, string $cnonce): string
    {
        $algorithm = strtoupper($challenge['algorithm'] ?? 'MD5');
        $session = str_ends_with($algorithm, '-SESS');
        $function = self::ALGORITHMS[$session ? substr($algorithm, 0, -5) : $algorithm] ?? throw new \UnexpectedValueException('Unsupported digest algorithm');
        $hash = static fn(string $text): string => hash($function, $text);
        $realm = $challenge['realm']; $nonce = $challenge['nonce'];
        $qops = self::qops($challenge);
        $qop = in_array('auth', $qops, true) ? 'auth' : (in_array('auth-int', $qops, true) ? 'auth-int' : null);
        if ($qop === null && $qops !== []) throw new \UnexpectedValueException('Unsupported digest qop');
        if ($qop === 'auth-int' && $body === null) throw new \UnexpectedValueException('Digest auth-int cannot cover a streamed body');
        $a1 = $hash("$username:$realm:$password");
        if ($session) $a1 = $hash("$a1:$nonce:$cnonce");
        $a2 = $hash($qop === 'auth-int' ? "$method:$uri:" . $hash($body) : "$method:$uri");
        $response = $qop === null ? $hash("$a1:$nonce:$a2") : $hash("$a1:$nonce:$nc:$cnonce:$qop:$a2");
        $userhash = strtolower($challenge['userhash'] ?? '') === 'true';
        $quote = static fn(string $value): string => '"' . addcslashes($value, '"\\') . '"';
        if ($userhash) $user = 'username=' . $quote($hash("$username:$realm"));
        // A name that cannot be a quoted ASCII string travels as an RFC 5987 extended value.
        elseif (preg_match('/[^\x20-\x7e]|["\\\\]/', $username)) $user = "username*=UTF-8''" . rawurlencode($username);
        else $user = 'username=' . $quote($username);
        $fields = [$user, 'realm=' . $quote($realm), 'uri=' . $quote($uri), 'algorithm=' . ($challenge['algorithm'] ?? 'MD5'), 'nonce=' . $quote($nonce)];
        if ($qop !== null) array_push($fields, "nc=$nc", 'cnonce=' . $quote($cnonce), "qop=$qop");
        $fields[] = 'response=' . $quote($response);
        if (isset($challenge['opaque'])) $fields[] = 'opaque=' . $quote($challenge['opaque']);
        if ($userhash) $fields[] = 'userhash=true';
        return 'Digest ' . implode(', ', $fields);
    }
}
