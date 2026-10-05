<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** Modeled waiter acceptors and timing. */
final class Waiter
{
    public readonly float $minDelay;
    public readonly float $maxDelay;
    /** @var string|list<string>|null the message of the last matched acceptor */
    public string|array|null $message = null;
    /** @param ?\Closure(float, float): float $random a uniform sample in [min, max] */
    public function __construct(public readonly string $name, private readonly JsonObject $description, public readonly float $timeout,
        ?float $minDelay = null, ?float $maxDelay = null, private readonly ?\Closure $random = null)
    {
        $this->minDelay = $minDelay ?? self::seconds($description->get('minDelay'));
        $this->maxDelay = $maxDelay ?? self::seconds($description->get('maxDelay'));
        if (!is_finite($timeout) || $timeout <= 0) throw new \InvalidArgumentException('Waiter timeout must be finite and positive');
        if (!is_finite($this->minDelay) || !is_finite($this->maxDelay) || $this->minDelay < 0 || $this->maxDelay < $this->minDelay) {
            throw new \InvalidArgumentException('Waiter delays must be finite, nonnegative, and ordered');
        }
    }
    private static function seconds(mixed $value): float
    {
        if (!$value instanceof JsonNumber) throw new \UnexpectedValueException('Invalid waiter delay');
        return (float)$value->integer(0);
    }

    /** Returns success, failure, or retry for one attempt's query values. */
    public function state(mixed $input, mixed $output, ?\Throwable $error): string
    {
        $this->message = null;
        foreach (WireCodec::items($this->description->get('acceptors')) as $acceptor) {
            $acceptor = WireCodec::object($acceptor);
            $matcher = WireCodec::object($acceptor->get('matcher'));
            $kind = $matcher->get('kind');
            $match = false;
            if ($kind === 'success') $match = ($error === null) === ($matcher->get('expected') === true);
            elseif ($kind === 'errorType') {
                $name = WireCodec::string($matcher->get('name'));
                $short = substr($name, (int)strrpos('#' . $name, '#'));
                $match = $error instanceof ServiceError && in_array($error->metadata->name, [$name, $short], true);
            } elseif ($error === null) {
                $source = $kind === 'inputOutput' ? new JsonObject(['input' => $input, 'output' => $output]) : $output;
                $result = Query::search(WireCodec::object(WireCodec::object($matcher->get('path'))->get('expression')), $source);
                $expected = $matcher->get('expected');
                $strings = is_array($result) ? array_map(static fn($item) => $item === $expected, $result) : [];
                $match = match ($matcher->get('comparator')) {
                    'stringEquals' => is_string($result) && $result === $expected,
                    'booleanEquals' => is_bool($result) && $result === ($expected === 'true'),
                    'allStringEquals' => $strings !== [] && !in_array(false, $strings, true),
                    'anyStringEquals' => in_array(true, $strings, true),
                    default => throw new \UnexpectedValueException('Unknown waiter comparator'),
                };
            }
            if (!$match) continue;
            $message = $acceptor->get('message');
            if ($message instanceof JsonObject) {
                $text = Query::search(WireCodec::object($message->get('expression')), $output);
                if (is_string($text) || (is_array($text) && array_filter($text, static fn($item) => !is_string($item)) === [])) $this->message = $text;
            }
            return WireCodec::string($acceptor->get('state'));
        }
        return $error === null ? 'retry' : 'failure';
    }

    /** Exponential backoff with jitter, ending exactly at the deadline when the next try would not fit. */
    public function delay(int $attempt, float $remaining): float
    {
        $ceiling = min($this->maxDelay, $this->minDelay * 2 ** min($attempt - 1, 62));
        $delay = $this->random !== null ? ($this->random)($this->minDelay, $ceiling) : $this->minDelay + ($ceiling - $this->minDelay) * (random_int(0, PHP_INT_MAX) / PHP_INT_MAX);
        if ($remaining <= $this->minDelay) return $remaining;
        return $remaining - $delay <= $this->minDelay ? $remaining - $this->minDelay : $delay;
    }
}
