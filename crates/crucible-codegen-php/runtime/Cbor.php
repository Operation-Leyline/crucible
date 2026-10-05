<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/**
 * RFC 8949 CBOR for the Smithy RPC v2 CBOR protocol, in pure PHP.
 *
 * Decoded items are ints (or a JsonNumber beyond 64 bits), floats, strings (text),
 * CborBytes, lists, JsonObject maps with text keys, CborTag, bools, and null
 * (undefined decodes to null). fromWire()/toWire() convert between CBOR items and
 * the JSON-shaped wire values WireCodec produces, directed by the schema, so
 * blobs become byte strings, timestamps tag 1 epochs, and big numbers tags 2-4.
 */
final class Cbor
{
    private int $position = 0;
    /** Marks the break that ends an indefinite-length item. */
    private readonly object $break;
    private function __construct(private readonly string $source) { $this->break = new \stdClass(); }

    public static function decode(string $source): mixed
    {
        $decoder = new self($source);
        $value = $decoder->item(0);
        if ($value === $decoder->break) throw new \UnexpectedValueException('Unexpected CBOR break');
        if ($decoder->position !== strlen($source)) throw new \UnexpectedValueException('Trailing CBOR input');
        return $value;
    }

    public static function encode(mixed $value, int $depth = 0): string
    {
        if ($depth > 255) throw new \UnexpectedValueException('CBOR nesting limit');
        if ($value === null) return "\xf6";
        if ($value === false) return "\xf4";
        if ($value === true) return "\xf5";
        if (is_int($value)) return $value >= 0 ? self::head(0, $value) : self::head(1, -1 - $value);
        if (is_float($value)) {
            // Smithy asks for the smallest lossless form and no half-precision floats.
            if (is_nan($value)) return "\xfa\x7f\xc0\x00\x00";
            $single = pack('G', $value);
            if (is_infinite($value) || unpack('G', $single)[1] === $value) return "\xfa" . $single;
            return "\xfb" . pack('E', $value);
        }
        if (is_string($value)) {
            if (!preg_match('//u', $value)) throw new \UnexpectedValueException('CBOR text must be UTF-8');
            return self::head(3, strlen($value)) . $value;
        }
        if ($value instanceof CborBytes) return self::head(2, strlen($value->bytes)) . $value->bytes;
        if ($value instanceof CborTag) return self::head(6, $value->tag) . self::encode($value->value, $depth + 1);
        if ($value instanceof JsonObject) {
            $out = self::head(5, count($value->members));
            foreach ($value->members as $key => $item) $out .= self::encode((string)$key, $depth + 1) . self::encode($item, $depth + 1);
            return $out;
        }
        if (is_array($value) && array_is_list($value)) {
            $out = self::head(4, count($value));
            foreach ($value as $item) $out .= self::encode($item, $depth + 1);
            return $out;
        }
        throw new \UnexpectedValueException('Not a CBOR value');
    }

    private static function head(int $major, int $argument): string
    {
        $type = $major << 5;
        if ($argument < 24) return chr($type | $argument);
        if ($argument <= 0xff) return chr($type | 24) . chr($argument);
        if ($argument <= 0xffff) return chr($type | 25) . pack('n', $argument);
        if ($argument <= 0xffffffff) return chr($type | 26) . pack('N', $argument);
        return chr($type | 27) . pack('J', $argument);
    }

    private function bytes(int $length): string
    {
        if ($length < 0 || $length > strlen($this->source) - $this->position) throw new \UnexpectedValueException('Truncated CBOR input');
        $bytes = substr($this->source, $this->position, $length);
        $this->position += $length;
        return $bytes;
    }

    /** @return array{int, int|string|null} the additional information and its argument; a decimal string beyond PHP_INT_MAX */
    private function argument(int $info): array
    {
        if ($info < 24) return [$info, $info];
        return match ($info) {
            24 => [$info, ord($this->bytes(1))],
            25 => [$info, unpack('n', $this->bytes(2))[1]],
            26 => [$info, unpack('N', $this->bytes(4))[1]],
            27 => [$info, self::unsigned($this->bytes(8))],
            31 => [$info, null],
            default => throw new \UnexpectedValueException('Reserved CBOR additional information'),
        };
    }

