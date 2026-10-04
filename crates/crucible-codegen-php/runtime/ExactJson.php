<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

final class ExactJson
{
    private int $position = 0;
    private function __construct(private readonly string $source) {}

    public static function decode(string $source): mixed
    {
        $parser = new self($source);
        $value = $parser->read(0);
        $parser->space();
        if ($parser->position !== strlen($source)) throw new \UnexpectedValueException('Trailing JSON input');
        return $value;
    }

    public static function encode(mixed $value, int $depth = 0): string
    {
        if ($depth > 255) throw new \UnexpectedValueException('JSON nesting limit');
        if ($value instanceof JsonNumber) return $value->text;
        if ($value instanceof JsonObject) {
            $members = $value->members;
            ksort($members, SORT_STRING);
            $parts = [];
            foreach ($members as $key => $item) {
                $parts[] = self::encode((string)$key) . ':' . self::encode($item, $depth + 1);
            }
            return '{' . implode(',', $parts) . '}';
        }
        if (is_array($value)) {
            if (!array_is_list($value)) throw new \UnexpectedValueException('JSON objects require JsonObject');
            return '[' . implode(',', array_map(fn($item) => self::encode($item, $depth + 1), $value)) . ']';
        }
        if (!is_null($value) && !is_scalar($value)) throw new \UnexpectedValueException('Not a JSON value');
        try { return json_encode($value, JSON_THROW_ON_ERROR | JSON_PRESERVE_ZERO_FRACTION | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES); }
        catch (\JsonException $error) { throw new \UnexpectedValueException('Invalid JSON value', 0, $error); }
    }

    private function space(): void
    {
        while (isset($this->source[$this->position]) && str_contains(" \t\r\n", $this->source[$this->position])) $this->position++;
    }

    private function read(int $depth): mixed
    {
        if ($depth > 255) throw new \UnexpectedValueException('JSON nesting limit');
        $this->space();
        $character = $this->source[$this->position] ?? '';
        if ($character === '"') return $this->string();
        if ($character === '{' || $character === '[') {
            $object = $character === '{';
            $end = $object ? '}' : ']';
            $result = [];
            $this->position++;
            $this->space();
            if (($this->source[$this->position] ?? '') === $end) { $this->position++; return $object ? new JsonObject($result) : $result; }
            while (true) {
                if ($object) {
                    $key = $this->string();
                    $this->space();
                    if (($this->source[$this->position++] ?? '') !== ':') throw new \UnexpectedValueException('Expected JSON colon');
                    $result[$key] = $this->read($depth + 1);
                } else { $result[] = $this->read($depth + 1); }
                $this->space();
                $next = $this->source[$this->position++] ?? '';
                if ($next === $end) return $object ? new JsonObject($result) : $result;
                if ($next !== ',') throw new \UnexpectedValueException('Expected JSON comma');
                $this->space();
            }
        }
        foreach (['true' => true, 'false' => false, 'null' => null] as $text => $value) {
            if (substr($this->source, $this->position, strlen($text)) === $text) { $this->position += strlen($text); return $value; }
        }
        if (preg_match('/\G-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?/', $this->source, $match, 0, $this->position)) {
            $this->position += strlen($match[0]);
            return new JsonNumber($match[0]);
        }
        throw new \UnexpectedValueException('Expected JSON value');
    }

    private function string(): string
    {
        $start = $this->position;
        if (($this->source[$this->position++] ?? '') !== '"') throw new \UnexpectedValueException('Expected JSON string');
        while (isset($this->source[$this->position])) {
            $character = $this->source[$this->position++];
            if ($character === '\\') { $this->position++; continue; }
            if ($character === '"') {
                try { return json_decode(substr($this->source, $start, $this->position - $start), false, 256, JSON_THROW_ON_ERROR); }
                catch (\JsonException $error) { throw new \UnexpectedValueException('Invalid JSON string', 0, $error); }
            }
        }
        throw new \UnexpectedValueException('Unterminated JSON string');
    }
}
