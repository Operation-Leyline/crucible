<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

final readonly class WireCodec
{
    public function __construct(public JsonObject $schema, private \Closure $models) {}
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
            case 'float':
            case 'double':
                $number = $this->number($value)->floating();
                if ($kind === 'float') $number = unpack('f', pack('f', $number))[1];
                if (!is_finite($number)) throw new \UnexpectedValueException('Float out of range');
                return $number;
            case 'blob':
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
                return array_map(fn($item) => $this->decode($item, $child), self::items($value));
            case 'map':
                $child = $context->child(self::object($shape->get('value')));
                $key = $context->child(self::object($shape->get('key')));
                $result = [];
                foreach (self::object($value)->members as $name => $item) {
                    // PHP array keys use wire strings, including enum wire values.
                    $this->decode((string)$name, $key);
                    $result[$name] = $this->decode($item, $child);
                }
                return $result;
            default: return ($this->models)($context->target, $value, $context);
        }
    }
    public function encode(mixed $value, WireContext $context): mixed
    {
        $shape = $context->shape();
        $kind = $shape->get('kind');
        if (in_array($kind, ['unit', 'structure', 'union', 'enum'], true)) {
            if (!$value instanceof WireModel) throw new \UnexpectedValueException('Expected modeled value');
            return $value->toWire($context);
        }
        if ($kind === 'document') {
            if (!$value instanceof JsonValue) throw new \UnexpectedValueException('Expected JsonValue');
            return $value->value();
        }
        if ($kind === 'timestamp') {
            if (!$value instanceof Timestamp) throw new \UnexpectedValueException('Expected Timestamp');
            return match ($context->format ?? $shape->get('format')) {
                'epoch-seconds' => $value->toEpoch(), 'http-date' => $value->toHttpDate(),
                default => $value->toDateTimeString(),
            };
        }
        if ($kind === 'blob') return base64_encode(self::string($value));
        if ($kind === 'list') {
            $child = $context->child(self::object($shape->get('member')));
            return array_map(fn($item) => $this->encode($item, $child), self::items($value));
        }
        if ($kind === 'map') {
            if (!is_array($value)) throw new \UnexpectedValueException('Expected map');
            $child = $context->child(self::object($shape->get('value')));
            $key = $context->child(self::object($shape->get('key')));
            $result = [];
            foreach ($value as $name => $item) {
                $this->decode((string)$name, $key);
                $result[$name] = $this->encode($item, $child);
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
            return new JsonNumber(ExactJson::encode($this->decode(new JsonNumber(ExactJson::encode($value)), $context)));
        }
        $this->decode($value, $context);
        return $value;
    }
    public function header(JsonObject $member, string $text): mixed
    {
        $shape = $this->shape(self::string($member->get('target')));
        $kind = $shape->get('kind');
        if ($kind === 'list') {
            if (trim($text) === '') return [];
            $parts = [];
            $part = '';
            $quoted = false;
            $escaped = false;
            $append = function () use (&$parts, &$part, $shape): void {
                $value = trim($part);
                if (str_starts_with($value, '"')) $value = self::string(ExactJson::decode($value));
                $parts[] = $this->header(self::object($shape->get('member')), $value);
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
        if (in_array($kind, ['byte', 'short', 'integer', 'long', 'float', 'double', 'boolean'], true) || ($kind === 'timestamp' && $member->get('format') === 'epoch-seconds')) {
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

}