    private static function unsigned(string $bytes): int|string
    {
        $value = unpack('J', $bytes)[1];
        return $value >= 0 ? $value : self::toDecimal($bytes);
    }

    private function length(mixed $argument): int
    {
        // A definite length can never exceed the remaining input, so nothing large is allocated.
        if (is_string($argument) || $argument > strlen($this->source) - $this->position) throw new \UnexpectedValueException('Truncated CBOR input');
        return $argument;
    }

    private function item(int $depth): mixed
    {
        if ($depth > 255) throw new \UnexpectedValueException('CBOR nesting limit');
        $initial = ord($this->bytes(1));
        $major = $initial >> 5;
        [$info, $argument] = $this->argument($initial & 31);
        if ($info === 31 && !in_array($major, [2, 3, 4, 5, 7], true)) throw new \UnexpectedValueException('Invalid indefinite CBOR item');
        switch ($major) {
            case 0: return is_int($argument) ? $argument : new JsonNumber($argument);
            case 1: return is_int($argument) ? -1 - $argument : new JsonNumber('-' . self::increment($argument));
            case 2:
            case 3:
                if ($info === 31) {
                    $text = '';
                    while (true) {
                        $chunk = ord($this->bytes(1));
                        if ($chunk === 0xff) break;
                        if ($chunk >> 5 !== $major || ($chunk & 31) === 31) throw new \UnexpectedValueException('Invalid CBOR string chunk');
                        [, $length] = $this->argument($chunk & 31);
                        $text .= $this->bytes($this->length($length));
                    }
                } else {
                    $text = $this->bytes($this->length($argument));
                }
                if ($major === 2) return new CborBytes($text);
                if (!preg_match('//u', $text)) throw new \UnexpectedValueException('CBOR text is not UTF-8');
                return $text;
            case 4:
                $items = [];
                if ($info === 31) {
                    while (($item = $this->item($depth + 1)) !== $this->break) $items[] = $item;
                    return $items;
                }
                for ($i = 0, $n = $this->length($argument); $i < $n; $i++) $items[] = $this->value($depth);
                return $items;
            case 5:
                $members = [];
                for ($i = 0, $n = $info === 31 ? PHP_INT_MAX : $this->length($argument); $i < $n; $i++) {
                    $key = $this->item($depth + 1);
                    if ($key === $this->break && $info === 31) break;
                    if (is_int($key)) $key = (string)$key;
                    if (!is_string($key)) throw new \UnexpectedValueException('CBOR map keys must be text');
                    if (array_key_exists($key, $members)) throw new \UnexpectedValueException('Duplicate CBOR map key');
                    $members[$key] = $this->value($depth);
                }
                return new JsonObject($members);
            case 6:
                if (!is_int($argument)) throw new \UnexpectedValueException('CBOR tag out of range');
                return new CborTag($argument, $this->value($depth));
            default:
                return match ($info) {
                    20 => false, 21 => true, 22, 23 => null,
                    25 => self::half($argument),
                    26 => unpack('G', pack('N', $argument))[1],
                    27 => unpack('E', is_int($argument) ? pack('J', $argument) : self::fromDecimal($argument, 8))[1],
                    31 => $this->break,
                    default => throw new \UnexpectedValueException('Unsupported CBOR simple value'),
                };
        }
    }

    private function value(int $depth): mixed
    {
        $item = $this->item($depth + 1);
        if ($item === $this->break) throw new \UnexpectedValueException('Unexpected CBOR break');
        return $item;
    }

    private static function half(int $bits): float
    {
        $exponent = ($bits >> 10) & 0x1f;
        $mantissa = $bits & 0x3ff;
        $value = match (true) {
            $exponent === 0 => $mantissa * 2 ** -24,
            $exponent === 31 => $mantissa === 0 ? INF : NAN,
            default => ($mantissa + 1024) * 2 ** ($exponent - 25),
        };
        return $bits & 0x8000 ? -$value : $value;
    }

