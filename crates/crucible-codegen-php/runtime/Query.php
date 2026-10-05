<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/**
 * Evaluates the portable JMESPath tree compiled by crucible-smithy.
 * Values are JSON shaped: null, bool, string, JsonNumber, list arrays, and JsonObject.
 * Numbers stay exact decimals; arithmetic never passes through a float.
 */
final class Query
{
    public static function search(JsonObject $node, mixed $input): mixed
    {
        return self::evaluate($node, $input);
    }

    private static function fail(string $message): never { throw new QueryException($message); }
    private static function node(mixed $value): JsonObject { return $value instanceof JsonObject ? $value : self::fail('JMESPath expected expression'); }
    private static function text(mixed $value): string { return is_string($value) ? $value : self::fail('JMESPath expected string'); }
    /** @return list<mixed> */
    private static function items(mixed $value): array { return is_array($value) && array_is_list($value) ? $value : self::fail('JMESPath expected array'); }
    private static function object(mixed $value): JsonObject { return $value instanceof JsonObject ? $value : self::fail('JMESPath expected object'); }
    private static function number(mixed $value): JsonNumber { return $value instanceof JsonNumber ? $value : self::fail('JMESPath expected number'); }
    private static function integer(mixed $value): int
    {
        try { return self::number($value)->integer(-2147483648, 2147483647); }
        catch (QueryException $error) { throw $error; }
        catch (\UnexpectedValueException) { self::fail('JMESPath integer out of range'); }
    }

    public static function truthy(mixed $value): bool
    {
        if ($value === null || $value === false || $value === '' || $value === []) return false;
        if ($value instanceof JsonObject) return $value->members !== [];
        return true;
    }

    /** JSON equality: numbers compare by value, never by spelling or PHP type juggling. */
    public static function equal(mixed $left, mixed $right): bool
    {
        if ($left instanceof JsonNumber || $right instanceof JsonNumber) {
            return $left instanceof JsonNumber && $right instanceof JsonNumber && self::compare($left, $right) === 0;
        }
        if ($left instanceof JsonObject || $right instanceof JsonObject) {
            if (!$left instanceof JsonObject || !$right instanceof JsonObject || count($left->members) !== count($right->members)) return false;
            foreach ($left->members as $key => $value) if (!$right->has((string)$key) || !self::equal($value, $right->get((string)$key))) return false;
            return true;
        }
        if (is_array($left) || is_array($right)) {
            if (!is_array($left) || !is_array($right) || count($left) !== count($right)) return false;
            foreach ($left as $index => $value) if (!self::equal($value, $right[$index])) return false;
            return true;
        }
        return $left === $right;
    }

    /** Orders two numbers or two strings (by code point). */
    public static function compare(mixed $left, mixed $right): int
    {
        if (!$left instanceof JsonNumber) return strcmp(self::text($left), self::text($right)) <=> 0;
        [$a, $ae] = self::parts(self::number($left)); [$b, $be] = self::parts(self::number($right));
        $as = $a === '0' ? 0 : (str_starts_with($a, '-') ? -1 : 1);
        $bs = $b === '0' ? 0 : (str_starts_with($b, '-') ? -1 : 1);
        if ($as !== $bs) return $as <=> $bs;
        if ($as === 0) return 0;
        $a = ltrim($a, '-'); $b = ltrim($b, '-');
        $order = ($ae + strlen($a)) <=> ($be + strlen($b));
        if ($order !== 0) return $as * $order;
        $count = max(strlen($a), strlen($b));
        return $as * (strcmp(str_pad($a, $count, '0'), str_pad($b, $count, '0')) <=> 0);
    }

    /** @return array{string, int} a signed coefficient without trailing zeros, and its exponent */
    private static function parts(JsonNumber $number): array
    {
        [$coefficient, $exponent] = $number->canonical();
        if (strlen(ltrim($exponent, '-')) > 15) self::fail('JMESPath number exponent exceeds supported precision');
        return [$coefficient, (int)$exponent];
    }

