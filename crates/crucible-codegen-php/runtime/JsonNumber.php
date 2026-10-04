<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** A JSON number whose original decimal digits never pass through a float. */
final readonly class JsonNumber
{
    public function __construct(public string $text)
    {
        if (!preg_match('/^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?$/D', $text)) {
            throw new \UnexpectedValueException('Invalid JSON number');
        }
    }

    /** Return the exact integer at a decimal scale, without requiring bcmath. */
    public function scaled(int $places, int $maxDigits = 32): string
    {
        preg_match('/^(-?)([0-9]+)(?:\.([0-9]+))?(?:[eE]([+-]?[0-9]+))?$/D', $this->text, $m);
        $fraction = $m[3] ?? '';
        $digits = ltrim($m[2] . $fraction, '0');
        if ($digits === '') return '0';
        $exponentText = $m[4] ?? '0';
        $exponentDigits = ltrim(ltrim($exponentText, '+-'), '0');
        if (strlen($exponentDigits) > 5 || abs((int)$exponentText) > 10000) {
            throw new \UnexpectedValueException('Numeric exponent out of range');
        }
        $shift = (int)$exponentText - strlen($fraction) + $places;
        if ($shift < 0) {
            $removed = -$shift;
            if ($removed >= strlen($digits) || trim(substr($digits, -$removed), '0') !== '') {
                throw new \UnexpectedValueException('Non-integral numeric value');
            }
            $digits = substr($digits, 0, strlen($digits) - $removed);
        } else {
            if (strlen($digits) + $shift > $maxDigits) throw new \UnexpectedValueException('Numeric value out of range');
            $digits .= str_repeat('0', $shift);
        }
        if (strlen($digits) > $maxDigits) throw new \UnexpectedValueException('Numeric value out of range');
        return $m[1] . $digits;
    }

    public function integer(int $min = PHP_INT_MIN, int $max = PHP_INT_MAX): int
    {
        $text = $this->scaled(0, 19);
        $negative = str_starts_with($text, '-');
        $digits = ltrim($text, '-');
        $bound = $negative ? substr((string)PHP_INT_MIN, 1) : (string)PHP_INT_MAX;
        if (strlen($digits) > strlen($bound) || (strlen($digits) === strlen($bound) && strcmp($digits, $bound) > 0)) {
            throw new \UnexpectedValueException('Integer out of range');
        }
        $value = (int)$text;
        if ($value < $min || $value > $max) throw new \UnexpectedValueException('Integer out of range');
        return $value;
    }

    public function floating(): float
    {
        $value = (float)$this->text;
        if (!is_finite($value)) throw new \UnexpectedValueException('Non-finite number');
        return $value;
    }
}