    /** Unsigned big-endian bytes to decimal digits. */
    public static function toDecimal(string $bytes): string
    {
        $limbs = [0]; // base 1e9, least significant first
        foreach (str_split($bytes) as $byte) {
            $carry = ord($byte);
            foreach ($limbs as $i => $limb) {
                $value = $limb * 256 + $carry;
                $limbs[$i] = $value % 1000000000;
                $carry = intdiv($value, 1000000000);
            }
            while ($carry > 0) { $limbs[] = $carry % 1000000000; $carry = intdiv($carry, 1000000000); }
        }
        $text = (string)array_pop($limbs);
        while ($limbs !== []) $text .= str_pad((string)array_pop($limbs), 9, '0', STR_PAD_LEFT);
        return $text;
    }

    /** Decimal digits to minimal unsigned big-endian bytes, or exactly $width bytes. */
    public static function fromDecimal(string $digits, ?int $width = null): string
    {
        $bytes = '';
        $digits = ltrim($digits, '0');
        while ($digits !== '') {
            $quotient = ''; $remainder = 0;
            foreach (str_split($digits, 9) as $chunk) {
                $value = $remainder * 10 ** strlen($chunk) + (int)$chunk;
                $quotient .= str_pad((string)intdiv($value, 256), strlen($chunk), '0', STR_PAD_LEFT);
                $remainder = $value % 256;
            }
            $bytes = chr($remainder) . $bytes;
            $digits = ltrim($quotient, '0');
        }
        if ($width === null) return $bytes;
        if (strlen($bytes) > $width) throw new \UnexpectedValueException('Integer out of range');
        return str_pad($bytes, $width, "\0", STR_PAD_LEFT);
    }

    private static function increment(string $digits): string
    {
        $i = strlen($digits) - 1;
        while ($i >= 0 && $digits[$i] === '9') { $digits[$i] = '0'; $i--; }
        return $i < 0 ? '1' . $digits : substr_replace($digits, (string)((int)$digits[$i] + 1), $i, 1);
    }

    private static function decrement(string $digits): string
    {
        $i = strlen($digits) - 1;
        while ($digits[$i] === '0') { $digits[$i] = '9'; $i--; }
        $digits[$i] = (string)((int)$digits[$i] - 1);
        return ltrim($digits, '0') ?: '0';
    }

    /** A CBOR integer or bignum for exact integer digits. */
    private static function integer(string $text): int|CborTag
    {
        try { return (new JsonNumber($text))->integer(); }
        catch (\UnexpectedValueException) { /* Beyond 64 bits: a bignum. */ }
        $negative = str_starts_with($text, '-');
        $digits = ltrim($text, '-');
        // Tag 3 carries -1 - n, so its magnitude is one less than the value's.
        return new CborTag($negative ? 3 : 2, new CborBytes(self::fromDecimal($negative ? self::decrement($digits) : $digits)));
    }

    private static function bignum(mixed $item): ?string
    {
        if (is_int($item)) return (string)$item;
        if ($item instanceof JsonNumber) return $item->text;
        if ($item instanceof CborTag && ($item->tag === 2 || $item->tag === 3) && $item->value instanceof CborBytes) {
            $magnitude = self::toDecimal($item->value->bytes);
            return $item->tag === 2 ? $magnitude : '-' . self::increment($magnitude);
        }
        return null;
    }

    private static function floatText(float $value): JsonNumber
    {
        return new JsonNumber(ExactJson::encode($value));
    }