    private static function evaluate(JsonObject $node, mixed $input): mixed
    {
        $eval = static fn(string $name): mixed => self::evaluate(self::node($node->get($name)), $input);
        switch (self::text($node->get('kind'))) {
            case 'identity': return $input;
            case 'literal': return $node->get('value');
            case 'field': return $input instanceof JsonObject ? $input->get(self::text($node->get('name'))) : null;
            case 'index':
                if (!is_array($input)) return null;
                $index = self::integer($node->get('index'));
                if ($index < 0) $index += count($input);
                return $index >= 0 && $index < count($input) ? $input[$index] : null;
            case 'subexpression': return self::evaluate(self::node($node->get('right')), $eval('left'));
            case 'projection':
                $list = $eval('left');
                if (!is_array($list)) return null;
                $result = [];
                foreach ($list as $item) {
                    $value = self::evaluate(self::node($node->get('right')), $item);
                    if ($value !== null) $result[] = $value;
                }
                return $result;
            case 'flatten':
                $list = $eval('expression');
                if (!is_array($list)) return null;
                $result = [];
                foreach ($list as $item) {
                    if (is_array($item)) array_push($result, ...$item); else $result[] = $item;
                }
                return $result;
            case 'objectValues':
                $object = $eval('expression');
                return $object instanceof JsonObject ? self::values($object) : null;
            case 'multiList':
                if ($input === null) return null;
                return array_map(static fn($item) => self::evaluate(self::node($item), $input), self::items($node->get('expressions')));
            case 'multiHash':
                if ($input === null) return null;
                $result = [];
                foreach (self::items($node->get('entries')) as $entry) {
                    $entry = self::object($entry);
                    $result[self::text($entry->get('key'))] = self::evaluate(self::node($entry->get('value')), $input);
                }
                return new JsonObject($result);
            case 'and': $left = $eval('left'); return self::truthy($left) ? $eval('right') : $left;
            case 'or': $left = $eval('left'); return self::truthy($left) ? $left : $eval('right');
            case 'not': return !self::truthy($eval('expression'));
            case 'condition': return self::truthy($eval('predicate')) ? $eval('then') : null;
            case 'compare':
                $left = $eval('left'); $right = $eval('right');
                $comparator = self::text($node->get('comparator'));
                if ($comparator === 'equal') return self::equal($left, $right);
                if ($comparator === 'notEqual') return !self::equal($left, $right);
                if (!$left instanceof JsonNumber || !$right instanceof JsonNumber) return null;
                $order = self::compare($left, $right);
                return match ($comparator) {
                    'lessThan' => $order < 0, 'lessThanEqual' => $order <= 0,
                    'greaterThan' => $order > 0, 'greaterThanEqual' => $order >= 0,
                    default => self::fail('Unknown JMESPath comparator'),
                };
            case 'slice':
                if (!is_array($input)) return null;
                $step = $node->get('step') === null ? 1 : self::integer($node->get('step'));
                if ($step === 0) self::fail('Zero slice step');
                $length = count($input);
                $boundary = static function (string $name, int $fallback) use ($node, $step, $length): int {
                    if ($node->get($name) === null) return $fallback;
                    $value = self::integer($node->get($name));
                    if ($value < 0) $value += $length;
                    return $step > 0 ? min($length, max(0, $value)) : min($length - 1, max(-1, $value));
                };
                $start = $boundary('start', $step > 0 ? 0 : $length - 1);
                $stop = $boundary('stop', $step > 0 ? $length : -1);
                $result = [];
                for ($i = $start; ($step > 0 && $i < $stop) || ($step < 0 && $i > $stop); $i += $step) $result[] = $input[$i];
                return $result;
            case 'function': return self::call(self::text($node->get('function')), self::items($node->get('arguments')), $input);
        }
        self::fail('Unknown compiled JMESPath node');
    }

    /** @return list<string> */
    private static function keys(JsonObject $object): array
    {
        $keys = array_map('strval', array_keys($object->members));
        sort($keys, SORT_STRING);
        return $keys;
    }
    /** @return list<mixed> */
    private static function values(JsonObject $object): array
    {
        return array_map(static fn(string $key) => $object->get($key), self::keys($object));
    }
    /** @return list<mixed> */
    private static function ordered(mixed $value): array
    {
        $list = self::items($value);
        if ($list !== []) {
            $numeric = $list[0] instanceof JsonNumber;
            foreach ($list as $item) $numeric ? self::number($item) : self::text($item);
        }
        $indexed = array_keys($list);
        usort($indexed, static fn(int $a, int $b) => self::compare($list[$a], $list[$b]) ?: $a <=> $b);
        return array_map(static fn(int $index) => $list[$index], $indexed);
    }
    private static function reference(array $nodes, int $index, mixed $input): mixed
    {
        $node = self::node($nodes[$index]);
        if ($node->get('kind') !== 'expressionReference') self::fail('JMESPath expected expression reference');
        return self::evaluate(self::node($node->get('expression')), $input);
    }
    /** @return list<string> code points of UTF-8 text */
    private static function points(string $text): array
    {
        if (preg_match_all('/./su', $text, $matches) === false) self::fail('JMESPath string is not UTF-8');
        return $matches[0];
    }

