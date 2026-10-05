<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

final readonly class WireCodec
{
    private bool $omitNulls;
    private bool $ignoreUnionType;
    private bool $nonFinite;
    private bool $bigStrings;
    public function __construct(public JsonObject $schema, private \Closure $models)
    {
        $settings = $schema->get('protocolSettings');
        $setting = static fn(string $name): bool => $settings instanceof JsonObject && $settings->get($name) === true;
        $this->omitNulls = $setting('omitNullStructureMembers');
        $this->ignoreUnionType = $setting('ignoreUnknownUnionType');
        // The CBOR bridge carries non-finite floats as these strings between Cbor and this codec.
        $this->nonFinite = $setting('nonFiniteFloatStrings') || $schema->get('protocol') === 'rpcv2Cbor';
        $this->bigStrings = $setting('arbitraryPrecisionStrings');
    }
    public static function object(mixed $value): JsonObject
    {
        if (!$value instanceof JsonObject) throw new \UnexpectedValueException('Expected object');
        return $value;
    }
    /** @return list<mixed> */
    public static function items(mixed $value): array
    {
        if (!is_array($value) || !array_is_list($value)) throw new \UnexpectedValueException('Expected list');
        return $value;
    }
    public static function string(mixed $value): string
    {
        if (!is_string($value)) throw new \UnexpectedValueException('Expected string');
        return $value;
    }
    public function shape(string $target): JsonObject { return self::object(self::object($this->schema->get('shapes'))->get($target)); }
    public function context(string $target): WireContext { return new WireContext($this, $target); }
    public function equivalent(mixed $left, mixed $right): bool
    {
        if ($left instanceof JsonNumber && $right instanceof JsonNumber) return $left->canonical() === $right->canonical();
        if ($left instanceof JsonObject && $right instanceof JsonObject) {
            if (count($left->members) !== count($right->members)) return false;
            foreach ($left->members as $name => $value) if (!$right->has((string)$name) || !$this->equivalent($value, $right->get((string)$name))) return false;
            return true;
        }
        if (is_array($left) && is_array($right)) {
            if (count($left) !== count($right)) return false;
            foreach ($left as $i => $value) if (!$this->equivalent($value, $right[$i])) return false;
            return true;
        }
        return $left === $right;
    }
    private function collection(array $values, JsonObject $shape): array
    {
        if ($shape->get('sparse') !== true && in_array(null, $values, true)) throw new \UnexpectedValueException('Null in dense list');
        if ($shape->get('set') === true) foreach ($values as $index => $value) {
            for ($i = 0; $i < $index; $i++) if ($this->equivalent($value, $values[$i])) throw new \UnexpectedValueException('Duplicate set member');
        }
        return $values;
    }
    public function document(string $target, mixed $value, bool $encode): mixed
    {
        if ($value === null) return null;
        $shape = $this->shape($target);
        switch ($shape->get('kind')) {
            case 'structure': case 'union':
                $object = self::object($value); $result = []; $known = []; $wireNames = [];
                $structure = $shape->get('kind') === 'structure';
                foreach (self::object($shape->get('members'))->members as $name => $member) {
                    $wire = $member->get('jsonName') ?? $name;
                    $source = $encode ? $name : $wire;
                    $known[$source] = true; $wireNames[$wire] = true;
                    if (!$object->has($source)) continue;
                    $item = $object->get($source);
                    // JSON protocols treat a structure member's null as absence, except in documents.
                    if ($item === null && $structure && $this->omitNulls && $this->shape($member->get('target'))->get('kind') !== 'document') continue;
                    $result[$encode ? $wire : $name] = $this->document($member->get('target'), $item, $encode);
                }
                if ($shape->get('kind') === 'union') foreach ($object->members as $name => $item) {
                    if (isset($known[$name])) continue;
                    if (!$encode && $name === '__type' && $this->ignoreUnionType) continue;
                    if ($encode && isset($wireNames[$name])) throw new \UnexpectedValueException('Unknown variant name is declared');
                    $result[$name] = $item;
                }
                return new JsonObject($result);
            case 'list': return array_map(fn($item) => $this->document($shape->get('member')->get('target'), $item, $encode), self::items($value));
            case 'map': return new JsonObject(array_map(fn($item) => $this->document($shape->get('value')->get('target'), $item, $encode), self::object($value)->members));
            default: return $value;
        }
    }
    /** Arbitrary-precision text: a JSON number, or a string matching $grammar when the protocol uses strings. */
    private function bigText(mixed $value, string $grammar): string
    {
        if (!$this->bigStrings) return $this->number($value)->text;
        if (!is_string($value) || !preg_match($grammar, $value)) throw new \UnexpectedValueException('Expected arbitrary-precision number string');
        return $value;
    }
    private function number(mixed $value): JsonNumber
    {
        if (!$value instanceof JsonNumber) throw new \UnexpectedValueException('Expected number');
        return $value;
    }
    public function decode(mixed $value, WireContext $context): mixed
    {
        $shape = $context->shape();
        $kind = $shape->get('kind');
        if ($value === null && $kind !== 'document') throw new \UnexpectedValueException('Unexpected null');
        switch ($kind) {
            case 'unit':
                if (self::object($value)->members !== []) throw new \UnexpectedValueException('Expected unit');
                return new Unit();
            case 'string': return self::string($value);
            case 'boolean':
                if (!is_bool($value)) throw new \UnexpectedValueException('Expected boolean');
                return $value;
            case 'byte': return $this->number($value)->integer(-128, 127);
            case 'short': return $this->number($value)->integer(-32768, 32767);
            case 'integer': return $this->number($value)->integer(-2147483648, 2147483647);
            case 'long': return $this->number($value)->integer();
            case 'bigInteger': return new BigInteger($this->bigText($value, '/^-?(?:0|[1-9][0-9]*)$/D'));
            case 'bigDecimal': return new BigDecimal($this->bigText($value, '/^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?$/D'));
            case 'float':
            case 'double':
                if ($this->nonFinite && is_string($value)) {
                    return match ($value) { 'NaN' => NAN, 'Infinity' => INF, '-Infinity' => -INF, default => throw new \UnexpectedValueException('Invalid floating point string') };
                }
                $number = $this->number($value)->floating();
                if ($kind === 'float') $number = unpack('f', pack('f', $number))[1];
                if (!is_finite($number)) throw new \UnexpectedValueException('Float out of range');
                return $number;
            case 'blob':
                if ($shape->get('streaming') === true) {
                    if (!$value instanceof ByteStream) throw new \UnexpectedValueException('Expected ByteStream');
                    return $value;
                }
                $text = self::string($value);
                $bytes = base64_decode($text, true);
                if ($bytes === false || base64_encode($bytes) !== $text) throw new \UnexpectedValueException('Expected padded standard base64');
                return $bytes;
            case 'timestamp':
                return match ($context->format ?? $shape->get('format')) {
                    'epoch-seconds' => Timestamp::fromEpoch($this->number($value)),
                    'http-date' => Timestamp::fromHttpDate(self::string($value)),
                    default => Timestamp::parse(self::string($value)),
                };
            case 'document': return new JsonValue($value);
            case 'list':
                $child = $context->child(self::object($shape->get('member')));
                return array_map(fn($item) => $item === null && $shape->get('sparse') === true ? null : $this->decode($item, $child), $this->collection(self::items($value), $shape));
            case 'map':
                $child = $context->child(self::object($shape->get('value')));
                $key = $context->child(self::object($shape->get('key')));
                $result = [];
                foreach (self::object($value)->members as $name => $item) {
                    // PHP array keys use wire strings, including enum wire values.
                    $this->decode((string)$name, $key);
                    if ($item === null && $shape->get('sparse') !== true) throw new \UnexpectedValueException('Null in dense map');
                    $result[$name] = $item === null ? null : $this->decode($item, $child);
                }
                return $result;
            default: return ($this->models)($context->target, $value, $context);
        }
    }
    public function encode(mixed $value, WireContext $context): mixed
    {
        $shape = $context->shape();
        $kind = $shape->get('kind');
        if (in_array($kind, ['unit', 'structure', 'union', 'enum', 'intEnum'], true)) {
            if (!$value instanceof WireModel) throw new \UnexpectedValueException('Expected modeled value');
            return $value->toWire($context);
        }
        if ($kind === 'document') {
            if (!$value instanceof JsonValue) throw new \UnexpectedValueException('Expected JsonValue');
            return $value->value();
        }
        if ($kind === 'bigInteger' || $kind === 'bigDecimal') {
            if ($kind === 'bigInteger' ? !$value instanceof BigInteger : !$value instanceof BigDecimal) throw new \UnexpectedValueException('Expected exact number');
            if (!$this->bigStrings) return $value->number;
            // RPC v2 JSON strings use plain integer digits for bigInteger.
            return $kind === 'bigInteger' ? $value->number->scaled(0, 100000) : $value->number->text;
        }
        if ($kind === 'timestamp') {
            if (!$value instanceof Timestamp) throw new \UnexpectedValueException('Expected Timestamp');
            return match ($context->format ?? $shape->get('format')) {
                'epoch-seconds' => $value->toEpoch(), 'http-date' => $value->toHttpDate(),
                default => $value->toDateTimeString(),
            };
        }
        if ($kind === 'blob' && $shape->get('streaming') === true) {
            // A stream is the raw HTTP body; it never passes through JSON.
            if (!$value instanceof ByteStream) throw new \UnexpectedValueException('Expected ByteStream');
            return $value;
        }
        if ($kind === 'blob') return base64_encode(self::string($value));
        if ($kind === 'list') {
            $child = $context->child(self::object($shape->get('member')));
            return $this->collection(array_map(fn($item) => $item === null && $shape->get('sparse') === true ? null : $this->encode($item, $child), self::items($value)), $shape);
        }
        if ($kind === 'map') {
            if (!is_array($value)) throw new \UnexpectedValueException('Expected map');
            $child = $context->child(self::object($shape->get('value')));
            $key = $context->child(self::object($shape->get('key')));
            $result = [];
            foreach ($value as $name => $item) {
                $this->decode((string)$name, $key);
                $encoded = $item === null && $shape->get('sparse') === true ? null : $this->encode($item, $child);
                if ($encoded === null && $shape->get('sparse') !== true) throw new \UnexpectedValueException('Null in dense map');
                $result[$name] = $encoded;
            }
            return new JsonObject($result);
        }
        if (in_array($kind, ['byte', 'short', 'integer', 'long'], true)) {
            if (!is_int($value)) throw new \UnexpectedValueException('Expected integer');
            $encoded = new JsonNumber((string)$value);
            $this->decode($encoded, $context);
            return $encoded;
        }
        if ($kind === 'float' || $kind === 'double') {
            if (!is_int($value) && !is_float($value)) throw new \UnexpectedValueException('Expected number');
            if (is_float($value) && !is_finite($value)) {
                if (!$this->nonFinite) throw new \UnexpectedValueException('Non-finite number');
                return is_nan($value) ? 'NaN' : ($value > 0 ? 'Infinity' : '-Infinity');
            }
            return new JsonNumber(ExactJson::encode($this->decode(new JsonNumber(ExactJson::encode($value)), $context)));
        }
        $this->decode($value, $context);
        return $value;
    }
    /** @param ?string $format a binding timestamp format that replaces the member's document format */
    public function header(JsonObject $member, string $text, ?string $format = null): mixed
    {
        $format ??= $member->get('format');
        $shape = $this->shape(self::string($member->get('target')));
        $kind = $shape->get('kind');
        if ($kind === 'list') {
            if (trim($text) === '') return [];
            $parts = [];
            $part = '';
            $quoted = false;
            $escaped = false;
            $append = function () use (&$parts, &$part, $shape, $format): void {
                $value = trim($part);
                if (str_starts_with($value, '"')) $value = self::string(ExactJson::decode($value));
                $parts[] = $this->header(self::object($shape->get('member')), $value, $format);
                $part = '';
            };
            for ($index = 0; $index < strlen($text); $index++) {
                $character = $text[$index];
                if ($character === ',' && !$quoted) { $append(); continue; }
                $part .= $character;
                if ($escaped) $escaped = false;
                elseif ($character === '\\' && $quoted) $escaped = true;
                elseif ($character === '"') $quoted = !$quoted;
            }
            if ($quoted) throw new \UnexpectedValueException('Unterminated quoted header');
            $append();
            return $parts;
        }
        if (in_array($kind, ['byte', 'short', 'integer', 'long', 'intEnum', 'bigInteger', 'bigDecimal', 'float', 'double', 'boolean'], true) || ($kind === 'timestamp' && $format === 'epoch-seconds')) {
            return ExactJson::decode($text);
        }
        return $text;
    }
    public function bound(mixed $value): string
    {
        if (is_string($value)) return $value;
        if ($value instanceof JsonNumber) return $value->text;
        if (is_bool($value)) return $value ? 'true' : 'false';
        if (is_array($value)) {
            return implode(', ', array_map(function ($item): string {
                $text = $this->bound($item);
                return $text === '' || strpbrk($text, ',"\\') !== false || trim($text) !== $text ? ExactJson::encode($text) : $text;
            }, self::items($value)));
        }
        throw new \UnexpectedValueException('Expected bound scalar');
    }
    /**
     * Converts encoded wire values to the query view used by waiters and endpoint rules:
     * member names as keys, and timestamps as exact epoch seconds.
     */
    public function queryValue(?string $target, mixed $value, ?string $format = null): mixed
    {
        if ($target === null || $value === null) return null;
        $shape = $this->shape($target);
        switch ($shape->get('kind')) {
            case 'structure': case 'union':
                $object = self::object($value); $members = self::object($shape->get('members')); $result = [];
                foreach ($object->members as $name => $item) {
                    $member = $members->get((string)$name);
                    $result[$name] = $member instanceof JsonObject ? $this->queryValue($member->get('target'), $item, $member->get('format')) : $item;
                }
                return new JsonObject($result);
            case 'list':
                $member = self::object($shape->get('member'));
                return array_map(fn($item) => $this->queryValue($member->get('target'), $item, $member->get('format')), self::items($value));
            case 'map':
                $member = self::object($shape->get('value'));
                return new JsonObject(array_map(fn($item) => $this->queryValue($member->get('target'), $item, $member->get('format')), self::object($value)->members));
            case 'timestamp':
                return $this->decode($value, new WireContext($this, $target, $format))->toEpoch();
            case 'blob':
                return $value instanceof ByteStream ? null : $value;
            default: return $value;
        }
    }
}