    /** Converts a WireCodec wire value for $target into a CBOR item. */
    public static function fromWire(WireCodec $codec, string $target, mixed $value, int $depth = 0): mixed
    {
        if ($depth > 255) throw new \UnexpectedValueException('CBOR nesting limit');
        if ($value === null) return null;
        $shape = $codec->shape($target);
        switch ($shape->get('kind')) {
            case 'structure': case 'union':
                $members = WireCodec::object($shape->get('members')); $result = [];
                foreach (WireCodec::object($value)->members as $name => $item) {
                    $member = $members->get((string)$name);
                    $result[$name] = $member instanceof JsonObject ? self::fromWire($codec, $member->get('target'), $item, $depth + 1) : self::unknownItem($item, $depth + 1);
                }
                return new JsonObject($result);
            case 'list':
                $member = WireCodec::string(WireCodec::object($shape->get('member'))->get('target'));
                return array_map(fn($item) => self::fromWire($codec, $member, $item, $depth + 1), WireCodec::items($value));
            case 'map':
                $member = WireCodec::string(WireCodec::object($shape->get('value'))->get('target'));
                return new JsonObject(array_map(fn($item) => self::fromWire($codec, $member, $item, $depth + 1), WireCodec::object($value)->members));
            case 'blob':
                $bytes = base64_decode(WireCodec::string($value), true);
                if ($bytes === false) throw new \UnexpectedValueException('Invalid blob base64');
                return new CborBytes($bytes);
            case 'timestamp':
                if (!$value instanceof JsonNumber) throw new \UnexpectedValueException('Expected epoch seconds');
                try { $epoch = $value->integer(); } catch (\UnexpectedValueException) { $epoch = (float)$value->text; }
                return new CborTag(1, $epoch);
            case 'float': case 'double':
                if (is_string($value)) return match ($value) { 'NaN' => NAN, 'Infinity' => INF, '-Infinity' => -INF, default => throw new \UnexpectedValueException('Invalid floating point string') };
                if (!$value instanceof JsonNumber) throw new \UnexpectedValueException('Expected number');
                return (float)$value->text;
            case 'byte': case 'short': case 'integer': case 'long': case 'intEnum':
                if (!$value instanceof JsonNumber) throw new \UnexpectedValueException('Expected integer');
                return $value->integer();
            case 'bigInteger':
                if (!$value instanceof JsonNumber) throw new \UnexpectedValueException('Expected integer');
                return self::integer($value->scaled(0, 100000));
            case 'bigDecimal':
                if (!$value instanceof JsonNumber) throw new \UnexpectedValueException('Expected decimal');
                [$coefficient, $exponent] = $value->canonical();
                return new CborTag(4, [(new JsonNumber($exponent))->integer(), self::integer($coefficient)]);
            case 'document':
                throw new \UnexpectedValueException('Documents are not supported by RPC v2 CBOR');
            default:
                return $value;
        }
    }