    private static function call(string $name, array $nodes, mixed $input): mixed
    {
        $variadic = $name === 'merge' || $name === 'not_null';
        $arity = in_array($name, ['contains', 'starts_with', 'ends_with', 'join', 'map', 'max_by', 'min_by', 'sort_by'], true) ? 2 : 1;
        if (($variadic && $nodes === []) || (!$variadic && count($nodes) !== $arity)) self::fail('JMESPath invalid function arity');
        $arg = static fn(int $index): mixed => self::evaluate(self::node($nodes[$index]), $input);
        switch ($name) {
            case 'abs': return new JsonNumber(ltrim(self::number($arg(0))->text, '-'));
            case 'ceil': case 'floor': return self::round(self::number($arg(0)), $name === 'ceil');
            case 'sum': case 'avg':
                $list = self::items($arg(0));
                if ($name === 'avg' && $list === []) return null;
                [$digits, $exponent] = ['0', 0];
                foreach ($list as $item) [$digits, $exponent] = self::add($digits, $exponent, ...self::exact(self::number($item)));
                if ($name === 'sum') return self::decimal($digits, $exponent);
                return self::divide($digits, $exponent, count($list), 34);
            case 'contains':
                $source = $arg(0); $wanted = $arg(1);
                if (is_string($source)) return str_contains($source, self::text($wanted));
                foreach (self::items($source) as $item) if (self::equal($item, $wanted)) return true;
                return false;
            case 'starts_with': return str_starts_with(self::text($arg(0)), self::text($arg(1)));
            case 'ends_with': return str_ends_with(self::text($arg(0)), self::text($arg(1)));
            case 'join':
                $separator = self::text($arg(0));
                return implode($separator, array_map(static fn($item) => self::text($item), self::items($arg(1))));
            case 'keys': return self::keys(self::object($arg(0)));
            case 'values': return self::values(self::object($arg(0)));
            case 'length':
                $value = $arg(0);
                if (is_array($value)) return new JsonNumber((string)count($value));
                if ($value instanceof JsonObject) return new JsonNumber((string)count($value->members));
                return new JsonNumber((string)count(self::points(self::text($value))));
            case 'map': return array_map(static fn($item) => self::reference($nodes, 0, $item), self::items($arg(1)));
            case 'max': case 'min': case 'sort':
                $list = self::ordered($arg(0));
                if ($name === 'sort') return $list;
                if ($list === []) return null;
                return $name === 'max' ? $list[count($list) - 1] : $list[0];
            case 'max_by': case 'min_by': case 'sort_by':
                $list = self::items($arg(0));
                $keys = array_map(static fn($item) => self::reference($nodes, 1, $item), $list);
                self::ordered($keys);
                if ($list === []) return $name === 'sort_by' ? [] : null;
                if ($name === 'sort_by') {
                    $indexed = array_keys($list);
                    usort($indexed, static fn(int $a, int $b) => self::compare($keys[$a], $keys[$b]) ?: $a <=> $b);
                    return array_map(static fn(int $index) => $list[$index], $indexed);
                }
                $selected = 0;
                for ($i = 1; $i < count($list); $i++) {
                    $order = self::compare($keys[$i], $keys[$selected]);
                    if (($name === 'max_by' && $order > 0) || ($name === 'min_by' && $order < 0)) $selected = $i;
                }
                return $list[$selected];
            case 'merge':
                $result = [];
                foreach (array_keys($nodes) as $index) foreach (self::object($arg($index))->members as $key => $value) $result[$key] = $value;
                return new JsonObject($result);
            case 'not_null':
                foreach (array_keys($nodes) as $index) if (($value = $arg($index)) !== null) return $value;
                return null;
            case 'reverse':
                $value = $arg(0);
                if (is_array($value)) return array_reverse($value);
                return implode('', array_reverse(self::points(self::text($value))));
            case 'to_array': $value = $arg(0); return is_array($value) ? $value : [$value];
            case 'to_number':
                $value = $arg(0);
                if ($value instanceof JsonNumber) return $value;
                if (is_string($value) && preg_match('/^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?$/D', $value)) return new JsonNumber($value);
                return null;
            case 'to_string': $value = $arg(0); return is_string($value) ? $value : ExactJson::encode($value);
            case 'type':
                $value = $arg(0);
                return match (true) {
                    $value === null => 'null', is_bool($value) => 'boolean', is_string($value) => 'string',
                    $value instanceof JsonNumber => 'number', is_array($value) => 'array', $value instanceof JsonObject => 'object',
                    default => self::fail('Unknown JMESPath value'),
                };
        }
        self::fail('Unknown compiled JMESPath function');
    }

