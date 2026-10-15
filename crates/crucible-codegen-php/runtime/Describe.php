<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/**
 * Formatting shared by the string representation of every generated model.
 *
 * A model prints as `TypeName(member=value, ...)` through `__toString()` (or
 * `describe()` on errors, which keep PHP's own exception text), and answers
 * `var_dump()` and `print_r()` through `__debugInfo()` with the same members.
 * Members the service left unset are omitted, nested models and collections
 * recurse, blobs show only their length, and `@sensitive` members show
 * REDACTED instead of their value.
 */
final class Describe
{
    /** What a string representation shows in place of a `@sensitive` value. */
    public const REDACTED = '*** Sensitive Data Redacted ***';

    /** Replaces a present sensitive value; an absent one stays absent so it is omitted. */
    public static function redact(mixed $value): mixed
    {
        return $value === null || $value === Absent::Value ? null : self::REDACTED;
    }

    /** Replaces a present blob with its length, so a payload never floods a log line. */
    public static function bytes(mixed $value): mixed
    {
        return is_string($value) ? '<' . strlen($value) . ' bytes>' : $value;
    }

    /**
     * The members as `var_dump()` shows them: unset members dropped.
     * @param array<string, mixed> $members
     * @return array<string, mixed>
     */
    public static function debug(array $members): array
    {
        return array_filter($members, static fn(mixed $value): bool => $value !== Absent::Value);
    }

    /** @param array<string, mixed> $members */
    public static function format(string $type, array $members): string
    {
        $parts = [];
        foreach ($members as $name => $value) {
            if ($value !== null && $value !== Absent::Value) $parts[] = $name . '=' . self::value($value);
        }
        return $type . '(' . implode(', ', $parts) . ')';
    }

    /** Renders one value the way {@see format()} renders a member. */
    public static function value(mixed $value): string
    {
        if ($value === null) return 'null';
        if (is_bool($value)) return $value ? 'true' : 'false';
        if (is_string($value)) return preg_match('//u', $value) === 1 ? $value : '<' . strlen($value) . ' bytes>';
        if (is_array($value)) {
            $items = [];
            if (array_is_list($value)) {
                foreach ($value as $item) $items[] = self::value($item);
                return '[' . implode(', ', $items) . ']';
            }
            foreach ($value as $key => $item) $items[] = $key . '=' . self::value($item);
            return '{' . implode(', ', $items) . '}';
        }
        if ($value instanceof \DateTimeInterface) return $value->format(\DateTimeInterface::ATOM);
        if ($value instanceof \Stringable) return (string)$value;
        if ($value instanceof \BackedEnum) return (string)$value->value;
        return is_object($value) ? $value::class : (string)$value;
    }
}