    /** Converts a decoded CBOR item into the WireCodec wire value for $target. */
    public static function toWire(WireCodec $codec, string $target, mixed $item, int $depth = 0): mixed
    {
        if ($depth > 255) throw new \UnexpectedValueException('CBOR nesting limit');
        if ($item === null) return null;
        $shape = $codec->shape($target);
        $kind = $shape->get('kind');
        switch ($kind) {
            case 'structure': case 'union': case 'unit':
                $members = $shape->get('members'); $result = [];
                foreach (WireCodec::object($item)->members as $name => $value) {
                    $member = $members instanceof JsonObject ? $members->get((string)$name) : null;
                    if ($member instanceof JsonObject) $result[$name] = self::toWire($codec, $member->get('target'), $value, $depth + 1);
                    // Unknown structure members are skipped; an unknown union variant keeps its value.
                    elseif ($kind === 'union') $result[$name] = self::wire($value, $depth + 1);
                }
                return new JsonObject($result);
            case 'list':
                if (!is_array($item) || !array_is_list($item)) throw new \UnexpectedValueException('Expected CBOR array');
                $member = WireCodec::string(WireCodec::object($shape->get('member'))->get('target'));
                return array_map(fn($value) => self::toWire($codec, $member, $value, $depth + 1), $item);
            case 'map':
                $member = WireCodec::string(WireCodec::object($shape->get('value'))->get('target'));
                return new JsonObject(array_map(fn($value) => self::toWire($codec, $member, $value, $depth + 1), WireCodec::object($item)->members));
            case 'blob':
                if (!$item instanceof CborBytes) throw new \UnexpectedValueException('Expected CBOR byte string');
                return base64_encode($item->bytes);
            case 'timestamp':
                if (!$item instanceof CborTag || $item->tag !== 1) throw new \UnexpectedValueException('Expected CBOR tag 1 epoch timestamp');
                $item = $item->value;
                if (is_int($item)) return new JsonNumber((string)$item);
                if (!is_float($item) || !is_finite($item)) throw new \UnexpectedValueException('Invalid epoch timestamp');
                return self::floatText($item);
            case 'float': case 'double':
                if (is_int($item)) return new JsonNumber((string)$item);
                if (!is_float($item)) throw new \UnexpectedValueException('Expected CBOR float');
                if (is_nan($item)) return 'NaN';
                if (is_infinite($item)) return $item > 0 ? 'Infinity' : '-Infinity';
                return self::floatText($item);
            case 'byte': case 'short': case 'integer': case 'long': case 'intEnum':
                if (is_int($item)) return new JsonNumber((string)$item);
                if ($item instanceof JsonNumber) return $item;
                throw new \UnexpectedValueException('Expected CBOR integer');
            case 'bigInteger':
                $digits = self::bignum($item);
                if ($digits === null) throw new \UnexpectedValueException('Expected CBOR integer or bignum');
                return new JsonNumber($digits);
            case 'bigDecimal':
                if ($item instanceof CborTag && $item->tag === 4) {
                    $parts = $item->value;
                    if (!is_array($parts) || count($parts) !== 2 || !array_is_list($parts) || !is_int($parts[0]) || ($mantissa = self::bignum($parts[1])) === null) {
                        throw new \UnexpectedValueException('Invalid CBOR decimal fraction');
                    }
                    return new JsonNumber($mantissa . 'e' . $parts[0]);
                }
                $digits = self::bignum($item);
                if ($digits === null) throw new \UnexpectedValueException('Expected CBOR decimal fraction');
                return new JsonNumber($digits);
            case 'string': case 'enum':
                return WireCodec::string($item);
            case 'boolean':
                if (!is_bool($item)) throw new \UnexpectedValueException('Expected CBOR boolean');
                return $item;
            default:
                return self::wire($item, $depth + 1);
        }
    }

    /** Model-free CBOR to wire conversion for unknown union variants: byte strings become base64 text. */
    private static function wire(mixed $value, int $depth): mixed
    {
        if ($depth > 255) throw new \UnexpectedValueException('CBOR nesting limit');
        return match (true) {
            is_int($value) => new JsonNumber((string)$value),
            is_float($value) => is_finite($value) ? self::floatText($value) : (is_nan($value) ? 'NaN' : ($value > 0 ? 'Infinity' : '-Infinity')),
            $value instanceof CborBytes => base64_encode($value->bytes),
            $value instanceof CborTag => self::wire($value->value, $depth + 1),
            $value instanceof JsonObject => new JsonObject(array_map(fn($item) => self::wire($item, $depth + 1), $value->members)),
            is_array($value) => array_map(fn($item) => self::wire($item, $depth + 1), $value),
            default => $value,
        };
    }

    /** Model-free wire to CBOR conversion for unknown union variants: exact integers stay integers. */
    private static function unknownItem(mixed $value, int $depth): mixed
    {
        if ($depth > 255) throw new \UnexpectedValueException('CBOR nesting limit');
        if ($value instanceof JsonNumber) {
            try { return $value->integer(); } catch (\UnexpectedValueException) { return $value->floating(); }
        }
        return match (true) {
            $value instanceof JsonObject => new JsonObject(array_map(fn($item) => self::unknownItem($item, $depth + 1), $value->members)),
            is_array($value) => array_map(fn($item) => self::unknownItem($item, $depth + 1), $value),
            default => $value,
        };
    }
}