    /** @return array{string, int} signed integer digits and a bounded decimal exponent */
    private static function exact(JsonNumber $number): array
    {
        [$coefficient, $exponent] = self::parts($number);
        if (abs($exponent) > 1000000) self::fail('JMESPath arithmetic exponent exceeds supported precision');
        return [$coefficient, $exponent];
    }
    private static function add(string $a, int $ae, string $b, int $be): array
    {
        $exponent = min($ae, $be);
        $a .= str_repeat('0', $ae - $exponent); $b .= str_repeat('0', $be - $exponent);
        $an = str_starts_with($a, '-'); $bn = str_starts_with($b, '-');
        $a = ltrim($a, '-'); $b = ltrim($b, '-');
        if ($an === $bn) return [($an ? '-' : '') . self::sum($a, $b), $exponent];
        $order = self::magnitude($a, $b);
        if ($order === 0) return ['0', $exponent];
        return $order > 0 ? [($an ? '-' : '') . self::difference($a, $b), $exponent] : [($bn ? '-' : '') . self::difference($b, $a), $exponent];
    }
    private static function magnitude(string $a, string $b): int
    {
        $a = ltrim($a, '0'); $b = ltrim($b, '0');
        return strlen($a) <=> strlen($b) ?: strcmp($a, $b) <=> 0;
    }
    private static function sum(string $a, string $b): string
    {
        $result = ''; $carry = 0;
        for ($i = strlen($a) - 1, $j = strlen($b) - 1; $i >= 0 || $j >= 0 || $carry; $i--, $j--) {
            $digit = ($i >= 0 ? (int)$a[$i] : 0) + ($j >= 0 ? (int)$b[$j] : 0) + $carry;
            $result = ($digit % 10) . $result; $carry = intdiv($digit, 10);
        }
        return ltrim($result, '0') ?: '0';
    }
    /** @param string $a not smaller than $b */
    private static function difference(string $a, string $b): string
    {
        $result = ''; $borrow = 0;
        for ($i = strlen($a) - 1, $j = strlen($b) - 1; $i >= 0; $i--, $j--) {
            $digit = (int)$a[$i] - ($j >= 0 ? (int)$b[$j] : 0) - $borrow;
            $borrow = $digit < 0 ? 1 : 0;
            $result = ($digit + 10 * $borrow) . $result;
        }
        return ltrim($result, '0') ?: '0';
    }
    /** Formats digits * 10^exponent as a plain decimal. */
    private static function decimal(string $digits, int $exponent): JsonNumber
    {
        $negative = str_starts_with($digits, '-');
        $digits = ltrim($digits, '-0');
        if ($digits === '') return new JsonNumber('0');
        if ($exponent >= 0) $text = $digits . str_repeat('0', $exponent);
        else {
            $digits = str_pad($digits, -$exponent + 1, '0', STR_PAD_LEFT);
            $text = rtrim(substr($digits, 0, $exponent) . '.' . substr($digits, $exponent), '0');
            $text = rtrim($text, '.');
        }
        return new JsonNumber(($negative ? '-' : '') . $text);
    }
    private static function divide(string $digits, int $exponent, int $divisor, int $precision): JsonNumber
    {
        $negative = str_starts_with($digits, '-');
        $digits = ltrim($digits, '-');
        // Scale so the quotient carries one guard digit beyond the requested places;
        // floor(floor(x) / n) equals floor(x / n), so truncating first is exact.
        $shift = $exponent + $precision + 1;
        $digits = $shift >= 0 ? $digits . str_repeat('0', $shift) : (strlen($digits) + $shift > 0 ? substr($digits, 0, $shift) : '0');
        $quotient = ''; $remainder = 0;
        foreach (str_split($digits) as $digit) {
            $remainder = $remainder * 10 + (int)$digit;
            $quotient .= intdiv($remainder, $divisor);
            $remainder %= $divisor;
        }
        // Round half away from zero at the requested precision, like big.Rat.FloatString.
        $round = (int)substr($quotient, -1) >= 5;
        $quotient = ltrim(substr($quotient, 0, -1), '0') ?: '0';
        if ($round) $quotient = self::sum($quotient, '1');
        return self::decimal(($negative ? '-' : '') . $quotient, -$precision);
    }
    private static function round(JsonNumber $number, bool $ceiling): JsonNumber
    {
        [$digits, $exponent] = self::exact($number);
        if ($exponent >= 0) return self::decimal($digits, $exponent);
        $negative = str_starts_with($digits, '-');
        $magnitude = ltrim($digits, '-');
        $whole = strlen($magnitude) > -$exponent ? substr($magnitude, 0, $exponent) : '0';
        // A canonical coefficient has no trailing zeros, so a negative exponent means a fraction.
        if ($ceiling !== $negative) $whole = self::sum($whole, '1');
        return self::decimal(($negative ? '-' : '') . $whole, 0);
    }
}
