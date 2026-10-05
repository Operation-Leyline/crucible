<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/**
 * Evaluates the shared endpoint rules IR. Rule values are JSON shaped:
 * strings, booleans, JsonNumber integers, list arrays, JsonObject, and null.
 */
final class Endpoints
{
    private static function fail(string $message): never { throw new EndpointException($message); }

    public static function hostLabel(mixed $value, bool $subdomains = false): bool
    {
        if (!is_string($value) || $value === '') return false;
        foreach ($subdomains ? explode('.', $value) : [$value] as $label) {
            if (!preg_match('/^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$/D', $label)) return false;
        }
        return true;
    }

    public static function parseUrl(mixed $value): ?JsonObject
    {
        if (!is_string($value) || preg_match('/[\x00-\x20\x7f?#]/', $value)) return null;
        $url = parse_url($value);
        if ($url === false || !isset($url['scheme'], $url['host']) || isset($url['user']) || isset($url['pass'])) return null;
        $scheme = strtolower($url['scheme']);
        if (!in_array($scheme, ['http', 'https'], true) || !str_starts_with(strtolower($value), $scheme . '://')) return null;
        $host = $url['host'];
        $bracketed = str_starts_with($host, '[');
        $address = trim($host, '[]');
        $ip = filter_var($address, FILTER_VALIDATE_IP) !== false;
        if ($bracketed !== ($ip && str_contains($address, ':'))) return null;
        if (!$ip && !self::hostLabel(rtrim($host, '.'), true)) return null;
        $authority = strtolower($address);
        if (str_contains($authority, ':')) $authority = '[' . $authority . ']';
        $port = $url['port'] ?? null;
        if ($port !== null && $port !== ($scheme === 'https' ? 443 : 80)) $authority .= ':' . $port;
        $path = $url['path'] ?? '';
        if ($path === '') $path = '/';
        return new JsonObject(['scheme' => $scheme, 'authority' => $authority, 'path' => $path,
            'normalizedPath' => str_ends_with($path, '/') ? $path : $path . '/', 'isIp' => $ip]);
    }

    private static function integer(mixed $value): ?int
    {
        if (is_int($value)) return $value;
        if (!$value instanceof JsonNumber) return null;
        try { return $value->integer(); } catch (\UnexpectedValueException) { return null; }
    }

    public static function attribute(mixed $value, mixed $path): mixed
    {
        $steps = [];
        if (is_string($path)) {
            foreach (explode('.', $path) as $part) {
                if (!preg_match('/^([^\[]*)(?:\[(-?[0-9]+)\])?$/D', $part, $match)) self::fail('Invalid endpoint attribute path');
                if ($match[1] !== '') $steps[] = ['member', $match[1]];
                if (isset($match[2])) $steps[] = ['index', (int)$match[2]];
            }
        } else {
            foreach (WireCodec::items($path) as $step) {
                $step = WireCodec::object($step);
                $steps[] = $step->get('kind') === 'member' ? ['member', WireCodec::string($step->get('name'))] : ['index', self::integer($step->get('index')) ?? self::fail('Invalid endpoint index')];
            }
        }
        foreach ($steps as [$kind, $key]) {
            if ($kind === 'member') $value = $value instanceof JsonObject ? $value->get($key) : null;
            else {
                if (!is_array($value)) return null;
                if ($key < 0) $key += count($value);
                $value = $value[$key] ?? null;
            }
            if ($value === null) return null;
        }
        return $value;
    }

    /** @param list<mixed> $args */
    public static function call(string $name, array $args): mixed
    {
        switch ($name) {
            case 'isSet': return $args[0] !== null;
            case 'not':
                if (!is_bool($args[0])) self::fail('Endpoint not expects a boolean');
                return !$args[0];
            case 'booleanEquals': return is_bool($args[0]) && is_bool($args[1]) && $args[0] === $args[1];
            case 'stringEquals': return is_string($args[0]) && is_string($args[1]) && $args[0] === $args[1];
            case 'getAttr': return self::attribute($args[0], $args[1]);
            case 'substring':
                [$text, $start, $end, $reverse] = [$args[0], self::integer($args[1]), self::integer($args[2]), $args[3]];
                if (!is_string($text) || preg_match('/[^\x00-\x7f]/', $text) || $start === null || $end === null || $start < 0 || $end < $start || $end > strlen($text)) return null;
                return $reverse === true ? substr($text, strlen($text) - $end, $end - $start) : substr($text, $start, $end - $start);
            case 'isValidHostLabel': return self::hostLabel($args[0], $args[1] === true);
            case 'parseUrl': return self::parseUrl($args[0]);
            case 'uriEncode': return rawurlencode(WireCodec::string($args[0]));
        }
        self::fail('Unsupported compiled endpoint function ' . $name);
    }

    /** @param array<string, mixed> $values */
    private static function expression(JsonObject $node, array $values): mixed
    {
        switch ($node->get('kind')) {
            case 'reference': return $values[WireCodec::string($node->get('name'))] ?? null;
            case 'boolean': case 'integer': return $node->get('value');
            case 'array': return array_map(static fn($item) => self::expression(WireCodec::object($item), $values), WireCodec::items($node->get('values')));
            case 'object': return new JsonObject(array_map(static fn($item) => self::expression(WireCodec::object($item), $values), WireCodec::object($node->get('values'))->members));
            case 'call': return self::call(WireCodec::string($node->get('function')), array_map(static fn($item) => self::expression(WireCodec::object($item), $values), WireCodec::items($node->get('arguments'))));
            case 'string':
                $text = '';
                foreach (WireCodec::items($node->get('parts')) as $part) {
                    $part = WireCodec::object($part);
                    $value = $part->get('kind') === 'literal' ? $part->get('value') : self::attribute($values[WireCodec::string($part->get('name'))] ?? null, $part->get('path') ?? []);
                    $text .= match (true) {
                        is_string($value) => $value, is_bool($value) => $value ? 'true' : 'false',
                        $value instanceof JsonNumber => $value->text, is_int($value) => (string)$value,
                        default => self::fail('Endpoint template references a missing or non-scalar parameter'),
                    };
                }
                return $text;
        }
        self::fail('Unsupported compiled endpoint expression');
    }

