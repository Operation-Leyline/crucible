<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** Whole epoch seconds and nanoseconds, including before the Unix epoch. */
final readonly class Timestamp
{
    public function __construct(public int $seconds, public int $nanoseconds = 0)
    {
        if ($nanoseconds < 0 || $nanoseconds > 999999999) throw new \InvalidArgumentException('Nanoseconds out of range');
    }

    public static function fromEpoch(JsonNumber $number): self
    {
        $scaled = $number->scaled(9, 28);
        $negative = str_starts_with($scaled, '-');
        $digits = str_pad(ltrim($scaled, '-'), 10, '0', STR_PAD_LEFT);
        $whole = substr($digits, 0, -9);
        $fraction = (int)substr($digits, -9);
        $seconds = (new JsonNumber(($negative ? '-' : '') . (ltrim($whole, '0') ?: '0')))->integer();
        if ($negative && $fraction !== 0) {
            if ($seconds === PHP_INT_MIN) throw new \UnexpectedValueException('Timestamp out of range');
            $seconds--;
            $fraction = 1000000000 - $fraction;
        }
        return new self($seconds, $fraction);
    }

    public function toEpoch(): JsonNumber
    {
        if ($this->nanoseconds === 0) return new JsonNumber((string)$this->seconds);
        $seconds = $this->seconds;
        $fraction = $this->nanoseconds;
        if ($seconds < 0) {
            // Increment before negating: abs(PHP_INT_MIN) would become a float.
            $whole = '-' . (string)(-($seconds + 1));
            $fraction = 1000000000 - $fraction;
        } else { $whole = (string)$seconds; }
        return new JsonNumber($whole . '.' . rtrim(str_pad((string)$fraction, 9, '0', STR_PAD_LEFT), '0'));
    }

    public static function parse(string $text): self
    {
        if (!preg_match('/^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.(\d{1,9}))?(Z|[+-]\d{2}:\d{2})$/D', $text, $m)) {
            throw new \UnexpectedValueException('Invalid date-time');
        }
        if ((int)$m[1] < 1 || !checkdate((int)$m[2], (int)$m[3], (int)$m[1]) || (int)$m[4] > 23 || (int)$m[5] > 59 || (int)$m[6] > 59) {
            throw new \UnexpectedValueException('Invalid calendar date');
        }
        $zone = $m[8];
        if ($zone !== 'Z' && ((int)substr($zone, 1, 2) > 23 || (int)substr($zone, 4, 2) > 59)) throw new \UnexpectedValueException('Invalid UTC offset');
        $date = new \DateTimeImmutable(substr($text, 0, 19) . $zone);
        return new self($date->getTimestamp(), (int)str_pad($m[7] ?? '', 9, '0'));
    }

    public function toDateTime(): \DateTimeImmutable
    {
        $date = (new \DateTimeImmutable('@' . $this->seconds))->setTimezone(new \DateTimeZone('UTC'));
        return $date->setTime((int)$date->format('H'), (int)$date->format('i'), (int)$date->format('s'), intdiv($this->nanoseconds, 1000));
    }

    public static function fromDateTime(\DateTimeInterface $date): self
    {
        return new self($date->getTimestamp(), (int)$date->format('u') * 1000);
    }

    private function calendar(): \DateTimeImmutable
    {
        $date = (new \DateTimeImmutable('@' . $this->seconds))->setTimezone(new \DateTimeZone('UTC'));
        $year = (int)$date->format('Y');
        if ($year < 1 || $year > 9999) throw new \UnexpectedValueException('Text timestamps require years 1 through 9999');
        return $date;
    }

    public function toDateTimeString(): string
    {
        $fraction = $this->nanoseconds === 0 ? '' : '.' . rtrim(str_pad((string)$this->nanoseconds, 9, '0', STR_PAD_LEFT), '0');
        return $this->calendar()->format('Y-m-d\TH:i:s') . $fraction . 'Z';
    }

    public static function fromHttpDate(string $text): self
    {
        $date = \DateTimeImmutable::createFromFormat('!D, d M Y H:i:s \G\M\T', $text, new \DateTimeZone('UTC'));
        if ($date === false || $date->format('D, d M Y H:i:s \G\M\T') !== $text || (int)$date->format('Y') < 1 || (int)$date->format('Y') > 9999) {
            throw new \UnexpectedValueException('Invalid HTTP date');
        }
        return new self($date->getTimestamp());
    }

    public function toHttpDate(): string { return $this->calendar()->format('D, d M Y H:i:s \G\M\T'); }
}