    /**
     * @param list<mixed> $rules
     * @param array<string, mixed> $parameters
     * @return array{url: string, headers: array<string, list<string>>, properties: array<string, mixed>}|null
     */
    private static function rules(array $rules, array $parameters): ?array
    {
        foreach ($rules as $rule) {
            $rule = WireCodec::object($rule);
            $scope = $parameters;
            foreach (WireCodec::items($rule->get('conditions')) as $condition) {
                $condition = WireCodec::object($condition);
                $value = self::call(WireCodec::string($condition->get('function')), array_map(static fn($item) => self::expression(WireCodec::object($item), $scope), WireCodec::items($condition->get('arguments'))));
                if ($value === null || $value === false) continue 2;
                if (is_string($assign = $condition->get('assign'))) $scope[$assign] = $value;
            }
            $result = WireCodec::object($rule->get('result'));
            switch ($result->get('kind')) {
                case 'tree':
                    $nested = self::rules(WireCodec::items($result->get('rules')), $scope);
                    if ($nested !== null) return $nested;
                    break;
                case 'error': self::fail((string)self::expression(WireCodec::object($result->get('message')), $scope));
                default:
                    $url = self::expression(WireCodec::object($result->get('url')), $scope);
                    if (self::parseUrl($url) === null) self::fail('Endpoint rule returned an invalid HTTP(S) URL');
                    $headers = [];
                    foreach (WireCodec::object($result->get('headers'))->members as $name => $items) {
                        foreach (WireCodec::items($items) as $item) {
                            $value = self::expression(WireCodec::object($item), $scope);
                            if (!is_string($value)) self::fail('Endpoint header values must be strings');
                            $headers[strtolower((string)$name)][] = $value;
                        }
                    }
                    $properties = array_map(static fn($item) => self::expression(WireCodec::object($item), $scope), WireCodec::object($result->get('properties'))->members);
                    return ['url' => $url, 'headers' => $headers, 'properties' => $properties];
            }
        }
        return null;
    }

    /**
     * Binds parameters from their modeled sources in order, then evaluates the rules.
     * @param array<string, mixed> $client @param array<string, mixed> $builtins
     * @return array{url: string, headers: array<string, list<string>>, properties: array<string, mixed>}
     */
    public static function resolve(JsonObject $ruleSet, ?JsonObject $sources, mixed $input, array $client, array $builtins): array
    {
        $parameters = [];
        foreach (WireCodec::object($ruleSet->get('parameters'))->members as $name => $parameter) {
            $name = (string)$name;
            $parameter = WireCodec::object($parameter);
            $value = null;
            foreach ($sources?->get($name) ?? [] as $source) {
                $source = WireCodec::object($source);
                $value = match ($source->get('kind')) {
                    'static', 'default' => $source->get('value'),
                    'inputMember' => $input instanceof JsonObject ? $input->get(WireCodec::string($source->get('member'))) : null,
                    'inputPath' => Query::search(WireCodec::object(WireCodec::object($source->get('path'))->get('expression')), $input),
                    'client' => $client[WireCodec::string($source->get('name'))] ?? null,
                    'builtIn' => $builtins[WireCodec::string($source->get('name'))] ?? null,
                    default => null,
                };
                if ($value !== null) break;
            }
            if ($value === null && $parameter->get('required') === true) self::fail('Required endpoint parameter ' . $name . ' is missing');
            if ($value !== null) {
                $valid = match ($parameter->get('kind')) {
                    'string' => is_string($value), 'boolean' => is_bool($value),
                    'stringArray' => is_array($value) && array_is_list($value) && array_filter($value, static fn($item) => !is_string($item)) === [],
                    default => false,
                };
                if (!$valid) self::fail('Endpoint parameter ' . $name . ' has the wrong type');
            }
            $parameters[$name] = $value;
        }
        return self::rules(WireCodec::items($ruleSet->get('rules')), $parameters) ?? self::fail('No endpoint rule matched');
    }

    /** Expands a modeled host prefix; every label must be a single valid host label. */
    public static function hostPrefix(string $url, JsonObject $description, mixed $input): string
    {
        $prefix = '';
        foreach (WireCodec::items($description->get('parts')) as $part) {
            $part = WireCodec::object($part);
            if ($part->get('kind') === 'literal') { $prefix .= WireCodec::string($part->get('value')); continue; }
            $member = WireCodec::string($part->get('member'));
            $value = $input instanceof JsonObject ? $input->get($member) : null;
            if (!self::hostLabel($value)) self::fail('Invalid endpoint host label ' . $member);
            $prefix .= $value;
        }
        $parts = parse_url($url);
        if ($parts === false || !isset($parts['scheme'], $parts['host'])) self::fail('Invalid endpoint URL');
        $host = $prefix . $parts['host'];
        if (strlen($host) > 253 || !self::hostLabel($host, true)) self::fail('Invalid endpoint hostname after prefix expansion');
        return $parts['scheme'] . '://' . $host . (isset($parts['port']) ? ':' . $parts['port'] : '') . ($parts['path'] ?? '');
    }
}
